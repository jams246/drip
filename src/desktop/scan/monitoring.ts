import { queryPath } from '#drip-window-icon'
import { dirname, join } from 'node:path'
import { normalizeFileId } from '../storage/files'
import { containsPath } from './pending'
import { type NotificationPath, createPathAliases, resolveNotificationPath } from './path-aliases'
import type { ScanLocation, WatchHealth } from './types'
import { type NativeRecord, type Registration, createWatchRegistry } from './watch-registry'

const LOCAL_FIXED_DRIVE = 3
const DIRECTORY_ATTRIBUTE = 0x10
const NAME_SURROGATE_TAG = 0x20000000
const OFFLINE_OR_RECALL_ATTRIBUTES = 0x441000
const FILE_NOT_FOUND = 2
const PATH_NOT_FOUND = 3
const REMOVED_ACTION = 2
const RENAMED_OLD_ACTION = 4
const MODIFIED_ACTION = 3
const FIRST_RETRY_MS = 1000
const MAX_RETRY_MS = 30000

interface PathInfo {
  attributes: number
  reparseTag: number
  caseSensitive: boolean
  driveType: number
  error: number
}
interface Root {
  item: ScanLocation
  state: WatchHealth['state']
  error: string
  retryAt: number
  attempts: number
  aliases: ReturnType<typeof createPathAliases>
}

function inspect(path: string): PathInfo {
  const info: PathInfo = JSON.parse(queryPath(path))
  if (info.error === FILE_NOT_FOUND || info.error === PATH_NOT_FOUND) return info
  if (info.error) throw new Error(`Windows metadata error ${info.error}: ${path}`)
  if (info.driveType !== LOCAL_FIXED_DRIVE) throw new Error('Only local fixed drives are supported.')
  if ((info.reparseTag & NAME_SURROGATE_TAG) !== 0) throw new Error('Links and mounted directories cannot be watched.')
  if ((info.attributes & OFFLINE_OR_RECALL_ATTRIBUTES) !== 0) throw new Error('Online-only files cannot be watched.')
  if (info.caseSensitive) throw new Error('Case-sensitive directories cannot be watched.')
  return info
}

function inspectAncestors(path: string) {
  const available: string[] = []
  let ancestor = dirname(path)
  if (ancestor === path) return available
  while (true) {
    if (!inspect(ancestor).error) available.push(ancestor)
    const parent = dirname(ancestor)
    if (parent === ancestor) return available
    ancestor = parent
  }
}

function forceContent(record: NativeRecord, path: string) {
  if (record.action !== MODIFIED_ACTION) return true
  const metadata: PathInfo = JSON.parse(queryPath(path))
  return Boolean(metadata.error) || (metadata.attributes & DIRECTORY_ATTRIBUTE) === 0
}

export function createMonitoring(
  changed: (id: string, path: string, force: boolean) => void,
  lost: (id: string) => void,
  excluded: (path: string) => boolean = () => false
) {
  const roots: Root[] = []
  let paused = true
  const registry = createWatchRegistry(route, lost, failed)

  function failed(owners: readonly string[], error: number) {
    for (const owner of owners) {
      const root = roots.find((entry) => entry.item.id === owner)
      if (root) retry(root, `Windows watcher error ${error}`)
    }
  }

  function retry(root: Root, error: string) {
    if (root.state !== 'missing') root.state = 'error'
    root.error = error
    root.attempts++
    root.retryAt = Date.now() + Math.min(MAX_RETRY_MS, FIRST_RETRY_MS * 2 ** (root.attempts - 1))
  }

  function registerRoot(root: Root) {
    const metadata = inspect(root.item.path)
    const ancestors = inspectAncestors(root.item.path)
    root.aliases.refresh([root.item.path, ...ancestors])
    if (!metadata.error && !root.aliases.complete()) throw new Error('Windows could not resolve selected path names.')
    for (const ancestor of ancestors) registry.attach(root.item.id, ancestor, false)
    if (metadata.error) {
      root.state = 'missing'
      registry.detach(root.item.id, root.item.path)
      throw new Error(`Windows metadata error ${metadata.error}: ${root.item.path}`)
    }
    if (Boolean(metadata.attributes & DIRECTORY_ATTRIBUTE) !== (root.item.kind === 'folder')) throw new Error('Selected path has changed file type.')
    if (root.item.kind === 'folder') registry.attach(root.item.id, root.item.path, true)
  }

  function ensure(root: Root) {
    try {
      registerRoot(root)
      root.state = 'watching'
      root.error = ''
      root.retryAt = 0
      if (root.attempts > 0) {
        changed(root.item.id, root.item.path, true)
        lost(root.item.id)
      }
      root.attempts = 0
    } catch (error) {
      if (root.state !== 'missing') registry.detach(root.item.id)
      retry(root, String(error))
    }
  }

  function lifecycle(root: Root, path: string, action: number) {
    root.state = action === REMOVED_ACTION || action === RENAMED_OLD_ACTION ? 'missing' : 'starting'
    root.retryAt = 0
    changed(root.item.id, root.item.path, true)
    lost(root.item.id)
    registry.detach(root.item.id, path)
  }

  function route(registration: Registration, record: NativeRecord) {
    const rawPath = normalizeFileId(join(registration.path, record.path))
    if (!containsPath(registration.path, rawPath) || excluded(rawPath)) return
    const owners = roots.filter((root) => registration.owners.includes(root.item.id))
    if (
      !registration.recursive &&
      record.action === MODIFIED_ACTION &&
      !owners.some((root) => root.item.kind === 'file' && normalizeFileId(dirname(root.item.path)) === normalizeFileId(registration.path))
    )
      return
    const removed = record.action === REMOVED_ACTION || record.action === RENAMED_OLD_ACTION
    let expanded: NotificationPath | undefined
    for (const root of owners) {
      const cached = root.aliases.resolveCachedPath(rawPath)
      if (!registration.recursive && removed && !cached && !containsPath(rawPath, root.item.path)) continue
      let resolved: NotificationPath
      if (removed && cached) resolved = { path: cached, kind: 'exact' }
      else {
        expanded ??= resolveNotificationPath(rawPath)
        resolved = cached && expanded.kind === 'parent' ? { path: cached, kind: 'exact' } : expanded
      }
      if (!excluded(resolved.path)) interpret(root, registration, record, resolved)
    }
  }

  function reconcileUncertain(root: Root, registration: Registration, record: NativeRecord, resolved: NotificationPath) {
    if (resolved.kind === 'exact') return false
    if (!registration.recursive) {
      if (!root.aliases.complete() || root.aliases.resolveCachedPath(join(registration.path, record.path))) lost(root.item.id)
      return true
    }
    if (resolved.kind === 'parent' && root.item.kind === 'folder' && containsPath(root.item.path, resolved.path)) changed(root.item.id, resolved.path, false)
    else lost(root.item.id)
    return true
  }

  function interpret(root: Root, registration: Registration, record: NativeRecord, resolved: NotificationPath) {
    const path = resolved.path
    root.aliases.remember(path, resolved.names)
    if (reconcileUncertain(root, registration, record, resolved)) return
    if (!registration.recursive && record.action !== MODIFIED_ACTION && containsPath(path, root.item.path)) {
      lifecycle(root, path, record.action)
      return
    }
    const matches = root.item.kind === 'folder' ? containsPath(root.item.path, path) : normalizeFileId(root.item.path) === path
    if (!matches) return
    if (!registration.recursive && root.item.kind === 'folder' && record.action === MODIFIED_ACTION) return
    changed(root.item.id, path, forceContent(record, path))
  }

  function add(item: ScanLocation) {
    if (roots.some((root) => root.item.id === item.id)) return
    const root: Root = { item, state: paused ? 'paused' : 'starting', error: '', retryAt: 0, attempts: 0, aliases: createPathAliases() }
    roots.push(root)
    if (!paused) ensure(root)
  }

  function remove(id: string) {
    registry.detach(id)
    const index = roots.findIndex((root) => root.item.id === id)
    if (index >= 0) roots.splice(index, 1)
  }

  function tick() {
    if (paused) return
    registry.drain()
    for (const root of roots) if (root.state !== 'watching' && root.retryAt <= Date.now()) ensure(root)
  }

  function stop() {
    paused = true
    registry.stop()
    for (const root of roots) root.state = 'paused'
  }

  function start() {
    paused = false
    for (const root of roots) {
      root.state = 'starting'
      root.retryAt = 0
      ensure(root)
    }
  }

  return {
    add,
    remove,
    start,
    stop,
    tick,
    drain: registry.drain,
    coverage: registry.coverage,
    acknowledge: registry.acknowledge,
    checkpoint: registry.checkpoint,
    health: (): WatchHealth[] => roots.map((root) => ({ id: root.item.id, state: root.state, error: root.error }))
  }
}
