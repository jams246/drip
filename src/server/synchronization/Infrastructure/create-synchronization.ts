import { readdir, rm } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { SynchronizationApplication } from '../Application/synchronization.js'
import { RootLocks } from '../Application/root-locks.js'
import { ChunkStorage } from './chunk-storage.js'
import { openSyncDatabase } from './database.js'
import { ensureDirectory, syncDirectory } from './durability.js'
import { MirrorStorage } from './mirror-storage.js'
import { SqliteSynchronizationRepository } from './operation-records.js'

export async function openSynchronization(dataDirectory: string) {
  const directory = resolve(dataDirectory)
  await ensureDirectory(directory)
  await ensureDirectory(join(directory, 'chunks'))
  await ensureDirectory(join(directory, 'mirrors'))
  await ensureDirectory(join(directory, 'staging'))
  const database = openSyncDatabase(join(directory, 'synchronization.sqlite'))
  const repository = new SqliteSynchronizationRepository(database)
  const chunks = new ChunkStorage(database, join(directory, 'chunks'))
  const application = new SynchronizationApplication(repository, new MirrorStorage(database, directory, chunks))
  const collections = new RootLocks()
  return {
    application,
    async recover() {
      await chunks.recover()
      await application.recover()
      for (const entry of await readdir(join(directory, 'staging'))) await rm(join(directory, 'staging', entry), { force: true })
      await syncDirectory(join(directory, 'staging'))
    },
    collectGarbage() {
      return collections.run('collection', async () => {
        repository.expire()
        await chunks.collectGarbage()
      })
    },
    async close() {
      await application.close()
      await collections.settled()
      await chunks.close()
      database.close()
    }
  }
}
