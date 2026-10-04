import { chmod, rename, rm } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import type { DatabaseSync } from 'node:sqlite'
import { SyncError } from '../Domain/errors.js'
import type { FileMove, Operation } from '../Domain/models.js'
import { ensureDirectory, syncDirectory } from './durability.js'
import { PRIVATE_FILE_MODE, finishFile, verifyFile } from './file-content.js'
import { MirrorPaths, optionalStat } from './mirror-paths.js'

const RESERVED = 1
const PUBLISHED = 2

export class MovePublication {
  constructor(
    private readonly database: DatabaseSync,
    private readonly paths: MirrorPaths
  ) {}

  async prepare(operation: Operation) {
    if (operation.change.kind !== 'move') return
    for (const move of operation.change.moves) {
      const source = await this.paths.existing(operation.deviceId, move.from)
      const stats = await optionalStat(source)
      if (!stats?.isFile() || stats.isSymbolicLink()) throw new SyncError('storage_failure', 'Move source is not a regular file.')
      await verifyFile(source, move.entry)
    }
  }

  async publish(operation: Operation) {
    if (operation.change.kind !== 'move') return
    await this.createParents(operation)
    const directory = join(this.paths.directory, 'staging', operation.operationId + '.moves')
    await ensureDirectory(directory)
    for (let index = 0; index < operation.change.moves.length; index++) {
      if (this.phase(operation.operationId, index) !== 0) continue
      const move = operation.change.moves[index]
      const temporary = join(directory, index + '.part')
      if (!(await optionalStat(temporary))) await this.reserve(operation, move, temporary)
      this.mark(operation.operationId, index, RESERVED)
    }
    for (const move of operation.change.moves) await this.paths.removeEmptyDirectories(operation.deviceId, move.entry.path)
    for (let index = 0; index < operation.change.moves.length; index++) {
      if (this.phase(operation.operationId, index) === PUBLISHED) continue
      await this.publishMove(operation, operation.change.moves[index], join(directory, index + '.part'))
      this.mark(operation.operationId, index, PUBLISHED)
    }
    await rm(directory, { recursive: true, force: true })
    await syncDirectory(dirname(directory))
  }

  private async createParents(operation: Operation) {
    if (operation.change.kind !== 'move') return
    const sources = operation.change.moves.map((move) => move.from.toLowerCase())
    for (const move of operation.change.moves) {
      const slash = move.entry.path.lastIndexOf('/')
      const parent = move.entry.path.slice(0, slash === 2 ? slash + 1 : slash)
      const key = parent.toLowerCase()
      if (sources.some((source) => key === source || key.startsWith(source + '/'))) continue
      await this.paths.directoryPath(operation.deviceId, parent, operation.operationId)
    }
  }

  private async reserve(operation: Operation, move: FileMove, temporary: string) {
    const source = await this.paths.existing(operation.deviceId, move.from)
    const current = await optionalStat(source)
    if (!current?.isFile() || current.isSymbolicLink()) throw new SyncError('storage_failure', 'Move source is not a regular file.')
    if (process.platform === 'win32') await chmod(source, PRIVATE_FILE_MODE)
    await rename(source, temporary)
    await syncDirectory(dirname(source))
    await syncDirectory(dirname(temporary))
  }

  private async publishMove(operation: Operation, move: FileMove, temporary: string) {
    const destination = await this.paths.destination(operation.deviceId, move.entry.path, operation.operationId)
    const previous = await this.paths.existing(operation.deviceId, move.entry.path)
    const pending = await optionalStat(temporary)
    if (!pending) {
      await verifyFile(destination, move.entry)
      await finishFile(destination, move.entry)
      return
    }
    await this.paths.prepareFile(destination)
    await finishFile(temporary, move.entry, PRIVATE_FILE_MODE)
    await this.paths.removePrevious(previous, destination)
    await rename(temporary, destination)
    await finishFile(destination, move.entry)
    await syncDirectory(dirname(destination))
    await syncDirectory(dirname(temporary))
  }

  private phase(operationId: string, position: number) {
    return Number(this.database.prepare('SELECT phase FROM operation_moves WHERE operation_id=? AND position=?').get(operationId, position)!.phase)
  }

  private mark(operationId: string, position: number, phase: number) {
    this.database.prepare('UPDATE operation_moves SET phase=? WHERE operation_id=? AND position=?').run(phase, operationId, position)
  }
}
