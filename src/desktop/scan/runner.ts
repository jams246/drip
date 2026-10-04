import { Worker } from 'node:worker_threads'
import { normalizeFileId } from '../storage/files'
import type { WatchStore } from '../storage/locations'
import type { createStorageWrites } from '../storage/retry'
import type { createMonitoring } from './monitoring'
import { type PendingJob, PendingJobs } from './pending'
import { createScanProgress } from './progress'
import type { ScanEvent, ScanLocation } from './types'
import type { ScanJobCommand, ScanJobResponse } from './worker-types'

interface RunnerContext {
  databasePath: string
  locations: ScanLocation[]
  pending: PendingJobs
  store: WatchStore
  writes: ReturnType<typeof createStorageWrites>
  monitoring: ReturnType<typeof createMonitoring>
  publish: (event: ScanEvent) => void
  canRun: () => boolean
  settled: (job: PendingJob) => void
}

// oxlint-disable-next-line eslint/max-statements -- One worker owns the job protocol callbacks and cancellation state.
export function createJobRunner(context: RunnerContext) {
  const worker = new Worker('../../../.perry/generated/scan-worker.ts')
  const progress = createScanProgress()
  const callbacks: (() => void)[] = []
  let nextJobId = 0
  let active: { job: PendingJob; item: ScanLocation; jobId: number; cancelling: boolean; started: boolean } | undefined
  let workerError = ''
  let stopped = false
  let idleCancellation = false

  function failure(error: unknown) {
    context.publish({ type: 'error', message: String(error) })
  }
  function post(message: ScanJobCommand) {
    // oxlint-disable-next-line unicorn/require-post-message-target-origin -- Node worker uses commands, not browser target origins.
    worker.postMessage(message)
  }
  function send(type: 'step' | 'commit' | 'cancel') {
    if (active) post({ type, jobId: active.jobId })
  }
  function notifyCancelled() {
    while (callbacks.length > 0) callbacks.shift()!()
  }
  function workerFailed(error: unknown) {
    if (stopped || workerError) return
    workerError = String(error)
    idleCancellation = false
    if (active) {
      context.pending.retry(active.job)
      active = undefined
    }
    failure(workerError)
    notifyCancelled()
  }
  function closeIdle() {
    if (workerError || stopped) return notifyCancelled()
    if (idleCancellation) return
    idleCancellation = true
    post({ type: 'cancel', jobId: 0 })
  }

  function startNext() {
    if (active || idleCancellation || !context.canRun() || workerError) return
    const job = context.pending.take()
    if (!job) return
    const item = context.locations.find((entry) => entry.id === job.watchId)
    if (!item) return startNext()
    const current = { job, item, jobId: ++nextJobId, cancelling: false, started: false }
    active = current
    context.publish({ type: 'progress', scan: progress.begin(item, current.jobId, job.path) })
    context.writes.enqueue(
      () => {
        context.store.startScan(item.id)
        context.store.monitoring.dirty(job)
      },
      () => {
        if (active !== current) return
        if (current.cancelling || !context.canRun()) {
          context.pending.retry(job)
          active = undefined
          closeIdle()
          return
        }
        current.started = true
        post({
          type: 'start',
          jobId: current.jobId,
          item,
          databasePath: context.databasePath,
          target: job.path,
          kind: job.kind,
          force: job.force,
          generation: job.generation
        })
      },
      (error) => {
        context.pending.retry(job)
        active = undefined
        failure(error)
        if (current.cancelling) closeIdle()
        else notifyCancelled()
      }
    )
  }

  function cancel(done?: () => void) {
    if (done) callbacks.push(done)
    if (!active) {
      closeIdle()
      return
    }
    if (active.cancelling) return
    active.cancelling = true
    if (active.started) send('cancel')
  }

  function complete(result: ScanJobResponse, retry: boolean) {
    const current = active!
    if (retry && context.locations.some((item) => item.id === current.item.id)) context.pending.retry(current.job)
    const waiting = context.pending.has(current.item.id)
    const files = current.job.kind === 'inventory' && result.type === 'done' ? result.files : 0
    const scan = progress.finish(current.item.id, files, result.skipped, result.errors, result.error, waiting)
    active = undefined
    context.writes.enqueue(
      () => {
        if (!retry) context.store.monitoring.clear(current.job)
        if (!waiting) context.store.finishScan(scan)
      },
      () => {
        context.publish({ type: 'progress', scan })
        if (!retry) context.settled(current.job)
      },
      failure
    )
    if (current.cancelling && result.type !== 'cancelled') closeIdle()
    else notifyCancelled()
    startNext()
  }

  function handleJobResult(result: ScanJobResponse) {
    if (!active || result.jobId !== active.jobId) return
    if (result.type === 'cancelled' || result.type === 'error') return complete(result, true)
    if (result.type === 'done') {
      if (result.bytes) progress.update(active.item.id, result.bytes, result.chunks, result.size)
      return complete(result, Boolean(result.errors || (result.missing && active.job.path === normalizeFileId(active.item.path))))
    }
    if (active.cancelling) return
    for (const entry of result.entries) {
      if (entry.needsHash || active.job.force || context.pending.forcePath(active.item.id, entry.path)) {
        context.pending.file(active.job, entry.path, active.job.force || context.pending.forcePath(active.item.id, entry.path))
      }
    }
    if (result.type === 'progress') context.publish({ type: 'progress', scan: progress.update(active.item.id, result.bytes, result.chunks, result.size) })
    if (result.type === 'ready') {
      context.monitoring.drain()
      if (!context.canRun() || context.pending.newer(active.job)) return cancel()
      send('commit')
      return
    }
    send('step')
  }
  worker.on('message', (result: ScanJobResponse) => {
    if (stopped || workerError) return
    if (idleCancellation && result.jobId === 0 && (result.type === 'cancelled' || result.type === 'error')) {
      idleCancellation = false
      if (result.type === 'error') return workerFailed(result.error)
      notifyCancelled()
      startNext()
      return
    }
    handleJobResult(result)
  })
  worker.on('error', workerFailed)
  worker.on('exit', (code: number) => workerFailed(`Scan worker exited with code ${code}.`))
  return {
    startNext,
    cancel,
    activeId: () => active?.item.id,
    unavailable: () => workerError,
    stop: () => {
      stopped = true
      void worker.terminate()
    }
  }
}
