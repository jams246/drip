import { ContentHasher } from './hash'
import { type RegionConsumer, RegionScanner } from './regions'
import { consumeFile } from './reader'

export function scanFile(path: string, buffer: Uint8Array, onRegion: RegionConsumer, onProgress: (bytes: number) => void) {
  const scanner = new RegionScanner(onRegion)
  const content = new ContentHasher()
  try {
    consumeFile(path, buffer, (data, count) => {
      scanner.update(data, count)
      content.updateRange(data, 0, count)
      onProgress(scanner.bytes)
    })
    scanner.finish()
    onProgress(scanner.bytes)
    return { bytes: scanner.bytes, hash: content.hex() }
  } finally {
    scanner.dispose()
    content.destroy()
  }
}
