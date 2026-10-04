import { closeSync, fstatSync, lstatSync, openSync, readSync } from 'node:fs'

export type ReadBytes = (fd: number, buffer: Uint8Array, offset: number, length: number, position: null) => number

export class FileReader {
  bytes = 0
  size = 0
  modifiedMs = 0
  private createdMs = 0
  private descriptor = -1

  constructor(private readonly path: string) {
    this.descriptor = openSync(path, 'r')
    try {
      const stats = fstatSync(this.descriptor)
      if (!stats.isFile()) throw new Error('Hash target is not a regular file.')
      this.size = stats.size
      this.modifiedMs = stats.mtimeMs
      this.createdMs = stats.birthtimeMs
    } catch (error) {
      this.close()
      throw error
    }
  }

  read(buffer: Uint8Array): number {
    return this.readRange(buffer, 0, buffer.length)
  }

  readRange(buffer: Uint8Array, offset: number, length: number): number {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || offset < 0 || length < 1 || offset + length > buffer.length)
      throw new RangeError('Invalid reader buffer range')
    if (this.descriptor < 0) throw new Error('File reader is closed.')
    const count = readSync(this.descriptor, buffer, offset, length, null)
    if (count < 0 || count > length) throw new Error('Invalid filesystem read result')
    this.bytes += count
    return count
  }

  validate() {
    const opened = fstatSync(this.descriptor)
    const current = lstatSync(this.path)
    if (
      !opened.isFile() ||
      !current.isFile() ||
      this.bytes !== this.size ||
      opened.size !== this.size ||
      current.size !== this.size ||
      opened.mtimeMs !== this.modifiedMs ||
      current.mtimeMs !== this.modifiedMs ||
      opened.birthtimeMs !== this.createdMs ||
      current.birthtimeMs !== this.createdMs
    )
      throw new Error('File changed during scan.')
  }

  close() {
    if (this.descriptor < 0) return
    const descriptor = this.descriptor
    this.descriptor = -1
    closeSync(descriptor)
  }
}

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
