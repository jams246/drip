import type { DatabaseSync } from 'node:sqlite'
import { SyncError } from '../Domain/errors.js'
import type { MirrorEntry, Root, RootRegistration } from '../Domain/models.js'
import { assertRootIdentity } from '../Domain/root-policy.js'
import { parseHead } from '../Domain/validation.js'
import { transaction } from './database.js'

type Row = Record<string, string | number | bigint | Uint8Array | null>

function readRoot(row: Row): Root {
  if (row.kind !== 'folder') throw new SyncError('storage_failure', 'Stored root kind is invalid.')
  return {
    rootId: String(row.id),
    deviceId: String(row.device_id),
    name: String(row.name),
    kind: row.kind,
    revision: Number(row.revision),
    retired: Boolean(row.retired)
  }
}

export class RootRecords {
  constructor(protected readonly database: DatabaseSync) {}

  assertDevice(deviceId: string) {
    if (this.database.prepare('SELECT 1 FROM device_fences WHERE device_id=?').get(deviceId)) throw new SyncError('retired', 'Device has been retired.')
  }

  registerRoot(deviceId: string, registration: RootRegistration): Root {
    this.assertDevice(deviceId)
    if (registration.rootId !== deviceId) throw new SyncError('invalid_request', 'Device root identity must match its authenticated device.')
    const existing = this.database.prepare('SELECT 1 FROM roots WHERE id=?').get(registration.rootId)
    if (existing) {
      const root = this.root(deviceId, registration.rootId)
      assertRootIdentity(root, registration)
      return root
    }
    this.database.prepare('INSERT INTO roots(id,device_id,name,kind) VALUES(?,?,?,?)').run(registration.rootId, deviceId, registration.name, registration.kind)
    return this.root(deviceId, registration.rootId)
  }

  root(deviceId: string, rootId: string): Root {
    this.assertDevice(deviceId)
    const row = this.database.prepare('SELECT * FROM roots WHERE id=? AND device_id=?').get(rootId, deviceId)
    if (!row) throw new SyncError('not_found', 'Root not found.')
    const root = readRoot(row)
    if (root.retired) throw new SyncError('retired', 'Root has been retired.')
    return root
  }

  rootIds(deviceId: string): string[] {
    return this.database
      .prepare('SELECT id FROM roots WHERE device_id=?')
      .all(deviceId)
      .map((row) => String(row.id))
  }

  heads(rootId: string, after: string, limit: number): MirrorEntry[] {
    return this.database
      .prepare('SELECT manifest FROM heads WHERE root_id=? AND path_key>? ORDER BY path_key LIMIT ?')
      .all(rootId, after, limit)
      .map((row) => parseHead(JSON.parse(String(row.manifest))))
  }

  fenceDevice(deviceId: string) {
    this.database.prepare('INSERT OR IGNORE INTO device_fences(device_id) VALUES(?)').run(deviceId)
  }

  head(rootId: string, path: string): MirrorEntry | undefined {
    const row = this.database.prepare('SELECT manifest FROM heads WHERE root_id=? AND path_key=?').get(rootId, path.toLowerCase())
    return row ? parseHead(JSON.parse(String(row.manifest))) : undefined
  }

  retire(rootId: string) {
    this.database.prepare('UPDATE roots SET retired=1 WHERE id=?').run(rootId)
  }

  removeRoot(rootId: string) {
    transaction(this.database, () => {
      this.database.prepare('DELETE FROM heads WHERE root_id=?').run(rootId)
      this.database.prepare('DELETE FROM operations WHERE root_id=?').run(rootId)
      this.database.prepare('DELETE FROM operation_fingerprints WHERE root_id=?').run(rootId)
    })
  }
}
