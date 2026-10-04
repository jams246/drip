import 'reflect-metadata'
import { NestFactory } from '@nestjs/core'
import { resolve } from 'node:path'
import { AccessApplication } from '../access/Application/access.js'
import { openComposition } from './composition.js'
import { acquireWriterLock } from './writer-lock.js'

export async function createStandalone(dataDirectory: string) {
  const path = resolve(dataDirectory)
  const release = await acquireWriterLock(path)
  let composition: Awaited<ReturnType<typeof openComposition>> | undefined
  let context: Awaited<ReturnType<typeof NestFactory.createApplicationContext>> | undefined
  try {
    composition = await openComposition(path)
    context = await NestFactory.createApplicationContext(composition.module, { logger: false, abortOnError: false })
    await context.get(AccessApplication).recoverRemovals()
    await composition.synchronization.collectGarbage()
  } catch (error) {
    try {
      await context?.close()
    } finally {
      try {
        await composition?.close()
      } finally {
        await release()
      }
    }
    throw error
  }
  const application = context
  const resources = composition
  let closed = false
  return {
    access: application.get(AccessApplication),
    async close() {
      if (closed) return
      closed = true
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
