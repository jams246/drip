const kibibyte = 1024
const mebibyte = kibibyte * kibibyte
const gibibyte = mebibyte * kibibyte

export function formatBytes(bytes: number) {
  if (bytes < kibibyte) return `${bytes} B`
  if (bytes < mebibyte) return `${(bytes / kibibyte).toFixed(1)} KiB`
  if (bytes < gibibyte) return `${(bytes / mebibyte).toFixed(1)} MiB`
  return `${(bytes / gibibyte).toFixed(2)} GiB`
}
