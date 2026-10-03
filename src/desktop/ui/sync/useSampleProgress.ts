import { type Dispatch, type SetStateAction, useEffect } from 'react'
import { applyProgressUpdates } from './progress'
import type { SyncFile } from './types'

const updateInterval = 100
const sampleStep = 1

export function useSampleProgress(paused: boolean, setFiles: Dispatch<SetStateAction<SyncFile[]>>) {
  useEffect(() => {
    if (paused) return undefined
    const timer = setInterval(() => {
      setFiles((files) =>
        applyProgressUpdates(
          files,
          files
            .filter((file) => file.status === 'syncing' && file.value !== undefined)
            .map((file) => {
              const next = (file.value ?? 0) + sampleStep
              return { fileId: file.id, value: next > file.max ? 0 : next }
            })
        )
      )
    }, updateInterval)
    return () => clearInterval(timer)
  }, [paused, setFiles])
}
