import { queryNames } from '#drip-window-icon'
import { basename, dirname, join } from 'node:path'
import { normalizeFileId } from '../storage/files'

const FILE_NOT_FOUND = 2
const PATH_NOT_FOUND = 3

interface PathNames {
  longPath: string
  shortPath: string
  error: number
}
export interface NotificationPath {
  path: string
  kind: 'exact' | 'parent' | 'unknown'
  names?: PathNames
}

function readNames(path: string): PathNames {
  return JSON.parse(queryNames(path))
}

function missing(names: PathNames) {
  return names.error === FILE_NOT_FOUND || names.error === PATH_NOT_FOUND
}

function identities(path: string, names: PathNames) {
  const aliases = [path, names.longPath]
  if (names.shortPath) aliases.push(names.shortPath, join(dirname(path), basename(names.shortPath)), join(dirname(names.longPath), basename(names.shortPath)))
  return aliases.map(normalizeFileId)
}

export function createPathAliases() {
  const known = new Map<string, string[]>()
  let complete = false

  function refresh(paths: readonly string[]) {
    complete = true
    for (const path of paths) {
      const names = readNames(path)
      if (names.longPath) {
        const canonical = normalizeFileId(names.longPath)
        if (!known.has(canonical)) known.set(canonical, [])
        remember(path, names)
      }
      if (!names.longPath || !names.shortPath) complete = false
    }
  }

  function remember(path: string, names?: PathNames) {
    if (!names?.longPath) return
    const canonical = normalizeFileId(names.longPath)
    const prior = known.get(canonical)
    if (!prior) return
    for (const alias of identities(path, names)) if (!prior.includes(alias)) prior.push(alias)
  }

  function resolveCachedPath(path: string) {
    const normalized = normalizeFileId(path)
    for (const [canonical, aliases] of known) if (aliases.includes(normalized)) return canonical
    return undefined
  }

  return { refresh, remember, resolveCachedPath, complete: () => complete }
}

export function resolveNotificationPath(path: string): NotificationPath {
  const names = readNames(path)
  if (names.longPath) return { path: normalizeFileId(names.longPath), kind: 'exact', names }
  if (!missing(names)) return { path, kind: 'unknown' }
  let parent = dirname(path)
  while (true) {
    const parentNames = readNames(parent)
    if (parentNames.longPath) return { path: normalizeFileId(parentNames.longPath), kind: 'parent', names: parentNames }
    const next = dirname(parent)
    if (!missing(parentNames) || parent === next) return { path, kind: 'unknown' }
    parent = next
  }
}
