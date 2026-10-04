import { readdir, rm } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { SynchronizationApplication } from '../Application/synchronization.js'
import { RootLocks } from '../Application/root-locks.js'
import { openSyncDatabase } from './database.js'
import { ensureDirectory, syncDirectory } from './durability.js'
import { MirrorStorage } from './mirror-storage.js'
import { SqliteSynchronizationRepository } from './operation-records.js'

export async function openSynchronization(dataDirectory: string) {
  const directory = resolve(dataDirectory)
  await ensureDirectory(directory)
  const database = openSyncDatabase(join(directory, 'synchronization.sqlite'))
  try {
    await ensureDirectory(join(directory, 'mirrors'))
    await ensureDirectory(join(directory, 'staging'))
  } catch (error) {
    database.close()
    throw error
  }
  const repository = new SqliteSynchronizationRepository(database)
  const application = new SynchronizationApplication(repository, new MirrorStorage(database, directory, repository))
  const collections = new RootLocks()
  return {
    application,
    async recover() {
      await application.recover()
      for (const entry of await readdir(join(directory, 'staging'))) await rm(join(directory, 'staging', entry), { recursive: true, force: true })
      await syncDirectory(join(directory, 'staging'))
    },
    expireOffers() {
      return collections.run('collection', () => application.expire())
    },
    async close() {
      await application.close()
      await collections.settled()
      database.close()
    }
  }
}
