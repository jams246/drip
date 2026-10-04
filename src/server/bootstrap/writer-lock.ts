import { spawn } from 'node:child_process'
import { chmod, mkdir } from 'node:fs/promises'
import { join } from 'node:path'

const PRIVATE_DIRECTORY_MODE = 0o700

export async function acquireWriterLock(dataDirectory: string): Promise<() => Promise<void>> {
  if (process.platform !== 'linux') throw new Error('Production server storage requires Linux and flock.')
  await mkdir(dataDirectory, { recursive: true, mode: 0o700 })
  await chmod(dataDirectory, PRIVATE_DIRECTORY_MODE)
  const child = spawn(
    'flock',
    [
      '--exclusive',
      '--nonblock',
      '--no-fork',
      join(dataDirectory, '.writer.lock'),
      process.execPath,
      '-e',
      "process.stdout.write('locked\\n');process.stdin.on('end',()=>process.exit(0));process.stdin.resume()"
    ],
    { stdio: ['pipe', 'pipe', 'pipe'] }
  )
  let stderr = ''
  child.stderr.on('data', (bytes: Buffer) => {
    stderr += bytes.toString()
  })
  await new Promise<void>((resolve, reject) => {
    child.once('error', reject)
    child.once('close', () => reject(new Error(stderr.trim() || 'Another server or administrative command owns this data directory.')))
    child.stdout.once('data', (bytes: Buffer) => {
      if (bytes.toString().startsWith('locked\n')) resolve()
      else reject(new Error('Could not establish the server storage lock.'))
    })
  })
  let released = false
  return async () => {
    if (released) return
    released = true
    if (child.exitCode !== null) return
    const exited = new Promise<void>((resolve) => child.once('close', () => resolve()))
    child.stdin.end()
    await exited
  }
}
