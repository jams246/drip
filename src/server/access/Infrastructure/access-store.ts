import { timingSafeEqual } from 'node:crypto'
import { chmodSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import type { SQLOutputValue } from 'node:sqlite'
import type { AccessRepository } from '../Application/ports.js'
import type { Device, StoredDevice } from '../Domain/device.js'

const PRIVATE_FILE_MODE = 0o600

function readDeviceState(state: SQLOutputValue): StoredDevice['state'] {
  if (state !== 'active' && state !== 'deleting') throw new Error('Invalid device state in the access database.')
  return state
}

function readDevice(row: Record<string, SQLOutputValue>): StoredDevice {
  return {
    deviceId: String(row.device_id),
    name: String(row.name),
    state: readDeviceState(row.state),
    createdAt: Number(row.created_ms)
  }
}

function matchesDigest(left: string, right: string): boolean {
  const stored = Buffer.from(left, 'hex')
  const supplied = Buffer.from(right, 'hex')
  return stored.length === supplied.length && timingSafeEqual(stored, supplied)
}

export class SqliteAccessStore implements AccessRepository {
  private readonly database: DatabaseSync

  constructor(dataDirectory: string) {
    mkdirSync(dataDirectory, { recursive: true, mode: 0o700 })
    const path = join(dataDirectory, 'access.sqlite')
    this.database = new DatabaseSync(path, { timeout: 5000 })
    try {
      this.initialize(path)
    } catch (error) {
      this.database.close()
      throw error
    }
  }

  private initialize(path: string): void {
    chmodSync(path, PRIVATE_FILE_MODE)
    this.database.exec('PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;')
    const version = Number(this.database.prepare('PRAGMA user_version').get()?.user_version)
    if (version > 1) {
      throw new Error('This access database requires a newer server.')
    }
    this.database.exec(`
      CREATE TABLE IF NOT EXISTS access_tokens (
        digest TEXT PRIMARY KEY, expires_ms INTEGER NOT NULL, consumed_device_id TEXT
      );
      CREATE TABLE IF NOT EXISTS access_devices (
        device_id TEXT PRIMARY KEY, secret_digest TEXT NOT NULL, name TEXT NOT NULL,
        state TEXT NOT NULL CHECK (state IN ('active', 'deleting')), created_ms INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS retired_devices (device_id TEXT PRIMARY KEY);
      PRAGMA user_version=1;
    `)
  }

  createToken(digest: string, expiresAt: number, now: number): void {
    this.transaction(() => {
      this.database.prepare('DELETE FROM access_tokens WHERE consumed_device_id IS NULL AND expires_ms <= ?').run(now)
      this.database.prepare('INSERT INTO access_tokens (digest, expires_ms) VALUES (?, ?)').run(digest, expiresAt)
    })
  }

  enroll(tokenDigest: string, deviceId: string, secretDigest: string, name: string, now: number): 'ok' | 'invalid' | 'retired' {
    return this.transaction(() => {
      if (this.isRetired(deviceId)) return 'retired'
      const token = this.database.prepare('SELECT expires_ms, consumed_device_id FROM access_tokens WHERE digest = ?').get(tokenDigest)
      if (!token) return 'invalid'
      const device = this.findDevice(deviceId)
      if (token.consumed_device_id !== null) {
        if (token.consumed_device_id !== deviceId || !device || device.state !== 'active') return 'invalid'
        return matchesDigest(device.secret_digest, secretDigest) && device.name === name ? 'ok' : 'invalid'
      }
      if (Number(token.expires_ms) <= now || device) return 'invalid'
      this.database.prepare('INSERT INTO access_devices VALUES (?, ?, ?, ?, ?)').run(deviceId, secretDigest, name, 'active', now)
      this.database.prepare('UPDATE access_tokens SET consumed_device_id = ? WHERE digest = ?').run(deviceId, tokenDigest)
      return 'ok'
    })
  }

  authenticate(deviceId: string, secretDigest: string): Device | null {
    const device = this.findDevice(deviceId)
    if (!device || device.state !== 'active' || !matchesDigest(device.secret_digest, secretDigest)) return null
    return { deviceId, name: device.name }
  }

  listDevices(): StoredDevice[] {
    return this.database.prepare('SELECT device_id, name, state, created_ms FROM access_devices ORDER BY created_ms, device_id').all().map(readDevice)
  }

  markDeleting(deviceId: string): boolean {
    return this.transaction(() => {
      if (this.isRetired(deviceId)) return true
      const result = this.database.prepare("UPDATE access_devices SET state = 'deleting' WHERE device_id = ?").run(deviceId)
      return Number(result.changes) !== 0
    })
  }

  finishRemoval(deviceId: string): void {
    this.transaction(() => {
      this.database.prepare('INSERT OR IGNORE INTO retired_devices VALUES (?)').run(deviceId)
      this.database.prepare('DELETE FROM access_tokens WHERE consumed_device_id = ?').run(deviceId)
      this.database.prepare('DELETE FROM access_devices WHERE device_id = ?').run(deviceId)
    })
  }

  listDeleting(): string[] {
    return this.database
      .prepare("SELECT device_id FROM access_devices WHERE state = 'deleting' ORDER BY device_id")
      .all()
      .map((row) => String(row.device_id))
  }

  close(): void {
    this.database.close()
  }

  private findDevice(deviceId: string) {
    const row = this.database.prepare('SELECT secret_digest, name, state FROM access_devices WHERE device_id = ?').get(deviceId)
    if (!row) return undefined
    return { secret_digest: String(row.secret_digest), name: String(row.name), state: readDeviceState(row.state) }
  }

  private isRetired(deviceId: string): boolean {
    return this.database.prepare('SELECT 1 FROM retired_devices WHERE device_id = ?').get(deviceId) !== undefined
  }

  private transaction<T>(action: () => T): T {
    this.database.exec('BEGIN IMMEDIATE')
    try {
      const result = action()
      this.database.exec('COMMIT')
      return result
    } catch (error) {
      this.database.exec('ROLLBACK')
      throw error
    }
  }
}
