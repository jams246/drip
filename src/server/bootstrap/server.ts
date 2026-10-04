import 'reflect-metadata'
import { NestFactory } from '@nestjs/core'
import type { NestExpressApplication } from '@nestjs/platform-express'
import type { AddressInfo } from 'node:net'
import { resolve } from 'node:path'
import { AccessApplication } from '../access/Application/access.js'
import { startAdminSocket } from './admin.js'
import { openComposition } from './composition.js'
import { ServerErrorFilter } from './error-filter.js'
import { acquireWriterLock } from './writer-lock.js'

export interface ServerOptions {
  dataDir: string
  host?: string
  port?: number
  testMode?: boolean
}

const DEFAULT_PORT = 8000
const MAX_PORT = 65535
const COLLECTION_INTERVAL_MS = 60_000

export async function createServer(options: ServerOptions) {
  const host = options.host ?? '127.0.0.1'
  const port = options.port ?? Number(process.env.DRIP_PORT ?? DEFAULT_PORT)
  if (!Number.isInteger(port) || port < 0 || port > MAX_PORT) throw new Error('Port must be an integer between 0 and 65535.')
  const dataDirectory = resolve(options.dataDir)
  const release = options.testMode ? async () => {} : await acquireWriterLock(dataDirectory)
  const { application, resources } = await initializeHttp(dataDirectory).catch(async (error: unknown) => {
    await release()
    throw error
  })
  let closeAdmin: (() => Promise<void>) | undefined
  let garbageTimer: ReturnType<typeof setInterval> | undefined
  const access = application.get(AccessApplication)
  let closed = false
  let listening = false
  let collecting: Promise<void> | undefined
  return {
    app: application,
    access,
    synchronization: resources.synchronization.application,
    collectGarbage: () => resources.synchronization.collectGarbage(),
    async listen(): Promise<void> {
      if (closed) throw new Error('The server is closed.')
      if (listening) return
      await application.listen(port, host)
      listening = true
      if (!options.testMode) closeAdmin = await startAdminSocket(dataDirectory, access)
      garbageTimer = setInterval(() => {
        if (collecting) return
        collecting = resources.synchronization
          .collectGarbage()
          .catch((error: unknown) => {
            console.error('Chunk collection failed:', error instanceof Error ? error.message : String(error))
          })
          .finally(() => {
            collecting = undefined
          })
      }, COLLECTION_INTERVAL_MS)
      garbageTimer.unref()
    },
    address(): AddressInfo | null {
      const address: AddressInfo | string | null = application.getHttpServer().address()
      if (typeof address === 'string') throw new Error('The HTTP listener must use a TCP address.')
      return address
    },
    async close(): Promise<void> {
      if (closed) return
      closed = true
      clearInterval(garbageTimer)
      try {
        await closeAdmin?.()
        await collecting
      } finally {
        try {
          await application.close()
        } finally {
          try {
            await resources.close()
          } finally {
            await release()
          }
        }
      }
    }
  }
}

async function initializeHttp(dataDirectory: string) {
  const resources = await openComposition(dataDirectory)
  let application: NestExpressApplication | undefined
  try {
    application = await NestFactory.create<NestExpressApplication>(resources.module, {
      logger: false,
      abortOnError: false,
      bodyParser: false
    })
    application.useBodyParser('json', { limit: '16mb' })
    application.useGlobalFilters(new ServerErrorFilter())
    await application.init()
    await resources.recover(application)
    return { application, resources }
  } catch (error) {
    try {
      await application?.close()
    } finally {
      await resources.close()
    }
    throw error
  }
}
