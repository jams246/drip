import { type ChunkConsumer, ChunkScanner } from './chunker'
import { consumeFile } from './reader'

export function scanFile(path: string, buffer: Uint8Array, onChunk: ChunkConsumer, onProgress: (bytes: number, chunks: number) => void) {
  const scanner = new ChunkScanner(onChunk)
  try {
    consumeFile(path, buffer, (data, count) => {
      scanner.update(data, count)
      onProgress(scanner.bytes, scanner.chunks)
    })
    scanner.finish()
    onProgress(scanner.bytes, scanner.chunks)
    return { bytes: scanner.bytes, chunks: scanner.chunks }
  } finally {
    scanner.dispose()
  }
}
