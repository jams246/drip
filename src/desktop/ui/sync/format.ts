const kibibyte = 1024
const mebibyte = kibibyte * kibibyte
const gibibyte = mebibyte * kibibyte

export function formatBytes(bytes: number) {
  if (bytes < kibibyte) return `${bytes} B`
  if (bytes < mebibyte) return `${(bytes / kibibyte).toFixed(1)} KiB`
  if (bytes < gibibyte) return `${(bytes / mebibyte).toFixed(1)} MiB`
  return `${(bytes / gibibyte).toFixed(2)} GiB`
}

export function formatSpeed(bytesPerSecond: number) {
  if (bytesPerSecond < kibibyte) return `${Math.floor(bytesPerSecond)} B/s`
  if (bytesPerSecond < mebibyte) return `${Math.floor(bytesPerSecond / kibibyte)} KiB/s`
  if (bytesPerSecond < gibibyte) return `${Math.floor(bytesPerSecond / mebibyte)} MiB/s`
  return `${Math.floor(bytesPerSecond / gibibyte)} GiB/s`
}
