import { queryNames, queryPath } from '#drip-window-icon'
import { basename } from 'node:path'
import { normalizeFileId } from '../storage/files'
import { WatchStore } from '../storage/locations'
import { createStorageWrites } from '../storage/retry'
import { isApplicationDataPath } from './eligibility'
import { createMonitoring } from './monitoring'
import { type PendingJob, PendingJobs } from './pending'
import { createJobRunner } from './runner'
import type { ScanEvent, ScanLocation } from './types'

const HOST_INTERVAL_MS = 100
const DRIVE_FIXED = 3
const FIRST_RETRY_MS = 1000
const MAX_RETRY_MS = 30000

// oxlint-disable-next-line eslint/max-statements -- One application session owns these lifecycle callbacks and shared state.
export function createScanService(databasePath: string, publish: (event: ScanEvent) => void) {
  const store = new WatchStore(databasePath)
  const writes = createStorageWrites()
  const saved = store.hydrate()
  const state = store.monitoring.restore()
  const locations = saved.locations
  const pending = new PendingJobs(state.pending)
  const verification = new Map(state.verification.map((id) => [id, 0]))
  let paused = state.paused
  let stopping = false
  let stopped = false
  let pausing = false
  let resumeRequested = false
  let lastHealth = ''

  function failure(error: unknown) {
    publish({ type: 'error', message: String(error) })
  }
  function write(operation: () => void, complete = () => {}, failed = failure) {
    writes.enqueue(operation, complete, failed)
  }
  function retryWrite(error: unknown, retry: () => void, attempts: number) {
    failure(error)
    setTimeout(retry, Math.min(MAX_RETRY_MS, FIRST_RETRY_MS * 2 ** attempts))
  }
  function monitoringState() {
    const event: ScanEvent = { type: 'monitoring', paused, verifying: verification.size > 0, health: monitoring.health() }
    const serialized = JSON.stringify(event)
    if (serialized !== lastHealth) {
      lastHealth = serialized
      publish(event)
    }
  }
  function verify(id: string) {
    const item = locations.find((entry) => entry.id === id)
    if (!item) return
    const job = pending.add(id, item.path, true, false)
    verification.set(id, job.generation)
    write(() => store.monitoring.verify(id))
  }
  const monitoring = createMonitoring(
    (id, path, force) => pending.add(id, path, force, true, true),
    verify,
    (path) => isApplicationDataPath(path, databasePath)
  )
  function settled(job: PendingJob, attempts = 0) {
    if (stopping || pending.has(job.watchId) || runner.activeId() === job.watchId || !verification.has(job.watchId)) return
    const generation = verification.get(job.watchId)!
    const coverage = monitoring.coverage(job.watchId)
    write(
      () => store.monitoring.verified(job.watchId, generation),
      () => {
        if (verification.get(job.watchId) === generation) verification.delete(job.watchId)
        monitoring.acknowledge(job.watchId, coverage)
        monitoringState()
      },
      (error) => retryWrite(error, () => settled(job, attempts + 1), attempts)
    )
  }
  const runner = createJobRunner({ databasePath, locations, pending, store, writes, monitoring, publish, canRun: () => !paused && !stopping, settled })

  function checkpoint(complete = () => {}, attempts = 0) {
    if (stopped) return complete()
    write(
      () => {
        store.monitoring.save(pending.waiting.map((job) => ({ ...job, force: job.force || job.kind === 'hash' })))
        for (const id of verification.keys()) store.monitoring.verify(id)
        store.monitoring.pause(paused)
        store.monitoring.session(false)
      },
      () => {
        monitoring.checkpoint()
        complete()
      },
      (error) => retryWrite(error, () => checkpoint(complete, attempts + 1), attempts)
    )
  }
  function resume() {
    paused = false
    write(
      () => {
        store.monitoring.pause(false)
        store.monitoring.session(true)
      },
      () => {
        if (paused || stopping) return
        monitoring.start()
        for (const item of locations) {
          const job = pending.add(item.id, item.path, verification.has(item.id), false)
          if (verification.has(item.id)) verification.set(item.id, job.generation)
        }
        monitoringState()
        runner.startNext()
      }
    )
  }
  function pause(next: boolean) {
    if (pausing) {
      resumeRequested = !next
      return
    }
    if (stopping || paused === next) return
    if (!next) return resume()
    paused = true
    pausing = true
    write(() => store.monitoring.pause(true))
    monitoring.stop()
    runner.cancel(() => {
      checkpoint(() => {
        pausing = false
        if (resumeRequested && !stopping) {
          resumeRequested = false
          resume()
        }
      })
      monitoringState()
    })
  }
  function select(item: ScanLocation) {
    const names: { longPath: string } = JSON.parse(queryNames(item.path))
    if (names.longPath) item = { id: normalizeFileId(names.longPath), name: basename(names.longPath) || names.longPath, path: names.longPath, kind: item.kind }
    if (locations.some((entry) => entry.id === item.id && entry.kind !== item.kind)) {
      failure('Stop watching this location before selecting its new file or folder type.')
      publish({ type: 'selection-ended' })
      return
    }
    const metadata: { driveType: number; error: number } = JSON.parse(queryPath(item.path))
    if (metadata.driveType !== DRIVE_FIXED || metadata.error) {
      failure('Select an accessible file or folder on a local fixed drive.')
      publish({ type: 'selection-ended' })
      return
    }
    write(
      () => store.saveLocation(item),
      () => {
        if (!locations.some((entry) => entry.id === item.id)) {
          locations.push(item)
          monitoring.add(item)
        }
        verify(item.id)
        publish({ type: 'selected', item })
        monitoringState()
        runner.startNext()
      }
    )
  }
  function remove(id: string) {
    const index = locations.findIndex((item) => item.id === id)
    if (index < 0) return
    monitoring.remove(id)
    locations.splice(index, 1)
    pending.remove(id)
    verification.delete(id)
    function erase() {
      write(
        () => store.removeLocation(id),
        () => {
          monitoring.checkpoint()
          publish({ type: 'removed', id })
          monitoringState()
          runner.startNext()
        }
      )
    }
    if (runner.activeId() === id) runner.cancel(erase)
    else erase()
  }
  function stop(done: () => void = () => {}) {
    if (stopped) return done()
    if (stopping) return
    stopping = true
    clearInterval(timer)
    monitoring.stop()
    runner.cancel(() =>
      checkpoint(() => {
        stopped = true
        writes.stop()
        void (async () => {
          await runner.stop()
          store.close()
          done()
        })()
      })
    )
  }
  function verifyAll() {
    if (paused || verification.size > 0) return
    for (const item of locations) verify(item.id)
    monitoringState()
    runner.startNext()
  }
  function rescan(id: string, path: string) {
    if (!locations.some((item) => item.id === id)) return
    pending.add(id, path, true, false)
    runner.startNext()
  }
  publish({ type: 'hydrated', locations: locations.slice(), scans: saved.scans })
  for (const item of locations) monitoring.add(item)
  if (state.unclean) for (const item of locations) verify(item.id)
  const timer = setInterval(() => {
    if (stopping) return
    monitoring.tick()
    monitoringState()
    runner.startNext()
  }, HOST_INTERVAL_MS)
  if (!paused) resume()
  else monitoringState()
  return {
    select,
    remove,
    pause,
    stop,
    verifyAll,
    rescan,
    busy: (id: string) => pending.has(id) || verification.has(id) || runner.activeId() === id,
    unavailable: runner.unavailable,
    metrics: runner.metrics,
    isPaused: () => paused
  }
}
