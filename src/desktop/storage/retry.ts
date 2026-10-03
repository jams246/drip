const SQLITE_BUSY = 5
const SQLITE_LOCKED = 6
const SQLITE_CODE_MASK = 255
const RETRY_INTERVAL_MS = 100
const WRITE_TIMEOUT_MS = 5000

function isDatabaseBusy(error: unknown): boolean {
  if (!error || typeof error !== 'object' || !('errcode' in error) || typeof error.errcode !== 'number') return false
  const code = error.errcode & SQLITE_CODE_MASK
  return code === SQLITE_BUSY || code === SQLITE_LOCKED
}

export function createStorageWrites() {
  const waiting: { write: () => void; saved: () => void; failed: (error: unknown) => void; started: number }[] = []
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  let running = false

  function flush() {
    timer = undefined
    if (stopped || running) return
    running = true
    try {
      while (waiting.length > 0) {
        const pending = waiting[0]
        if (pending.started === 0) pending.started = Date.now()
        try {
          pending.write()
        } catch (error) {
          if (isDatabaseBusy(error) && Date.now() - pending.started < WRITE_TIMEOUT_MS) {
            timer = setTimeout(flush, RETRY_INTERVAL_MS)
            return
          }
          waiting.shift()
          pending.failed(error)
          continue
        }
        waiting.shift()
        pending.saved()
      }
    } finally {
      running = false
    }
  }

  function enqueue(write: () => void, saved: () => void, failed: (error: unknown) => void) {
    if (stopped) return
    waiting.push({ write, saved, failed, started: 0 })
    if (timer === undefined) flush()
  }

  function stop() {
    stopped = true
    if (timer !== undefined) clearTimeout(timer)
    waiting.length = 0
  }

  return { enqueue, stop }
}
