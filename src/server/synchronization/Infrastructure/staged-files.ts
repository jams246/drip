import { blake3 } from '@noble/hashes/blake3.js'
import { chmod, copyFile, open, rm } from 'node:fs/promises'
import { join } from 'node:path'
import { SyncError } from '../Domain/errors.js'
import type { Operation, PlannedRegion } from '../Domain/models.js'
import { ensureDirectory, syncDirectory } from './durability.js'
import { IO_BUFFER_BYTES, PRIVATE_FILE_MODE, finishFile, readExact, verifyFile, writeAll } from './file-content.js'
import { MirrorPaths, optionalStat } from './mirror-paths.js'
import type { SqliteSynchronizationRepository } from './operation-records.js'

const REGION_PAGE_LIMIT = 1000

export class StagedFiles {
  constructor(
    private readonly repository: SqliteSynchronizationRepository,
    private readonly paths: MirrorPaths
  ) {}

  path(operation: Operation) {
    return join(this.paths.directory, 'staging', operation.operationId + '.part')
  }

  async seed(operation: Operation) {
    const change = operation.change
    if (change.kind !== 'upsert' || change.entry.kind !== 'file' || operation.metadataOnly) return
    const staging = this.path(operation)
    await ensureDirectory(join(this.paths.directory, 'staging'))
    await rm(staging, { force: true })
    if (operation.basis) {
      const source = await this.paths.existing(operation.deviceId, operation.basis.path)
      const stats = await optionalStat(source)
      if (!stats?.isFile() || stats.isSymbolicLink() || stats.size !== operation.basis.size)
        throw new SyncError('storage_failure', 'Mirror basis is unavailable.')
      await copyFile(source, staging)
      await chmod(staging, PRIVATE_FILE_MODE)
    }
    await this.initialize(staging, operation)
    await syncDirectory(join(this.paths.directory, 'staging'))
  }

  private async initialize(staging: string, operation: Operation) {
    if (operation.change.kind !== 'upsert' || operation.change.entry.kind !== 'file') return
    const size = operation.change.entry.size
    const file = await open(staging, operation.basis ? 'r+' : 'wx', PRIVATE_FILE_MODE)
    try {
      const initialized = operation.basis ? operation.basis.size : 0
      if (operation.basis && initialized < size) {
        const zeroes = new Uint8Array(IO_BUFFER_BYTES)
        for (let offset = initialized; offset < size; offset += zeroes.length)
          await writeAll(file, zeroes.subarray(0, Math.min(zeroes.length, size - offset)), offset)
      }
      if (operation.basis) await file.truncate(size)
      await file.sync()
    } finally {
      await file.close()
    }
  }

  async upload(operation: Operation, region: PlannedRegion, bytes: Uint8Array) {
    if (bytes.length !== region.length || Buffer.from(blake3(bytes)).toString('hex') !== region.hash)
      throw new SyncError('invalid_request', 'Region bytes do not match their declared hash and length.')
    const file = await open(this.path(operation), 'r+')
    try {
      if (!operation.basis && region.offset > (await file.stat()).size) throw new SyncError('conflict', 'Upload new file regions in order without gaps.')
      await writeAll(file, bytes, region.offset)
      await file.sync()
    } finally {
      await file.close()
    }
  }

  async complete(operation: Operation) {
    if (operation.change.kind !== 'upsert' || operation.change.entry.kind !== 'file') return
    await chmod(this.path(operation), PRIVATE_FILE_MODE)
    if (operation.basis) await this.copyMatches(operation)
    await verifyFile(this.path(operation), operation.change.entry)
    await finishFile(this.path(operation), operation.change.entry, PRIVATE_FILE_MODE)
  }

  private async copyMatches(operation: Operation) {
    const source = await this.paths.existing(operation.deviceId, operation.basis!.path)
    const stats = await optionalStat(source)
    if (!stats?.isFile() || stats.isSymbolicLink()) throw new SyncError('storage_failure', 'Mirror basis is not a regular file.')
    const basis = await open(source, 'r')
    const staging = await open(this.path(operation), 'r+')
    try {
      let after = -1
      for (;;) {
        const regions = this.repository.regions(operation.operationId, after, REGION_PAGE_LIMIT)
        if (!regions.length) break
        for (const region of regions) {
          after = region.offset
          if (region.basisOffset === null) continue
          if (region.basisOffset > operation.basis!.size - region.length) throw new SyncError('storage_failure', 'Comparison range exceeds mirror basis.')
          const bytes = await readExact(basis, region.length, region.basisOffset)
          if (Buffer.from(blake3(bytes)).toString('hex') !== region.hash)
            throw new SyncError('storage_failure', 'Mirror basis region failed integrity verification.')
          await writeAll(staging, bytes, region.offset)
        }
      }
      await staging.sync()
    } finally {
      await Promise.all([basis.close(), staging.close()])
    }
  }
}
