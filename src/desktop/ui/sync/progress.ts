import type { ProgressUpdate, SyncFile } from './types'

export function applyProgressUpdates(files: readonly SyncFile[], updates: readonly ProgressUpdate[]): SyncFile[] {
  const updatesById = new Map<string, Omit<ProgressUpdate, 'fileId'>>()
  for (const { fileId, ...update } of updates) {
    updatesById.set(fileId, { ...updatesById.get(fileId), ...update })
  }
  return files.map((file) => {
    const update = updatesById.get(file.id)
    if (!update) return file
    const next = { ...file, ...update }
    if (next.value === file.value && next.max === file.max && next.stage === file.stage && next.status === file.status) return file
    return next
  })
}
