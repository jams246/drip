import { chmod, rename, rm } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import type { DatabaseSync } from 'node:sqlite'
import type { MirrorStorage as MirrorStoragePort } from '../Application/ports.js'
import { SyncError } from '../Domain/errors.js'
import type { Change, FileEntry, Operation, PlannedRegion, Root } from '../Domain/models.js'
import { syncDirectory } from './durability.js'
import { PRIVATE_FILE_MODE, finishFile, verifyFile } from './file-content.js'
import { MirrorPaths, optionalStat } from './mirror-paths.js'
import { MovePublication } from './move-publication.js'
import type { SqliteSynchronizationRepository } from './operation-records.js'
import { StagedFiles } from './staged-files.js'

export class MirrorStorage implements MirrorStoragePort {
  private readonly paths: MirrorPaths
  private readonly staged: StagedFiles
  private readonly moves: MovePublication

  constructor(
    private readonly database: DatabaseSync,
    directory: string,
    repository: SqliteSynchronizationRepository
  ) {
    this.paths = new MirrorPaths(directory)
    this.staged = new StagedFiles(repository, this.paths)
    this.moves = new MovePublication(database, this.paths)
  }

  assertPathCapacity(root: Root, change: Change) {
    this.paths.assertPathCapacity(root, change)
  }

  seed(operation: Operation) {
    return this.staged.seed(operation)
  }

  upload(operation: Operation, region: PlannedRegion, bytes: Uint8Array) {
    return this.staged.upload(operation, region, bytes)
  }

  async prepare(operation: Operation) {
    const change = operation.change
    if (change.kind === 'move') return this.moves.prepare(operation)
    if (change.kind !== 'upsert' || change.entry.kind !== 'file') return
    if (!operation.metadataOnly) return this.verifyStage(operation, change.entry)
    const source = await this.paths.existing(operation.deviceId, change.entry.path)
    const stats = await optionalStat(source)
    if (!stats?.isFile() || stats.isSymbolicLink()) throw new SyncError('storage_failure', 'Metadata update target is not a regular file.')
    await verifyFile(source, change.entry)
  }

  async publish(operation: Operation) {
    try {
      const change = operation.change
      if (change.kind === 'move') await this.moves.publish(operation)
      else if (change.kind === 'delete') await this.paths.deleteEntry(operation.deviceId, change.path)
      else if (change.entry.kind === 'directory') await this.paths.directoryPath(operation.deviceId, change.entry.path, operation.operationId)
      else await this.publishFile(operation)
    } catch (error) {
      if (error instanceof SyncError) throw error
      throw new SyncError('storage_failure', `Mirror publication failed: ${String(error)}`)
    }
  }

  async discard(operation: Operation) {
    await rm(this.staged.path(operation), { force: true })
    await rm(join(this.paths.directory, 'staging', operation.operationId + '.moves'), { recursive: true, force: true })
    await syncDirectory(join(this.paths.directory, 'staging'))
  }

  async removeRoot(deviceId: string, rootId: string) {
    await this.paths.existing(deviceId, 'C:/')
    await rm(this.paths.root(deviceId), { recursive: true, force: true })
    await syncDirectory(join(this.paths.directory, 'mirrors'))
    for (const row of this.database.prepare('SELECT id FROM operations WHERE root_id=?').all(rootId)) {
      await rm(join(this.paths.directory, 'staging', String(row.id) + '.part'), { force: true })
      await rm(join(this.paths.directory, 'staging', String(row.id) + '.moves'), { recursive: true, force: true })
    }
    await syncDirectory(join(this.paths.directory, 'staging'))
  }

  private async publishFile(operation: Operation) {
    if (operation.change.kind !== 'upsert' || operation.change.entry.kind !== 'file') return
    const entry = operation.change.entry
    const destination = await this.paths.destination(operation.deviceId, entry.path, operation.operationId)
    const previous = await this.paths.existing(operation.deviceId, entry.path)
    if (operation.metadataOnly) {
      await this.publishMetadata(previous, destination, entry)
      return
    }
    const staging = this.staged.path(operation)
    if (!(await optionalStat(staging))) {
      await verifyFile(destination, entry)
      await finishFile(destination, entry)
      await this.paths.removePrevious(previous, destination)
      return
    }
    await this.paths.prepareFile(destination)
    await this.paths.removePrevious(previous, destination)
    await rename(staging, destination)
    await finishFile(destination, entry)
    await syncDirectory(dirname(destination))
    await syncDirectory(dirname(staging))
  }

  private async verifyStage(operation: Operation, entry: FileEntry) {
    const verified = this.database.prepare('SELECT content_verified FROM operations WHERE id=?').get(operation.operationId)!.content_verified
    if (verified) {
      await verifyFile(this.staged.path(operation), entry)
      return
    }
    await this.staged.complete(operation)
    this.database.prepare('UPDATE operations SET content_verified=1 WHERE id=?').run(operation.operationId)
  }

  private async publishMetadata(previous: string, destination: string, entry: FileEntry) {
    const current = await optionalStat(previous)
    if (!current?.isFile() || current.isSymbolicLink()) throw new SyncError('storage_failure', 'Metadata update target is not a regular file.')
    if (previous !== destination) {
      await this.paths.prepareFile(destination)
      if (process.platform === 'win32') await chmod(previous, PRIVATE_FILE_MODE)
      await rename(previous, destination)
    }
    await finishFile(destination, entry)
    await syncDirectory(dirname(destination))
  }
}
