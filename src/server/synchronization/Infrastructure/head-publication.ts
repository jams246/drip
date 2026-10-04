import type { DatabaseSync } from 'node:sqlite'
import type { MirrorEntry, Operation } from '../Domain/models.js'
import { parseHead } from '../Domain/validation.js'

export function replaceHeads(database: DatabaseSync, operation: Operation) {
  const change = operation.change
  if (change.kind === 'delete') {
    database.prepare('DELETE FROM heads WHERE root_id=? AND path_key=?').run(operation.rootId, change.path.toLowerCase())
    return
  }
  if (change.kind === 'move') {
    const regions = change.moves.map((move) =>
      database
        .prepare('SELECT offset,length,hash FROM head_regions WHERE root_id=? AND path_key=? ORDER BY offset')
        .all(operation.rootId, move.from.toLowerCase())
    )
    for (const move of change.moves) {
      database.prepare('DELETE FROM heads WHERE root_id=? AND path_key=?').run(operation.rootId, move.from.toLowerCase())
      const prefix = move.entry.path.toLowerCase() + '/'
      database.prepare('DELETE FROM heads WHERE root_id=? AND substr(path_key,1,?)=?').run(operation.rootId, prefix.length, prefix)
    }
    change.moves.forEach((move, index) => {
      putHead(database, operation.rootId, move.entry)
      for (const region of regions[index])
        database
          .prepare('INSERT INTO head_regions VALUES(?,?,?,?,?)')
          .run(operation.rootId, move.entry.path.toLowerCase(), Number(region.offset), Number(region.length), String(region.hash))
      rebaseCasing(database, operation.rootId, move.entry.path, false)
    })
    return
  }
  if (operation.metadataOnly) {
    database
      .prepare('UPDATE heads SET manifest=? WHERE root_id=? AND path_key=?')
      .run(JSON.stringify(change.entry), operation.rootId, change.entry.path.toLowerCase())
  } else {
    putHead(database, operation.rootId, change.entry)
    if (change.entry.kind === 'file')
      database
        .prepare('INSERT INTO head_regions SELECT ?,?,offset,length,hash FROM operation_regions WHERE operation_id=?')
        .run(operation.rootId, change.entry.path.toLowerCase(), operation.operationId)
  }
  rebaseCasing(database, operation.rootId, change.entry.path, change.entry.kind === 'directory')
}

function putHead(database: DatabaseSync, rootId: string, entry: MirrorEntry) {
  database.prepare('DELETE FROM heads WHERE root_id=? AND path_key=?').run(rootId, entry.path.toLowerCase())
  database.prepare('INSERT INTO heads VALUES(?,?,?)').run(rootId, entry.path.toLowerCase(), JSON.stringify(entry))
}

function rebaseCasing(database: DatabaseSync, rootId: string, path: string, includeLeaf: boolean) {
  const parts = path.split('/')
  const count = includeLeaf ? parts.length : parts.length - 1
  for (let depth = 1; depth <= count; depth++) {
    const prefix = parts.slice(0, depth).join('/')
    const key = prefix.toLowerCase()
    for (const row of database
      .prepare('SELECT path_key,manifest FROM heads WHERE root_id=? AND (path_key=? OR substr(path_key,1,?)=?)')
      .all(rootId, key, key.length + 1, key + '/')) {
      const entry = parseHead(JSON.parse(String(row.manifest)))
      const displayPath = prefix + entry.path.slice(prefix.length)
      if (entry.path !== displayPath)
        database
          .prepare('UPDATE heads SET manifest=? WHERE root_id=? AND path_key=?')
          .run(JSON.stringify({ ...entry, path: displayPath }), rootId, String(row.path_key))
    }
  }
}
