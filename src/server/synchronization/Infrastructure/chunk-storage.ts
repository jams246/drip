import { blake3 } from '@noble/hashes/blake3.js'
import { randomUUID } from 'node:crypto'
import { lstat, open, readFile, readdir, rename, unlink } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import type { DatabaseSync } from 'node:sqlite'
import { RootLocks } from '../Application/root-locks.js'
import { SyncError } from '../Domain/errors.js'
import type { Operation } from '../Domain/models.js'
import { transaction } from './database.js'
import { ensureDirectory, isMissing, syncDirectory } from './durability.js'

const MAX_CHUNK_BYTES = 1_048_576
const GC_PAGE_SIZE = 1000
const PRIVATE_FILE_MODE = 0o600
const OFFER_LIFETIME_MS = 86_400_000

export class ChunkStorage {
  private readonly locks = new RootLocks()

  constructor(
    private readonly database: DatabaseSync,
    private readonly directory: string
  ) {}

  path(hash: string) {
    return join(this.directory, hash.slice(0, 2), hash)
  }

  async upload(operation: Operation, hash: string, bytes: Uint8Array) {
    const pin = this.database.prepare('SELECT length FROM operation_pins WHERE operation_id=? AND hash=?').get(operation.operationId, hash)
    if (!pin || bytes.length !== Number(pin.length) || bytes.length > MAX_CHUNK_BYTES || Buffer.from(blake3(bytes)).toString('hex') !== hash) {
      throw new SyncError('invalid_request', 'Chunk bytes do not match the declared hash and length.')
    }
    await this.locks.run(hash, async () => {
      this.assertActive(operation)
      await this.write(hash, bytes, operation)
      this.assertActive(operation)
      this.database
        .prepare('INSERT INTO chunks(hash,length) VALUES(?,?) ON CONFLICT(hash) DO UPDATE SET length=excluded.length,deleting=0')
        .run(hash, bytes.length)
      this.database.prepare("UPDATE operations SET expires_at=? WHERE id=? AND status='offered'").run(Date.now() + OFFER_LIFETIME_MS, operation.operationId)
    })
  }

  async read(hash: string, length: number): Promise<Uint8Array> {
    const bytes = await readFile(this.path(hash))
    if (bytes.length !== length || Buffer.from(blake3(bytes)).toString('hex') !== hash)
      throw new SyncError('storage_failure', 'Stored chunk failed integrity verification.')
    return bytes
  }

  async collectGarbage() {
    let after = ''
    for (;;) {
      const rows = this.database
        .prepare(`SELECT hash FROM chunks WHERE hash>? AND NOT EXISTS(SELECT 1 FROM operation_pins WHERE hash=chunks.hash)
        AND NOT EXISTS(SELECT 1 FROM head_chunks WHERE hash=chunks.hash) ORDER BY hash LIMIT ?`)
        .all(after, GC_PAGE_SIZE)
      if (!rows.length) break
      for (const row of rows) {
        after = String(row.hash)
        await this.remove(after)
      }
    }
    await this.removeOrphans()
  }

  async close() {
    await this.locks.settled()
  }

  async recover() {
    for (const shard of await readdir(this.directory, { withFileTypes: true })) {
      if (!shard.isDirectory() || !/^[0-9a-f]{2}$/u.test(shard.name)) continue
      for (const entry of await readdir(join(this.directory, shard.name), { withFileTypes: true })) {
        if (!entry.isFile() || !/^[0-9a-f]{64}\.[0-9a-f-]{36}\.part$/u.test(entry.name)) continue
        await unlink(join(this.directory, shard.name, entry.name))
        await syncDirectory(join(this.directory, shard.name))
      }
    }
  }

  private async write(hash: string, bytes: Uint8Array, operation: Operation) {
    const path = this.path(hash)
    await ensureDirectory(dirname(path))
    if (await this.reuse(hash, bytes.length)) return
    const temporary = path + '.' + randomUUID() + '.part'
    try {
      const file = await open(temporary, 'wx', PRIVATE_FILE_MODE)
      try {
        await file.writeFile(bytes)
        await file.sync()
      } finally {
        await file.close()
      }
      this.assertActive(operation)
      await rename(temporary, path)
      await syncDirectory(dirname(path))
    } finally {
      await unlink(temporary).catch((error: unknown) => {
        if (!isMissing(error)) throw error
      })
    }
  }

  private async reuse(hash: string, length: number): Promise<boolean> {
    try {
      await this.read(hash, length)
      const file = await open(this.path(hash), 'r')
      try {
        await file.sync()
      } finally {
        await file.close()
      }
      await syncDirectory(dirname(this.path(hash)))
      return true
    } catch (error) {
      if (!isMissing(error)) throw error
      return false
    }
  }

  private assertActive(operation: Operation) {
    const row = this.database
      .prepare(`SELECT 1 FROM operations o JOIN roots r ON r.id=o.root_id WHERE o.id=? AND o.status='offered'
      AND r.retired=0 AND NOT EXISTS(SELECT 1 FROM device_fences f WHERE f.device_id=r.device_id)`)
      .get(operation.operationId)
    if (!row) throw new SyncError('retired', 'Operation no longer accepts uploads.')
  }

  private async remove(hash: string) {
    await this.locks.run(hash, async () => {
      const claimed = transaction(
        this.database,
        () =>
          this.database
            .prepare(`UPDATE chunks SET deleting=1 WHERE hash=?
        AND NOT EXISTS(SELECT 1 FROM operation_pins WHERE hash=chunks.hash) AND NOT EXISTS(SELECT 1 FROM head_chunks WHERE hash=chunks.hash)`)
            .run(hash).changes
      )
      if (!claimed) return
      await unlink(this.path(hash)).catch((error: unknown) => {
        if (!isMissing(error)) throw error
      })
      await syncDirectory(dirname(this.path(hash)))
      this.database.prepare('DELETE FROM chunks WHERE hash=? AND deleting=1').run(hash)
    })
  }

  private async removeOrphans() {
    for (const shard of await readdir(this.directory, { withFileTypes: true })) {
      if (!shard.isDirectory() || !/^[0-9a-f]{2}$/u.test(shard.name)) continue
      for (const entry of await readdir(join(this.directory, shard.name), { withFileTypes: true })) {
        if (!entry.isFile() || !/^[0-9a-f]{64}$/u.test(entry.name)) continue
        if (this.database.prepare('SELECT 1 FROM chunks WHERE hash=?').get(entry.name)) continue
        const length = (await lstat(this.path(entry.name))).size
        this.database.prepare('INSERT OR IGNORE INTO chunks(hash,length,deleting) VALUES(?,?,1)').run(entry.name, length)
        await this.remove(entry.name)
      }
    }
  }
}
