/* oxlint-disable eslint/no-magic-numbers -- Win32 file attributes and error constants. */
import { queryPath } from '#drip-window-icon'
import { lstatSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { normalizeFileId } from '../storage/files'

interface WindowsPathInfo {
  attributes: number
  reparseTag: number
  caseSensitive: boolean
  driveType: number
  error: number
}

export type PathQuery = (path: string) => string

export function isDatabasePath(path: string, databasePath: string): boolean {
  const normalized = normalizeFileId(path)
  const database = normalizeFileId(databasePath)
  return normalized === database || normalized === database + '-wal' || normalized === database + '-shm' || normalized === database + '-journal'
}

export function isApplicationDataPath(path: string, databasePath: string): boolean {
  const normalized = normalizeFileId(path)
  const logs = normalizeFileId(join(dirname(databasePath), 'logs'))
  return isDatabasePath(path, databasePath) || normalized === logs || normalized.startsWith(logs + '/')
}

export function inspectScopedScanPath(path: string, selection: string, databasePath: string, query?: PathQuery): ReturnType<typeof inspectScanPath> {
  if (isApplicationDataPath(path, databasePath)) return 'excluded'
  const selectedId = normalizeFileId(selection)
  const pathId = normalizeFileId(path)
  const prefix = selectedId.endsWith('/') ? selectedId : selectedId + '/'
  if (pathId !== selectedId && !pathId.startsWith(prefix)) throw new Error('Scan target is outside its selected location.')
  let ancestor = dirname(path)
  let missingAncestor = false
  while (true) {
    const eligibility = inspectScanPath(ancestor, databasePath, query)
    if (eligibility === 'skip' || eligibility === 'excluded') return 'skip'
    if (eligibility === 'missing') missingAncestor = true
    else if (!lstatSync(ancestor).isDirectory()) throw new Error(`Scan ancestor is no longer a folder: ${ancestor}`)
    const parent = dirname(ancestor)
    if (parent === ancestor) break
    ancestor = parent
  }
  return missingAncestor ? 'missing' : inspectScanPath(path, databasePath, query)
}

export function inspectScanPath(path: string, databasePath: string, query: PathQuery = queryPath): 'eligible' | 'excluded' | 'skip' | 'missing' {
  if (isApplicationDataPath(path, databasePath)) return 'excluded'
  const result: unknown = JSON.parse(query(path))
  if (
    !result ||
    typeof result !== 'object' ||
    !('attributes' in result) ||
    typeof result.attributes !== 'number' ||
    !('reparseTag' in result) ||
    typeof result.reparseTag !== 'number' ||
    !('driveType' in result) ||
    typeof result.driveType !== 'number' ||
    !('error' in result) ||
    typeof result.error !== 'number' ||
    !('caseSensitive' in result) ||
    typeof result.caseSensitive !== 'boolean'
  )
    throw new Error('Invalid Windows path metadata.')
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- Native metadata fields are checked above.
  const info = result as WindowsPathInfo
  if (info.error === 2 || info.error === 3) return 'missing'
  if (info.error !== 0) throw new Error(`Windows path lookup failed (${info.error}): ${path}`)
  if (info.driveType !== 3) throw new Error(`Only local fixed drives are supported: ${path}`)
  if (info.caseSensitive) throw new Error(`Case-sensitive folders are not supported: ${path}`)
  const recallAttributes = 0x1000 | 0x40000 | 0x400000
  if ((info.reparseTag & 0x20000000) !== 0 || (info.attributes & recallAttributes) !== 0) return 'skip'
  return 'eligible'
}
