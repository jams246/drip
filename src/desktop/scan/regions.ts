/* oxlint-disable eslint/no-magic-numbers -- Pinned FastCDC masks and unsigned 64-bit arithmetic. */
/* oxlint-disable typescript/no-unnecessary-type-conversion -- Number construction removes measured Perry hot-loop boxing. */
import { gearHigh, gearLow } from './gear'
import { ContentHasher } from './hash'

export type RegionConsumer = (offset: number, length: number, hash: string) => void
export const MIN_REGION = 65_536
export const AVG_REGION = 262_144
export const MAX_REGION = 1_048_576

export class RegionScanner {
  bytes = 0
  regions = 0
  private length = 0
  private offset = 0
  private low = 0
  private high = 0
  private hasher: ContentHasher | undefined

  constructor(
    private readonly consume: RegionConsumer,
    private readonly hash = true
  ) {
    if (hash) this.hasher = new ContentHasher()
  }

  // oxlint-disable-next-line eslint/max-statements -- One streaming loop avoids per-byte Perry method dispatch; benchmarked before integration.
  update(buffer: Uint8Array, count: number): void {
    // Numeric construction keeps scalar locals unboxed in Perry's compiled loop.
    const validCount = Number(count)
    let start = 0
    let index = 0
    let length = Number(this.length)
    let low = Number(this.low)
    let high = Number(this.high)
    while (index < validCount) {
      if (length < MIN_REGION) {
        const skip = Math.min(MIN_REGION - length, validCount - index)
        length += skip
        index += skip
        continue
      }
      const byte = Number(buffer[index])
      const sum = ((low << 1) >>> 0) + Number(gearLow[byte])
      const carry = sum >= 4_294_967_296 ? 1 : 0
      high = ((high << 1) + (low >>> 31) + Number(gearHigh[byte]) + carry) >>> 0
      low = sum >>> 0
      const beforeAverage = length < AVG_REGION
      const highMask = beforeAverage ? 0x0000d917 : 0x0000d903
      const lowMask = beforeAverage ? 0x47537000 : 0x03537000
      if ((high & highMask) === 0 && (low & lowMask) === 0) {
        // Reference cut excludes the candidate byte; it belongs to the next region.
        this.hasher?.updateRange(buffer, start, index - start)
        this.length = length
        this.complete()
        start = index
        length = 0
        low = 0
        high = 0
        continue
      }
      length++
      index++
      if (length === MAX_REGION) {
        this.hasher?.updateRange(buffer, start, index - start)
        this.length = length
        this.complete()
        start = index
        length = 0
        low = 0
        high = 0
      }
    }
    this.length = length
    this.low = low
    this.high = high
    this.hasher?.updateRange(buffer, start, validCount - start)
    this.bytes += validCount
  }

  finish(): void {
    if (this.length > 0) this.complete()
    this.dispose()
  }

  dispose(): void {
    this.hasher?.destroy()
    this.hasher = undefined
  }

  private complete(): void {
    const digest = this.hasher ? this.hasher.hex() : ''
    this.consume(this.offset, this.length, digest)
    this.offset += this.length
    this.regions++
    this.length = 0
    this.low = 0
    this.high = 0
    if (this.hash) this.hasher = new ContentHasher()
  }
}
