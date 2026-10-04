import { chmod, lstat, open, rename, rm, rmdir, unlink } from 'node:fs/promises'
import { dirname, join, resolve, sep, toNamespacedPath } from 'node:path'
import type { DatabaseSync } from 'node:sqlite'
import type { MirrorStorage as MirrorStoragePort } from '../Application/ports.js'
import { SyncError } from '../Domain/errors.js'
import { type Change, type Operation, type Root, canonicalPath } from '../Domain/models.js'
import { parseHead } from '../Domain/validation.js'
import { ChunkStorage } from './chunk-storage.js'
import { ensureDirectory, isMissing, syncDirectory } from './durability.js'

const PRIVATE_FILE_MODE = 0o600
const READ_ONLY_FILE_MODE = 0o444
const LINUX_MAX_PATH_BYTES = 4096
const WINDOWS_MAX_PATH_UNITS = 32767

export class MirrorStorage implements MirrorStoragePort {
  constructor(
    private readonly database: DatabaseSync,
    private readonly directory: string,
    private readonly chunks: ChunkStorage
  ) {}

  assertPathCapacity(root: Root, change: Change) {
    const destination = resolve(this.rootPath(root.deviceId, root.rootId), change.path)
    if (Buffer.byteLength(destination, 'utf8') >= LINUX_MAX_PATH_BYTES || toNamespacedPath(destination).length >= WINDOWS_MAX_PATH_UNITS)
      throw new SyncError('invalid_request', 'File path exceeds mirror storage limits.')
  }

  async upload(operation: Operation, hash: string, bytes: Uint8Array) {
    try {
      await this.chunks.upload(operation, hash, bytes)
    } catch (error) {
      if (error instanceof SyncError) throw error
      throw new SyncError('storage_failure', `Chunk storage failed: ${String(error)}`)
    }
  }

  async publish(operation: Operation) {
    try {
      await this.publishChange(operation)
    } catch (error) {
      if (error instanceof SyncError) throw error
      throw new SyncError('storage_failure', `File publication failed: ${String(error)}`)
    }
  }

  async removeRoot(deviceId: string, rootId: string) {
    const root = this.rootPath(deviceId, rootId)
    await this.assertDirectories(root, join(this.directory, 'mirrors'))
    await rm(root, { recursive: true, force: true })
    const staging = join(this.directory, 'staging')
    for (const operation of this.database.prepare("SELECT id FROM operations WHERE root_id=? AND status='publishing'").all(rootId)) {
      await rm(join(staging, String(operation.id) + '.part'), { force: true })
    }
    await syncDirectory(staging)
    await syncDirectory(dirname(root)).catch((error: unknown) => {
      if (!isMissing(error)) throw error
    })
    try {
      await rmdir(dirname(root))
      await syncDirectory(join(this.directory, 'mirrors'))
    } catch (error) {
      if (!isMissing(error) && !isNotEmpty(error)) throw error
    }
  }

  private rootPath(deviceId: string, rootId: string) {
    return join(this.directory, 'mirrors', deviceId, rootId)
  }

  private async publishChange(operation: Operation) {
    const root = this.rootPath(operation.deviceId, operation.rootId)
    const row = this.database.prepare('SELECT manifest FROM heads WHERE root_id=? AND path_key=?').get(operation.rootId, canonicalPath(operation.change.path))
    const previous = row ? parseHead(JSON.parse(String(row.manifest))) : undefined
    if (operation.change.kind === 'delete') {
      if (previous) await this.deleteFile(root, previous.path)
      return
    }
    await ensureDirectory(root)
    const destination = resolve(root, operation.change.path)
    if (!destination.startsWith(resolve(root) + sep)) throw new SyncError('invalid_request', 'File path escapes root.')
    await this.assertDirectories(dirname(destination), root)
    await ensureDirectory(dirname(destination))
    await this.prepareDestination(destination)
    const staging = join(this.directory, 'staging')
    await ensureDirectory(staging)
    const temporary = join(staging, operation.operationId + '.part')
    await this.reconstruct(temporary, operation)
    await rename(temporary, destination)
    await syncDirectory(dirname(destination))
    await syncDirectory(staging)
    if (previous && previous.path !== operation.change.path) await this.deletePreviousPath(root, previous.path, destination)
  }

  private async reconstruct(path: string, operation: Operation) {
    if (operation.change.kind !== 'upsert') return
    await rm(path, { force: true })
    const file = await open(path, 'wx', PRIVATE_FILE_MODE)
    try {
      for (const chunk of operation.change.chunks) await file.writeFile(await this.chunks.read(chunk.hash, chunk.length))
      await file.utimes(new Date(operation.change.modifiedMs), new Date(operation.change.modifiedMs))
      await file.chmod(READ_ONLY_FILE_MODE)
      await file.sync()
    } finally {
      await file.close()
    }
  }

  private async prepareDestination(path: string) {
    try {
      const current = await lstat(path)
      if (current.isSymbolicLink()) throw new SyncError('storage_failure', 'Mirror contains an unexpected link.')
      if (current.isDirectory()) {
        await rmdir(path)
        await syncDirectory(dirname(path))
      } else if (!current.isFile()) throw new SyncError('storage_failure', 'Mirror contains an unsupported entry.')
      else if (process.platform === 'win32') await chmod(path, PRIVATE_FILE_MODE)
    } catch (error) {
      if (!isMissing(error)) throw error
    }
  }

  private async deletePreviousPath(root: string, previousPath: string, destination: string) {
    const previous = await lstat(resolve(root, previousPath)).catch((error: unknown) => {
      if (!isMissing(error)) throw error
      return undefined
    })
    if (!previous) return
    const current = await lstat(destination)
    if (previous.dev === current.dev && previous.ino === current.ino) return
    await this.deleteFile(root, previousPath)
  }

  private async deleteFile(root: string, relativePath: string) {
    const path = resolve(root, relativePath)
    await this.assertDirectories(dirname(path), root)
    try {
      const current = await lstat(path)
      if (!current.isFile() || current.isSymbolicLink()) throw new SyncError('storage_failure', 'Delete target is not a regular file.')
      if (process.platform === 'win32') await chmod(path, PRIVATE_FILE_MODE)
      await unlink(path)
      await syncDirectory(dirname(path))
      await this.pruneDirectories(dirname(path), root)
    } catch (error) {
      if (!isMissing(error)) throw error
      await this.pruneDirectories(dirname(path), root)
    }
  }

  private async pruneDirectories(directory: string, root: string) {
    while (directory !== root && directory.startsWith(root + sep)) {
      try {
        await rmdir(directory)
        await syncDirectory(dirname(directory))
      } catch (error) {
        if (isNotEmpty(error)) return
        if (!isMissing(error)) throw error
      }
      directory = dirname(directory)
    }
  }

  private async assertDirectories(directory: string, root: string) {
    let current = directory
    while (current === root || current.startsWith(root + sep)) {
      try {
        const entry = await lstat(current)
        if (!entry.isDirectory() || entry.isSymbolicLink()) throw new SyncError('storage_failure', 'Mirror ancestor is not a regular directory.')
      } catch (error) {
        if (!isMissing(error)) throw error
      }
      if (current === root) return
      current = dirname(current)
    }
  }
}

function isNotEmpty(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && 'code' in error && (error.code === 'ENOTEMPTY' || error.code === 'EEXIST'))
}
