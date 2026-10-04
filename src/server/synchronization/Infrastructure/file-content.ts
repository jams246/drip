import { blake3 } from '@noble/hashes/blake3.js'
import { chmod, open } from 'node:fs/promises'
import type { FileHandle } from 'node:fs/promises'
import { SyncError } from '../Domain/errors.js'
import type { FileEntry } from '../Domain/models.js'

export const IO_BUFFER_BYTES = 65_536
export const PRIVATE_FILE_MODE = 0o600
export const READ_ONLY_FILE_MODE = 0o444
const MILLISECONDS_PER_SECOND = 1000

export async function writeAll(file: FileHandle, bytes: Uint8Array, position: number) {
  let written = 0
  while (written < bytes.length) {
    const result = await file.write(bytes, written, bytes.length - written, position + written)
    if (!result.bytesWritten) throw new SyncError('storage_failure', 'File write made no progress.')
    written += result.bytesWritten
  }
}

export async function readExact(file: FileHandle, length: number, position: number): Promise<Uint8Array> {
  const bytes = new Uint8Array(length)
  let read = 0
  while (read < length) {
    const result = await file.read(bytes, read, length - read, position + read)
    if (!result.bytesRead) throw new SyncError('storage_failure', 'Mirror basis ended unexpectedly.')
    read += result.bytesRead
  }
  return bytes
}

export async function verifyFile(path: string, entry: FileEntry) {
  const file = await open(path, 'r')
  const hasher = blake3.create()
  try {
    const stats = await file.stat()
    if (!stats.isFile() || stats.size !== entry.size) throw new SyncError('storage_failure', 'Mirrored file size does not match its entry.')
    const buffer = new Uint8Array(IO_BUFFER_BYTES)
    let position = 0
    while (position < entry.size) {
      const { bytesRead } = await file.read(buffer, 0, Math.min(buffer.length, entry.size - position), position)
      if (!bytesRead) throw new SyncError('storage_failure', 'Mirrored file ended unexpectedly.')
      hasher.update(buffer.subarray(0, bytesRead))
      position += bytesRead
    }
    if (Buffer.from(hasher.digest()).toString('hex') !== entry.hash)
      throw new SyncError('storage_failure', 'Mirrored file failed whole-file integrity verification.')
  } finally {
    hasher.destroy()
    await file.close()
  }
}

export async function finishFile(path: string, entry: FileEntry, mode = READ_ONLY_FILE_MODE) {
  await chmod(path, PRIVATE_FILE_MODE)
  const file = await open(path, 'r+')
  try {
    const seconds = entry.modifiedMs / MILLISECONDS_PER_SECOND
    // Node treats negative numbers as the current time; numeric strings preserve pre-epoch values.
    const timestamp = seconds < 0 ? String(seconds) : seconds
    await file.utimes(timestamp, timestamp)
    await file.chmod(mode)
    await file.sync()
  } finally {
    await file.close()
  }
}
