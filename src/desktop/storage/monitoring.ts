import type { DatabaseSync, StatementSync } from 'node:sqlite'
import { transaction } from './database'

export interface DirtyPath {
  watchId: string
  path: string
  generation: number
  force: boolean
}

export class MonitoringStore {
  private readonly state: StatementSync
  private readonly pending: StatementSync
  private readonly verification: StatementSync
  private readonly updatePause: StatementSync
  private readonly updateSession: StatementSync
  private readonly insertVerification: StatementSync
  private readonly deleteVerification: StatementSync
  private readonly insertDirty: StatementSync
  private readonly deleteDirty: StatementSync
  private readonly deleteCovered: StatementSync

  constructor(private readonly database: DatabaseSync) {
    this.state = database.prepare('SELECT paused, unclean FROM application_state WHERE id = 1')
    this.pending = database.prepare('SELECT watch_id, path, generation, force FROM pending_changes')
    this.verification = database.prepare('SELECT watch_id FROM verification_pending')
    this.updatePause = database.prepare('UPDATE application_state SET paused = ? WHERE id = 1')
    this.updateSession = database.prepare('UPDATE application_state SET unclean = ? WHERE id = 1')
    this.insertVerification = database.prepare('INSERT OR IGNORE INTO verification_pending (watch_id) VALUES (?)')
    this.deleteVerification = database.prepare('DELETE FROM verification_pending WHERE watch_id = ?')
    this.insertDirty = database.prepare(`INSERT INTO pending_changes (watch_id, path, generation, force) VALUES (?, ?, ?, ?)
      ON CONFLICT(watch_id, path) DO UPDATE SET generation = excluded.generation, force = MAX(force, excluded.force)`)
    this.deleteDirty = database.prepare('DELETE FROM pending_changes WHERE watch_id = ? AND path = ? AND generation <= ?')
    this.deleteCovered = database.prepare('DELETE FROM pending_changes WHERE watch_id = ? AND generation <= ?')
  }

  restore() {
    const state = this.state.get()!
    const pending: DirtyPath[] = this.pending.all().map((row) => ({
      watchId: String(row.watch_id),
      path: String(row.path),
      generation: Number(row.generation),
      force: Boolean(row.force)
    }))
    const verification = this.verification.all().map((row) => String(row.watch_id))
    return { paused: Boolean(state.paused), unclean: Boolean(state.unclean), pending, verification }
  }

  pause(paused: boolean) {
    this.updatePause.run(paused ? 1 : 0)
  }

  session(unclean: boolean) {
    this.updateSession.run(unclean ? 1 : 0)
  }

  verify(watchId: string) {
    this.insertVerification.run(watchId)
  }

  verified(watchId: string, generation: number) {
    transaction(this.database, () => {
      this.deleteCovered.run(watchId, generation)
      this.deleteVerification.run(watchId)
    })
  }

  dirty(change: DirtyPath) {
    this.insertDirty.run(change.watchId, change.path, change.generation, change.force ? 1 : 0)
  }

  clear(change: DirtyPath) {
    this.deleteDirty.run(change.watchId, change.path, change.generation)
  }

  save(pending: readonly DirtyPath[]) {
    transaction(this.database, () => {
      this.database.exec('DELETE FROM pending_changes')
      for (const change of pending) this.dirty(change)
    })
  }
}
