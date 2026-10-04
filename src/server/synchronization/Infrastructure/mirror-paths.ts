import { chmod, lstat, readdir, rename, rmdir, unlink } from 'node:fs/promises'
import { dirname, join, resolve, toNamespacedPath } from 'node:path'
import { SyncError } from '../Domain/errors.js'
import type { Change, Root } from '../Domain/models.js'
import { ensureDirectory, isMissing, syncDirectory } from './durability.js'

const PRIVATE_FILE_MODE = 0o600
const DRIVE_PREFIX_LENGTH = 3
const LINUX_MAX_PATH_BYTES = 4096
const WINDOWS_MAX_PATH_UNITS = 32767

export class MirrorPaths {
  constructor(readonly directory: string) {}

  root(deviceId: string) {
    return join(this.directory, 'mirrors', deviceId)
  }

  path(deviceId: string, source: string) {
    return join(this.root(deviceId), ...components(source))
  }

  assertPathCapacity(root: Root, change: Change) {
    const paths =
      change.kind === 'move' ? change.moves.flatMap((move) => [move.from, move.entry.path]) : [change.kind === 'delete' ? change.path : change.entry.path]
    for (const source of paths) {
      const path = resolve(this.path(root.deviceId, source))
      if (Buffer.byteLength(path, 'utf8') >= LINUX_MAX_PATH_BYTES || toNamespacedPath(path).length >= WINDOWS_MAX_PATH_UNITS)
        throw new SyncError('invalid_request', 'Source path exceeds mirror storage limits.')
    }
  }

  async existing(deviceId: string, source: string): Promise<string> {
    let current = this.root(deviceId)
    await assertDirectory(join(this.directory, 'mirrors'))
    for (const component of components(source)) {
      const stats = await optionalStat(current)
      if (stats && (!stats.isDirectory() || stats.isSymbolicLink())) throw new SyncError('storage_failure', 'Mirror ancestor is not a regular directory.')
      const names = stats ? await readdir(current) : []
      const matches = names.filter((name) => name.toLowerCase() === component.toLowerCase())
      if (matches.length > 1) throw new SyncError('storage_failure', 'Mirror contains conflicting path casing.')
      current = join(current, matches[0] ?? component)
    }
    return current
  }

  async directoryPath(deviceId: string, source: string, operationId: string) {
    return this.ensure(deviceId, components(source), operationId)
  }

  async destination(deviceId: string, source: string, operationId: string) {
    const parts = components(source)
    const name = parts.pop()!
    return join(await this.ensure(deviceId, parts, operationId), name)
  }

  async prepareFile(path: string) {
    const current = await optionalStat(path)
    if (!current) return
    if (current.isDirectory() && !current.isSymbolicLink()) {
      await rmdir(path)
      await syncDirectory(dirname(path))
      return
    }
    if (current.isSymbolicLink() || !current.isFile()) throw new SyncError('storage_failure', 'Mirror destination is not a regular file.')
    if (process.platform === 'win32') await chmod(path, PRIVATE_FILE_MODE)
  }

  async removeEmptyDirectories(deviceId: string, source: string) {
    const path = await this.existing(deviceId, source)
    const current = await optionalStat(path)
    if (!current?.isDirectory() || current.isSymbolicLink()) return
    await removeDirectoryTree(path)
  }

  async deleteEntry(deviceId: string, source: string) {
    const path = await this.existing(deviceId, source)
    const current = await optionalStat(path)
    if (!current) return
    if (current.isSymbolicLink()) throw new SyncError('storage_failure', 'Mirror contains an unexpected link.')
    if (current.isDirectory()) await rmdir(path)
    else if (current.isFile()) {
      if (process.platform === 'win32') await chmod(path, PRIVATE_FILE_MODE)
      await unlink(path)
    } else throw new SyncError('storage_failure', 'Mirror contains an unsupported entry.')
    await syncDirectory(dirname(path))
  }

  async removePrevious(previous: string, destination: string) {
    if (previous === destination) return
    const old = await optionalStat(previous)
    if (!old) return
    const current = await optionalStat(destination)
    if (old.dev === current?.dev && old.ino === current.ino) return
    if (!old.isFile() || old.isSymbolicLink()) throw new SyncError('storage_failure', 'Previous mirror entry is not a regular file.')
    if (process.platform === 'win32') await chmod(previous, PRIVATE_FILE_MODE)
    await unlink(previous)
    await syncDirectory(dirname(previous))
  }

  private async ensure(deviceId: string, parts: string[], operationId: string) {
    await assertDirectory(join(this.directory, 'mirrors'))
    let current = this.root(deviceId)
    await assertDirectory(current)
    await ensureDirectory(current)
    for (let index = 0; index < parts.length; index++) {
      const component = parts[index]
      const target = join(current, component)
      const temporary = join(current, '.' + operationId + '-case-' + index)
      const pending = await optionalStat(temporary)
      if (pending) {
        if (!pending.isDirectory() || pending.isSymbolicLink()) throw new SyncError('storage_failure', 'Invalid casing recovery directory.')
        await rename(temporary, target)
        await syncDirectory(current)
      }
      await ensureComponent(current, component, temporary)
      current = target
    }
    return current
  }
}

async function removeDirectoryTree(path: string): Promise<void> {
  for (const name of await readdir(path)) {
    const child = join(path, name)
    const stats = await optionalStat(child)
    if (!stats?.isDirectory() || stats.isSymbolicLink()) throw new SyncError('storage_failure', 'Unreserved mirror entry prevents directory replacement.')
    await removeDirectoryTree(child)
  }
  await rmdir(path)
  await syncDirectory(dirname(path))
}

async function ensureComponent(parent: string, component: string, temporary: string) {
  const names = await readdir(parent)
  const matches = names.filter((name) => name.toLowerCase() === component.toLowerCase())
  if (matches.length > 1) throw new SyncError('storage_failure', 'Mirror contains conflicting path casing.')
  const target = join(parent, component)
  if (!matches.length) {
    await ensureDirectory(target)
    return
  }
  const previous = join(parent, matches[0])
  await assertDirectory(previous)
  if (matches[0] === component) return
  await rename(previous, temporary)
  await syncDirectory(parent)
  await rename(temporary, target)
  await syncDirectory(parent)
}

export async function optionalStat(path: string) {
  try {
    return await lstat(path)
  } catch (error) {
    if (!isMissing(error)) throw error
    return undefined
  }
}

async function assertDirectory(path: string) {
  const stats = await optionalStat(path)
  if (stats && (!stats.isDirectory() || stats.isSymbolicLink())) throw new SyncError('storage_failure', 'Mirror ancestor is not a regular directory.')
}

function components(source: string) {
  const rest = source.slice(DRIVE_PREFIX_LENGTH)
  return rest ? [source[0], ...rest.split('/')] : [source[0]]
}
