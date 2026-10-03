import { dirname, isAbsolute, join } from 'node:path'
import { getExePath } from 'perry/updater'

export function resolveDatabasePath(): string {
  const appImage = process.env.APPIMAGE
  if (process.platform === 'win32') delete process.env.APPIMAGE
  try {
    const executablePath: unknown = getExePath()
    if (typeof executablePath !== 'string' || !isAbsolute(executablePath)) throw new Error('Could not locate the running executable.')
    return join(dirname(executablePath), 'drip.sqlite')
  } finally {
    if (process.platform === 'win32' && appImage !== undefined) process.env.APPIMAGE = appImage
  }
}
