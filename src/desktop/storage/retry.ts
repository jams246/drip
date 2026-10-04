const SQLITE_BUSY = 5
const SQLITE_LOCKED = 6
const SQLITE_CODE_MASK = 255
const FIRST_RETRY_MS = 1000
const MAX_RETRY_MS = 30000
const DRAIN_BUDGET_MS = 8

function isDatabaseBusy(error: unknown): boolean {
  if (!error || typeof error !== 'object' || !('errcode' in error) || typeof error.errcode !== 'number') return false
  const code = error.errcode & SQLITE_CODE_MASK
  return code === SQLITE_BUSY || code === SQLITE_LOCKED
}

export function createStorageWrites() {
  const waiting: { write: () => void; saved: () => void; failed: (error: unknown) => void; attempts: number }[] = []
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  let running = false

  function flush() {
    timer = undefined
    if (stopped || running) return
    running = true
    const started = Date.now()
    try {
      while (waiting.length > 0 && Date.now() - started < DRAIN_BUDGET_MS) {
        const pending = waiting[0]
        try {
          pending.write()
        } catch (error) {
          if (isDatabaseBusy(error)) {
            timer = setTimeout(flush, Math.min(MAX_RETRY_MS, FIRST_RETRY_MS * 2 ** pending.attempts++))
            return
          }
          waiting.shift()
          pending.failed(error)
          continue
        }
        waiting.shift()
        pending.saved()
      }
      if (waiting.length > 0) timer = setTimeout(flush, 0)
    } finally {
      running = false
    }
  }

  function enqueue(write: () => void, saved: () => void, failed: (error: unknown) => void) {
    if (stopped) return
    waiting.push({ write, saved, failed, attempts: 0 })
    if (timer === undefined && !running) timer = setTimeout(flush, 0)
  }

  function stop() {
    stopped = true
    if (timer !== undefined) clearTimeout(timer)
    waiting.length = 0
  }

  return { enqueue, stop }
}
