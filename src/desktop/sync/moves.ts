import type { Change, FileEntry } from '../../protocol/sync'
import type { Pending } from './pending'

const MOVE_BATCH_LIMIT = 1000
const MAX_MOVE_BODY_BYTES = 16_773_120
const UTF8 = new TextEncoder()
interface Candidate {
  from: string
  entry: FileEntry
  source: Pending
  target: Pending
}

function ancestors(path: string) {
  const parents: string[] = []
  let slash = path.lastIndexOf('/')
  while (slash > 2) {
    const parent = path.slice(0, slash)
    parents.push(parent)
    slash = parent.lastIndexOf('/')
  }
  return parents
}

// oxlint-disable-next-line eslint/max-statements -- Index source fingerprints and namespace dependencies before matching targets.
function candidates(pending: Pending[]) {
  const basis = new Map<string, { rows: Pending[]; index: number }>()
  const structural = new Set<string>()
  for (const row of pending) {
    if (row.previous?.kind !== 'file') continue
    if (row.change.kind === 'upsert' && row.change.entry.kind === 'directory') structural.add(row.pathKey)
    if (
      row.change.kind === 'upsert' &&
      row.change.entry.kind === 'file' &&
      row.change.entry.hash === row.previous.hash &&
      row.change.entry.size === row.previous.size
    )
      continue
    const key = row.previous.hash + ':' + row.previous.size
    if (!basis.has(key)) basis.set(key, { rows: [], index: 0 })
    basis.get(key)!.rows.push(row)
  }
  const used = new Set<string>()
  const moves: Candidate[] = []
  for (const target of pending) {
    if (target.change.kind !== 'upsert' || target.change.entry.kind !== 'file') continue
    const entry = target.change.entry
    const own =
      target.previous?.kind === 'file' && target.previous.hash === entry.hash && target.previous.size === entry.size && target.previous.path !== entry.path
        ? target
        : undefined
    const bucket = basis.get(entry.hash + ':' + entry.size)
    if (bucket) while (bucket.index < bucket.rows.length && used.has(bucket.rows[bucket.index].pathKey)) bucket.index++
    const source = own && !used.has(own.pathKey) ? own : bucket?.rows[bucket.index]
    if (source?.previous?.kind !== 'file' || source.previous.path === entry.path) continue
    if (ancestors(target.pathKey).some((parent) => structural.has(parent) && parent !== source.pathKey)) continue
    used.add(source.pathKey)
    moves.push({ from: source.previous.path, entry, source, target })
  }
  return moves
}

// oxlint-disable-next-line eslint/max-statements -- Namespace dependencies identify moves that must reserve their sources together.
function moveComponents(pending: Pending[], available: Candidate[]) {
  const bySource = new Map(available.map((move, index) => [move.from.toLowerCase(), index]))
  const byTarget = new Map(available.map((move, index) => [move.entry.path.toLowerCase(), index]))
  const children = new Map<number, string[]>()
  const nested = new Map<number, number[]>()
  for (const row of pending) {
    if (row.previous?.kind !== 'file' && !(row.change.kind === 'delete' && row.previous?.kind !== 'directory')) continue
    for (const parent of ancestors(row.pathKey)) {
      const target = byTarget.get(parent)
      if (target === undefined) continue
      if (!children.has(target)) children.set(target, [])
      children.get(target)!.push(row.pathKey)
      const source = bySource.get(row.pathKey)
      if (source === undefined) continue
      if (!nested.has(target)) nested.set(target, [])
      if (!nested.has(source)) nested.set(source, [])
      nested.get(target)!.push(source)
      nested.get(source)!.push(target)
    }
  }
  const visited = new Set<number>()
  const components: { moves: Candidate[]; blocked: boolean }[] = []
  for (let index = 0; index < available.length; index++) {
    if (visited.has(index)) continue
    const component: Candidate[] = []
    const positions: number[] = []
    const waiting = [index]
    while (waiting.length) {
      const current = waiting.pop()!
      if (visited.has(current)) continue
      visited.add(current)
      const move = available[current]
      component.push(move)
      positions.push(current)
      const successor = bySource.get(move.entry.path.toLowerCase())
      const predecessor = byTarget.get(move.from.toLowerCase())
      if (successor !== undefined) waiting.push(successor)
      if (predecessor !== undefined) waiting.push(predecessor)
      for (const child of nested.get(current) ?? []) waiting.push(child)
    }
    const sources = new Set(component.map((move) => move.source.pathKey))
    const blocked = positions.some((position) => children.get(position)?.some((child) => !sources.has(child)))
    components.push({ moves: component, blocked })
  }
  return components
}

// oxlint-disable-next-line eslint/max-statements -- Select complete components within the publication budget and freeze their operation members.
export function movedFiles(pending: Pending[], protectedSources = new Set<string>()) {
  const selected: Candidate[] = []
  let bytes = 0
  for (const component of moveComponents(pending, candidates(pending))) {
    if (component.blocked) {
      const sources = new Set(component.moves.map((move) => move.source.pathKey))
      for (const source of sources) {
        protectedSources.add(source)
        for (const parent of ancestors(source)) protectedSources.add(parent)
      }
      continue
    }
    let componentBytes = 0
    for (const move of component.moves) componentBytes += UTF8.encode(JSON.stringify({ from: move.from, entry: move.entry })).length + 1
    if (bytes + componentBytes > MAX_MOVE_BODY_BYTES) continue
    if (selected.length && selected.length + component.moves.length > MOVE_BATCH_LIMIT) continue
    selected.push(...component.moves)
    bytes += componentBytes
    if (selected.length >= MOVE_BATCH_LIMIT) break
  }
  if (!selected.length) return undefined
  const members = new Map<string, Pending>()
  const moves = selected.map((move) => {
    members.set(move.target.pathKey, move.target)
    members.set(move.source.pathKey, move.source)
    return { from: move.from, entry: move.entry }
  })
  const targets = new Set(selected.map((move) => move.target.pathKey))
  for (const row of pending) {
    if (row.change.kind === 'delete' && row.previous?.kind === 'directory' && ancestors(row.pathKey).some((parent) => targets.has(parent))) {
      members.set(row.pathKey, row)
    }
  }
  return { change: { kind: 'move', moves } as Change, members: [...members.values()] }
}
