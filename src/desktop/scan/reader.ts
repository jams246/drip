import { closeSync, openSync, readSync } from 'node:fs'

export type ReadBytes = (fd: number, buffer: Uint8Array, offset: number, length: number, position: null) => number

export function consumeFile(
  path: string,
  buffer: Uint8Array,
  consume: (buffer: Uint8Array, count: number, offset: number) => void,
  read: ReadBytes = readSync
): number {
  if (buffer.length === 0) throw new RangeError('Reader buffer must not be empty')
  const fd = openSync(path, 'r')
  let offset = 0
  try {
    while (true) {
      const count = read(fd, buffer, 0, buffer.length, null)
      if (count < 0 || count > buffer.length) throw new Error('Invalid filesystem read result')
      if (count === 0) break
      consume(buffer, count, offset)
      offset += count
    }
  } finally {
    closeSync(fd)
  }
  // Perry 0.5.1520 narrows private numeric returns hidden inside try/while.
  return offset
}
