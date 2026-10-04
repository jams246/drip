import { Module } from '@nestjs/common'
import type { DynamicModule, INestApplicationContext } from '@nestjs/common'
import { APP_GUARD } from '@nestjs/core'
import { AccessApplication } from '../access/Application/access.js'
import { SqliteAccessStore } from '../access/Infrastructure/access-store.js'
import { SecureCredentials } from '../access/Infrastructure/credential-secrets.js'
import { AccessController } from '../access/Presentation/access.controller.js'
import { AccessGuard } from '../access/Presentation/access.guard.js'
import { SynchronizationApplication } from '../synchronization/Application/synchronization.js'
import { openSynchronization } from '../synchronization/Infrastructure/create-synchronization.js'
import { SynchronizationController } from '../synchronization/Presentation/synchronization.controller.js'

@Module({})
// oxlint-disable-next-line typescript/no-extraneous-class -- Nest requires a module class for composition.
class ServerModule {}

export async function openComposition(dataDirectory: string) {
  const accessStore = new SqliteAccessStore(dataDirectory)
  let synchronization: Awaited<ReturnType<typeof openSynchronization>>
  try {
    synchronization = await openSynchronization(dataDirectory)
  } catch (error) {
    accessStore.close()
    throw error
  }
  const nestModule: DynamicModule = {
    module: ServerModule,
    controllers: [AccessController, SynchronizationController],
    providers: [
      { provide: SqliteAccessStore, useValue: accessStore },
      { provide: SynchronizationApplication, useValue: synchronization.application },
      { provide: SecureCredentials, useFactory: () => new SecureCredentials() },
      {
        provide: AccessApplication,
        useFactory: (store: SqliteAccessStore, secrets: SecureCredentials, sync: SynchronizationApplication) => new AccessApplication(store, secrets, sync),
        inject: [SqliteAccessStore, SecureCredentials, SynchronizationApplication]
      },
      { provide: APP_GUARD, useClass: AccessGuard }
    ]
  }
  return {
    module: nestModule,
    synchronization,
    async recover(context: INestApplicationContext) {
      await context.get(AccessApplication).recoverRemovals()
      await synchronization.recover()
      await synchronization.collectGarbage()
    },
    async close() {
      try {
        await synchronization.close()
      } finally {
        accessStore.close()
      }
    }
  }
}
