import { mkdir, open } from 'node:fs/promises'
import { dirname, resolve, toNamespacedPath } from 'node:path'

export async function syncDirectory(path: string) {
  // Windows functional runs cannot prove Linux directory durability.
  if (process.platform === 'win32') return
  const directory = await open(path, 'r')
  try {
    await directory.sync()
  } finally {
    await directory.close()
  }
}

export async function ensureDirectory(path: string) {
  const created = await mkdir(path, { recursive: true, mode: 0o755 })
  if (created) {
    const parent = dirname(toNamespacedPath(resolve(created)))
    let current = toNamespacedPath(resolve(path))
    while (current !== parent) {
      await syncDirectory(current)
      const next = dirname(current)
      if (next === current) throw new Error('Directory durability traversal cannot reach its created parent.')
      current = next
    }
    await syncDirectory(parent)
  }
}

export function isMissing(error: unknown): boolean {
  return Boolean(error && typeof error === 'object' && 'code' in error && error.code === 'ENOENT')
}
