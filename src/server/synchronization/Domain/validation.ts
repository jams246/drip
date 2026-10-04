import { MAX_REGION_BYTES, REGION_PAGE_LIMIT } from '../../../protocol/sync.js'
import { SyncError } from './errors.js'
import type { MirrorEntry, Offer, RegionFingerprint, RootRegistration } from './models.js'

const MAX_NAME_LENGTH = 255
const MAX_PATH_LENGTH = 4096
const MAX_COMPONENT_BYTES = 255
const MAX_MODIFICATION_MS = 8_640_000_000_000_000
const DRIVE_ROOT_LENGTH = 3
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u
const HASH = /^[0-9a-f]{64}$/u
const WINDOWS_RESERVED_NAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu
const UTF8 = new TextEncoder()

function invalid(message: string): never {
  throw new SyncError('invalid_request', message)
}

export function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) invalid('Expected a JSON object.')
  // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- The object boundary is checked; individual properties remain unknown.
  return value as Record<string, unknown>
}

export function validateId(value: unknown): string {
  if (typeof value !== 'string' || !UUID.test(value)) invalid('Expected a canonical UUID.')
  return value
}

export function validateHash(value: unknown): string {
  if (typeof value !== 'string' || !HASH.test(value)) invalid('Expected a lowercase BLAKE3-256 hash.')
  return value
}

export function validatePath(value: unknown): string {
  if (typeof value !== 'string' || value.length > MAX_PATH_LENGTH || !value.isWellFormed() || !/^[A-Z]:\//u.test(value))
    invalid('Expected an absolute local drive path.')
  const rest = value.slice(DRIVE_ROOT_LENGTH)
  // oxlint-disable-next-line eslint/no-control-regex -- Windows filenames cannot contain control characters.
  if (/[\\:\u0000-\u001f<>"|?*]/u.test(rest)) invalid('Invalid source path.')
  if (
    rest &&
    rest
      .split('/')
      .some(
        (part) =>
          !part || part === '.' || part === '..' || /[. ]$/u.test(part) || WINDOWS_RESERVED_NAME.test(part) || UTF8.encode(part).length > MAX_COMPONENT_BYTES
      )
  )
    invalid('Invalid source path.')
  return value
}

export function nonnegativeInteger(value: unknown, label: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) invalid(`Invalid ${label}.`)
  return value
}

export function parseRootRegistration(value: unknown): RootRegistration {
  const input = record(value)
  if (typeof input.name !== 'string' || !input.name.trim() || input.name.length > MAX_NAME_LENGTH) invalid('Invalid root name.')
  if (input.kind !== 'folder') invalid('Device root must be a folder.')
  return { rootId: validateId(input.rootId), name: input.name, kind: input.kind }
}

export function parseHead(value: unknown): MirrorEntry {
  const entry = record(value)
  const path = validatePath(entry.path)
  if (entry.kind === 'directory') return { kind: entry.kind, path }
  if (entry.kind !== 'file' || path.length === DRIVE_ROOT_LENGTH) invalid('Invalid file entry.')
  const size = nonnegativeInteger(entry.size, 'file size')
  if (typeof entry.modifiedMs !== 'number' || !Number.isFinite(entry.modifiedMs) || Math.abs(entry.modifiedMs) > MAX_MODIFICATION_MS)
    invalid('Invalid modification time.')
  return { kind: entry.kind, path, size, modifiedMs: entry.modifiedMs, hash: validateHash(entry.hash) }
}

export function parseOffer(value: unknown): Offer {
  const input = record(value)
  const change = record(input.change)
  const common = { operationId: validateId(input.operationId), baseRevision: nonnegativeInteger(input.baseRevision, 'base revision') }
  if (change.kind === 'delete') return { ...common, change: { kind: change.kind, path: validatePath(change.path) } }
  if (change.kind === 'upsert') return { ...common, change: { kind: change.kind, entry: parseHead(change.entry) } }
  if (change.kind !== 'move' || !Array.isArray(change.moves) || !change.moves.length) invalid('Invalid move batch.')
  const sources = new Set<string>()
  const destinations = new Set<string>()
  const moves = change.moves.map((item: unknown) => {
    const move = record(item)
    const from = validatePath(move.from)
    const entry = parseHead(move.entry)
    if (entry.kind !== 'file' || sources.has(from.toLowerCase()) || destinations.has(entry.path.toLowerCase())) invalid('Invalid file move.')
    sources.add(from.toLowerCase())
    destinations.add(entry.path.toLowerCase())
    return { from, entry }
  })
  for (const path of destinations) {
    const parts = path.split('/').slice(0, -1)
    while (parts.length) {
      if (destinations.has(parts.join('/'))) invalid('Move destinations cannot contain another moved file.')
      parts.pop()
    }
  }
  return { ...common, change: { kind: 'move', moves } }
}

export function parseRegions(value: unknown): RegionFingerprint[] {
  const input = record(value)
  if (!Array.isArray(input.regions) || !input.regions.length || input.regions.length > REGION_PAGE_LIMIT) invalid('Invalid region page.')
  return input.regions.map((item: unknown) => {
    const region = record(item)
    const offset = nonnegativeInteger(region.offset, 'region offset')
    const length = nonnegativeInteger(region.length, 'region length')
    if (!length || length > MAX_REGION_BYTES || !Number.isSafeInteger(offset + length)) invalid('Invalid region length.')
    return { offset, length, hash: validateHash(region.hash) }
  })
}
