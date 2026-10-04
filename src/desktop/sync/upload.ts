import type { FileEntry, OperationReceipt, RegionPage } from '../../protocol/sync'
import { MAX_REGION_BYTES, REGION_PAGE_LIMIT } from '../../protocol/sync'
import { lstatSync } from 'node:fs'
import { ContentHasher } from '../scan/hash'
import { FileReader } from '../scan/reader'
import type { SyncTransport } from './http'
import type { SyncStore } from './store'
import type { FrozenOperation, SyncCredentials } from './types'

export class SourceChangedError extends Error {}

export function validateReceipt(value: OperationReceipt): OperationReceipt {
  if (
    !value ||
    !['offered', 'publishing', 'committed', 'aborted'].includes(value.status) ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 0 ||
    typeof value.planComplete !== 'boolean' ||
    typeof value.stageReady !== 'boolean' ||
    typeof value.uploadRequired !== 'boolean'
  )
    throw new Error('Invalid synchronization receipt.')
  return value
}

function endpoint(operation: FrozenOperation) {
  return `/v1/roots/${operation.rootId}/operations/${operation.operationId}`
}
const yieldIo = () => new Promise<void>((resolve) => setTimeout(resolve, 0))
const STAGE_POLL_MS = 100
const IO_SLICE_BYTES = 16_384

export function validateMetadata(path: string, entry: FileEntry, createdMs?: number) {
  try {
    const current = lstatSync(path)
    if (
      !current.isFile() ||
      current.size !== entry.size ||
      current.mtimeMs !== entry.modifiedMs ||
      (createdMs !== undefined && current.birthtimeMs !== createdMs)
    )
      throw new SourceChangedError('Source file changed before commit.')
    return current.birthtimeMs
  } catch (error) {
    if (error && typeof error === 'object' && 'code' in error && (error.code === 'ENOENT' || error.code === 'ENOTDIR')) {
      throw new SourceChangedError('Source file disappeared before commit.')
    }
    throw error
  }
}

async function sourceFile(path: string, entry: FileEntry, consume: (reader: FileReader, hasher: ContentHasher) => Promise<void>) {
  let reader: FileReader | undefined
  const hasher = new ContentHasher()
  try {
    reader = new FileReader(path)
    if (reader.size !== entry.size || reader.modifiedMs !== entry.modifiedMs) throw new SourceChangedError('Source file changed before upload.')
    await consume(reader, hasher)
    reader.validate()
    if (hasher.hex() !== entry.hash) throw new SourceChangedError('Source file changed during upload.')
  } catch (error) {
    if (error instanceof SourceChangedError) throw error
    if (error instanceof Error && (error.message.includes('File changed') || ('code' in error && (error.code === 'ENOENT' || error.code === 'ENOTDIR')))) {
      throw new SourceChangedError('Source file changed during upload.')
    }
    throw error
  } finally {
    reader?.close()
    hasher.destroy()
  }
}

export async function validateSource(path: string, entry: FileEntry, active: () => boolean) {
  await sourceFile(path, entry, async (reader, hasher) => {
    const buffer = new Uint8Array(IO_SLICE_BYTES)
    while (true) {
      if (!active()) throw new SourceChangedError('Source changed before commit.')
      const count = reader.read(buffer)
      if (!count) return
      hasher.updateRange(buffer, 0, count)
      await yieldIo()
    }
  })
}

async function requiredRegions(transport: SyncTransport, credentials: SyncCredentials, operation: FrozenOperation, after: number) {
  const page = await transport.request<RegionPage>(credentials, endpoint(operation) + `/regions?after=${after}&limit=${REGION_PAGE_LIMIT}`)
  if (!page || !Array.isArray(page.required) || page.required.length > REGION_PAGE_LIMIT) throw new Error('Invalid region transfer plan.')
  let previous = after
  for (const offset of page.required) {
    if (!Number.isSafeInteger(offset) || offset <= previous) throw new Error('Invalid region transfer offsets.')
    previous = offset
  }
  if (page.next !== null && (!page.required.length || page.next !== previous)) throw new Error('Invalid region transfer cursor.')
  return page
}

async function prepareUploadPlan(
  transport: SyncTransport,
  credentials: SyncCredentials,
  store: SyncStore,
  operation: FrozenOperation,
  receipt: OperationReceipt,
  active: () => boolean
) {
  let planned = receipt
  if (!receipt.planComplete) {
    let after = -1
    while (true) {
      if (!active()) throw new SourceChangedError('Source changed while planning upload.')
      const regions = store.regions(operation, after, REGION_PAGE_LIMIT)
      if (!regions.length) break
      await transport.request(credentials, endpoint(operation) + '/regions', 'POST', { regions })
      after = regions[regions.length - 1].offset
    }
    planned = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation) + '/regions/complete', 'POST'))
  }
  while (!planned.stageReady) {
    if (!active()) throw new SourceChangedError('Source changed while preparing staging.')
    await new Promise<void>((resolve) => setTimeout(resolve, STAGE_POLL_MS))
    planned = validateReceipt(await transport.request<OperationReceipt>(credentials, endpoint(operation)))
    if (planned.status !== 'offered') throw new SourceChangedError('Transfer plan is no longer accepting uploads.')
  }
}

export async function uploadMissing(
  transport: SyncTransport,
  credentials: SyncCredentials,
  store: SyncStore,
  operation: FrozenOperation,
  receipt: OperationReceipt,
  active: () => boolean,
  transferred: (bytes: number) => void
) {
  if (operation.change.kind !== 'upsert' || operation.change.entry.kind !== 'file') return
  const entry = operation.change.entry
  if (!receipt.uploadRequired && receipt.planComplete) return validateSource(operation.sourcePath, entry, active)
  await prepareUploadPlan(transport, credentials, store, operation, receipt, active)
  let required = await requiredRegions(transport, credentials, operation, -1)
  let requiredIndex = 0
  async function advanceRequired() {
    if (requiredIndex < required.required.length || required.next === null) return
    required = await requiredRegions(transport, credentials, operation, required.next)
    requiredIndex = 0
  }
  // oxlint-disable-next-line eslint/max-statements -- One descriptor validates ordered regions and transfers bounded literals.
  await sourceFile(operation.sourcePath, entry, async (reader, whole) => {
    const bytes = new Uint8Array(MAX_REGION_BYTES)
    let after = -1
    let offset = 0
    while (true) {
      const regions = store.regions(operation, after, REGION_PAGE_LIMIT)
      if (!regions.length) break
      for (const region of regions) {
        if (!active()) throw new SourceChangedError('Source changed during upload.')
        if (region.offset !== offset || region.length <= 0 || region.length > MAX_REGION_BYTES) throw new Error('Invalid frozen file fingerprints.')
        const hasher = new ContentHasher()
        try {
          let count = 0
          while (count < region.length) {
            if (!active()) throw new SourceChangedError('Source changed during upload.')
            const read = reader.readRange(bytes, count, Math.min(IO_SLICE_BYTES, region.length - count))
            if (!read) throw new SourceChangedError('Source file ended during upload.')
            hasher.updateRange(bytes, count, read)
            whole.updateRange(bytes, count, read)
            count += read
            await yieldIo()
          }
          if (hasher.hex() !== region.hash) throw new SourceChangedError('Source region changed during upload.')
        } finally {
          hasher.destroy()
        }
        if (!active()) throw new SourceChangedError('Source changed during upload.')
        await advanceRequired()
        if (!active()) throw new SourceChangedError('Source changed during upload.')
        const requiredOffset = required.required[requiredIndex]
        if (requiredOffset !== undefined && requiredOffset < region.offset) throw new Error('Server requested an unknown file region.')
        if (requiredOffset === region.offset) {
          await transport.request(credentials, endpoint(operation) + '/regions/' + region.offset, 'PUT', bytes, region.length)
          transferred(region.length)
          requiredIndex++
        }
        offset += region.length
        after = region.offset
      }
    }
    if (!active()) throw new SourceChangedError('Source changed during upload.')
    await advanceRequired()
    if (!active()) throw new SourceChangedError('Source changed during upload.')
    if (requiredIndex < required.required.length || offset !== entry.size) throw new Error('Server requested regions outside the frozen file.')
  })
}
