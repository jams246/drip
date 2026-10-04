import { SyncError } from './errors.js'
import type { Chunk, Offer, RootRegistration } from './models.js'

const MAX_NAME_LENGTH = 255
const MAX_PATH_LENGTH = 4096
const MAX_COMPONENT_BYTES = 255
const MAX_MODIFICATION_MS = 8_640_000_000_000_000
const MAX_CHUNK_BYTES = 1_048_576
const MAX_CHUNKS = 100_000
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u
const HASH = /^[0-9a-f]{64}$/u
const UTF8 = new TextEncoder()

function invalid(message: string): never {
  throw new SyncError('invalid_request', message)
}

function record(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) invalid('Expected a JSON object.')
  return value
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value))
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
  if (typeof value !== 'string' || !value || value.length > MAX_PATH_LENGTH || !value.isWellFormed()) invalid('Invalid relative file path.')
  if (value.includes('\\') || value.includes(':') || value.includes('\0') || value.startsWith('/')) invalid('Invalid relative file path.')
  if (value.split('/').some((part) => !part || part === '.' || part === '..' || UTF8.encode(part).length > MAX_COMPONENT_BYTES))
    invalid('Invalid relative file path.')
  return value
}

export function nonnegativeInteger(value: unknown, label: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) invalid(`Invalid ${label}.`)
  return value
}

export function parseRootRegistration(value: unknown): RootRegistration {
  const input = record(value)
  if (typeof input.name !== 'string' || !input.name.trim() || input.name.length > MAX_NAME_LENGTH) invalid('Invalid root name.')
  if (input.kind !== 'file' && input.kind !== 'folder') invalid('Invalid root kind.')
  return { rootId: validateId(input.rootId), name: input.name, kind: input.kind }
}

function parseChunks(value: unknown, size: number): Chunk[] {
  if (!Array.isArray(value) || value.length > MAX_CHUNKS) invalid('Invalid chunk manifest.')
  let offset = 0
  const lengths = new Map<string, number>()
  const chunks = value.map((item: unknown) => {
    const chunk = record(item)
    const length = nonnegativeInteger(chunk.length, 'chunk length')
    if (!length || length > MAX_CHUNK_BYTES || chunk.offset !== offset) invalid('Chunk manifest must be contiguous and bounded.')
    const result = { offset, length, hash: validateHash(chunk.hash) }
    const previous = lengths.get(result.hash)
    if (previous !== undefined && previous !== length) invalid('Repeated chunk hashes must have identical lengths.')
    lengths.set(result.hash, length)
    offset += length
    return result
  })
  if (offset !== size) invalid('Chunk lengths must equal file size.')
  return chunks
}

export function parseOffer(value: unknown): Offer {
  const input = record(value)
  const change = record(input.change)
  const common = { operationId: validateId(input.operationId), baseRevision: nonnegativeInteger(input.baseRevision, 'base revision') }
  const path = validatePath(change.path)
  if (change.kind === 'delete') return { ...common, change: { kind: 'delete', path } }
  if (change.kind !== 'upsert' || change.profile !== 'fastcdc-v1-blake3-256') invalid('Unsupported change or chunk profile.')
  const size = nonnegativeInteger(change.size, 'file size')
  if (typeof change.modifiedMs !== 'number' || !Number.isFinite(change.modifiedMs) || change.modifiedMs < 0 || change.modifiedMs > MAX_MODIFICATION_MS)
    invalid('Invalid modification time.')
  return { ...common, change: { kind: 'upsert', path, profile: change.profile, size, modifiedMs: change.modifiedMs, chunks: parseChunks(change.chunks, size) } }
}

export function parseHead(value: unknown): import('./models.js').FileHead {
  const head = record(value)
  const size = nonnegativeInteger(head.size, 'file size')
  if (typeof head.modifiedMs !== 'number' || !Number.isFinite(head.modifiedMs) || head.modifiedMs < 0 || head.modifiedMs > MAX_MODIFICATION_MS)
    invalid('Invalid modification time.')
  return { path: validatePath(head.path), size, modifiedMs: head.modifiedMs, chunks: parseChunks(head.chunks, size) }
}
