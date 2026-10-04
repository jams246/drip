#!/usr/bin/env node
import 'reflect-metadata'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'
import type { AdminCommand } from './admin.js'
import { executeAdmin, requestAdmin } from './admin.js'
import { createServer } from './server.js'
import { createStandalone } from './standalone.js'

const usage = `Usage:
  drip serve [--host <host>] [--port <port>]
  drip token create
  drip devices list
  drip devices remove <device-id>

Global option: --data-dir <directory> (default: DRIP_DATA_DIR or /data)
Server port: --port <port> (default: DRIP_PORT or 8000)
Registration tokens expire after 15 minutes and enroll one device.`

const REMOVE_ARGUMENT_COUNT = 3

export async function runCli(arguments_: string[]): Promise<void> {
  const { values, positionals } = parseArgs({
    args: arguments_,
    allowPositionals: true,
    options: {
      'data-dir': { type: 'string', default: process.env.DRIP_DATA_DIR ?? '/data' },
      host: { type: 'string', default: '0.0.0.0' },
      port: { type: 'string', default: process.env.DRIP_PORT ?? '8000' },
      help: { type: 'boolean' }
    }
  })
  if (values.help) {
    console.log(usage)
    return
  }
  const dataDirectory = resolve(values['data-dir'])
  if (positionals.length === 1 && positionals[0] === 'serve') {
    await serveServer(dataDirectory, values)
    return
  }
  const result = await runAdmin(dataDirectory, parseCommand(positionals))
  console.log(JSON.stringify(result, null, 2))
}

async function serveServer(dataDirectory: string, values: { host: string; port: string }): Promise<void> {
  const server = await createServer({
    dataDir: dataDirectory,
    host: values.host,
    port: Number(values.port)
  })
  try {
    await server.listen()
  } catch (error) {
    await server.close()
    throw error
  }
  console.log(`DRIP server listening on http://${values.host}:${server.address()?.port}`)
  const stop = () => {
    process.off('SIGINT', stop)
    process.off('SIGTERM', stop)
    void server.close().catch((error: unknown) => {
      console.error(error instanceof Error ? error.message : String(error))
      process.exitCode = 1
    })
  }
  process.once('SIGINT', stop)
  process.once('SIGTERM', stop)
}

async function runAdmin(dataDirectory: string, command: AdminCommand): Promise<unknown> {
  let result: unknown
  try {
    result = await requestAdmin(dataDirectory, command)
  } catch (error) {
    const code = error && typeof error === 'object' && 'code' in error ? error.code : undefined
    if (code !== 'ENOENT' && code !== 'ECONNREFUSED') throw error
    const context = await createStandalone(dataDirectory)
    try {
      result = await executeAdmin(context.access, command)
    } finally {
      await context.close()
    }
  }
  return result
}

function parseCommand(positionals: string[]): AdminCommand {
  if (positionals.length === 2 && positionals[0] === 'token' && positionals[1] === 'create') return { action: 'token-create' }
  if (positionals.length === 2 && positionals[0] === 'devices' && positionals[1] === 'list') return { action: 'devices-list' }
  if (positionals.length === REMOVE_ARGUMENT_COUNT && positionals[0] === 'devices' && positionals[1] === 'remove')
    return { action: 'devices-remove', deviceId: positionals[2] }
  throw new Error(usage)
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  await runCli(process.argv.slice(2)).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  })
}
