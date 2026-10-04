import { closeSync, fstatSync, lstatSync, openSync, readSync } from 'node:fs'
import type { Chunk, OperationReceipt } from '../../protocol/sync'
import { ChunkHasher } from '../scan/hash'
import type { SyncTransport } from './http'
import type { FrozenOperation, SyncCredentials } from './types'

export class SourceChangedError extends Error {}

// oxlint-disable-next-line eslint/max-statements -- One descriptor scope checks file metadata, reads exact bytes, and verifies the digest.
export function readUploadChunk(operation: FrozenOperation, chunk: Chunk): Uint8Array {
  if (operation.change.kind !== 'upsert') throw new Error('Only file changes upload chunks.')
  let descriptor = -1
  try {
    descriptor = openSync(operation.sourcePath, 'r')
    const before = fstatSync(descriptor)
    if (!before.isFile() || before.size !== operation.change.size || before.mtimeMs !== operation.change.modifiedMs)
      throw new SourceChangedError('Source file changed before upload.')
    const bytes = new Uint8Array(chunk.length)
    let count = 0
    while (count < bytes.length) {
      const read = readSync(descriptor, bytes, count, bytes.length - count, chunk.offset + count)
      if (read <= 0) throw new SourceChangedError('Source file ended during upload.')
      count += read
    }
    const hasher = new ChunkHasher()
    hasher.updateRange(bytes, 0, bytes.length)
    const hash = hasher.hex()
    hasher.destroy()
    const after = fstatSync(descriptor)
    const current = lstatSync(operation.sourcePath)
    if (
      hash !== chunk.hash ||
      after.size !== before.size ||
      after.mtimeMs !== before.mtimeMs ||
      after.birthtimeMs !== before.birthtimeMs ||
      !current.isFile() ||
      current.size !== before.size ||
      current.mtimeMs !== before.mtimeMs ||
      current.birthtimeMs !== before.birthtimeMs
    ) {
      throw new SourceChangedError('Source file changed during upload.')
    }
    return bytes
  } catch (error) {
    if (error instanceof SourceChangedError) throw error
    if (error && typeof error === 'object' && 'code' in error && (error.code === 'ENOENT' || error.code === 'ENOTDIR')) {
      throw new SourceChangedError('Source file disappeared before upload.')
    }
    throw error
  } finally {
    if (descriptor >= 0) closeSync(descriptor)
  }
}

export function validateReceipt(value: OperationReceipt): OperationReceipt {
  if (
    !value ||
    !['offered', 'publishing', 'committed', 'aborted'].includes(value.status) ||
    !Number.isSafeInteger(value.revision) ||
    value.revision < 0 ||
    !Array.isArray(value.missing) ||
    value.missing.some((hash) => typeof hash !== 'string' || !/^[0-9a-f]{64}$/.test(hash))
  ) {
    throw new Error('Invalid synchronization receipt.')
  }
  return value
}

export async function uploadMissing(
  transport: SyncTransport,
  credentials: SyncCredentials,
  operation: FrozenOperation,
  receipt: OperationReceipt,
  isActive: () => boolean
) {
  if (operation.change.kind !== 'upsert' && receipt.missing.length > 0) throw new Error('Delete operation requested file chunks.')
  const chunks = new Map<string, Chunk>()
  if (operation.change.kind === 'upsert') for (const chunk of operation.change.chunks) chunks.set(chunk.hash, chunk)
  const remaining = receipt.missing.slice()
  let failed = false
  async function uploadNext() {
    try {
      while (remaining.length > 0) {
        if (failed || !isActive()) return
        const hash = remaining.shift()!
        const chunk = chunks.get(hash)
        if (!chunk) throw new Error('Server requested a chunk outside the offered manifest.')
        const bytes = readUploadChunk(operation, chunk)
        await transport.request(credentials, `/v1/roots/${operation.rootId}/operations/${operation.operationId}/chunks/${hash}`, 'PUT', bytes)
      }
    } catch (error) {
      failed = true
      throw error
    }
  }
  const results = await Promise.allSettled([uploadNext(), uploadNext(), uploadNext(), uploadNext()])
  for (const result of results) if (result.status === 'rejected') throw result.reason
}
