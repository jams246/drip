/* oxlint-disable eslint/no-magic-numbers -- BLAKE3 byte/word layout and digest encoding. */
/* oxlint-disable typescript/no-unnecessary-type-conversion -- Number construction removes measured Perry hot-loop boxing. */
import { _BLAKE3 } from '@noble/hashes/blake3.js'

// Pinned to noble 2.4.0: only ingestion differs from its protected base implementation.
// Perry currently copies input typed-array views; keep pending bytes in fixed hash state.
export class ChunkHasher extends _BLAKE3 {
  constructor() {
    super()
    // Independent pending words avoid Perry's copy-and-synchronize byte/word aliases.
    this.buffer32 = new Uint32Array(16)
  }

  // oxlint-disable-next-line eslint/max-statements -- Local compression state avoids Perry field dispatch for each input word.
  updateRange(data: Uint8Array, start: number, count: number): void {
    if (this.finished || this.destroyed) throw new Error('Hash is already finalized')
    if (start < 0 || count < 0 || start + count > data.length) throw new RangeError('Invalid hash range')
    // Numeric construction keeps scalar locals unboxed in Perry's compiled loop.
    const end = Number(start) + Number(count)
    let index = Number(start)
    let position = Number(this.pos)
    const words: Uint32Array = this.buffer32
    while (index < end) {
      if (position === 64) {
        this.pos = position
        this.compress(words, 0, false)
        position = 0
      }
      const word = position >>> 2
      const shift = (position & 3) * 8
      if (shift === 0 && end - index >= 4) {
        words[word] = Number(data[index]) | (Number(data[index + 1]) << 8) | (Number(data[index + 2]) << 16) | (Number(data[index + 3]) << 24)
        position += 4
        index += 4
        continue
      }
      words[word] = (words[word] & ~(255 << shift)) | (Number(data[index]) << shift)
      position++
      index++
    }
    this.pos = position
    this.length += count
  }

  hex(): string {
    // Upstream pads its byte buffer. Our independent pending words need identical padding.
    const partial = this.pos & 3
    const first = Math.ceil(this.pos / 4)
    if (partial > 0) this.buffer32[this.pos >>> 2] &= 0xffffffff >>> ((4 - partial) * 8)
    this.buffer32.fill(0, first)
    const digest = new Uint8Array(32)
    this.digestInto(digest)
    let result = ''
    const digits = '0123456789abcdef'
    for (let index = 0; index < digest.length; index++) {
      result += digits[digest[index] >>> 4] + digits[digest[index] & 15]
    }
    return result
  }
}
