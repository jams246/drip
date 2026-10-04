import { normalizeFileId } from '../storage/files'
import { recordDiagnostic } from '../diagnostics'
import type { DirtyPath } from '../storage/monitoring'
const QUIET_INTERVAL_MS = 5000
const MAX_RETRY_MS = 30000
const FIRST_RETRY_MS = 1000

export interface PendingJob extends DirtyPath {
  kind: 'inventory' | 'hash'
  readyAt: number
  live: boolean
  attempts: number
}

export function containsPath(scope: string, path: string) {
  const root = normalizeFileId(scope)
  const candidate = normalizeFileId(path)
  return candidate === root || candidate.startsWith(root.endsWith('/') ? root : root + '/')
}

export class PendingJobs {
  readonly waiting: PendingJob[] = []
  private readonly paths = new Map<string, PendingJob[]>()
  private readonly ownerGenerations = new Map<string, number>()
  private liveCount = 0
  private generation = 0

  constructor(saved: readonly DirtyPath[]) {
    for (const change of saved) {
      this.generation = Math.max(this.generation, change.generation)
      this.ownerGenerations.set(change.watchId, Math.max(this.ownerGenerations.get(change.watchId) ?? 0, change.generation))
      this.enqueue({ ...change, kind: 'inventory', readyAt: 0, live: true, attempts: 0 })
    }
  }

  private pathKey(watchId: string, path: string) {
    return watchId + '\0' + path
  }

  private enqueue(job: PendingJob) {
    recordDiagnostic('scan.queued', `kind=${job.kind} generation=${job.generation} attempt=${job.attempts}`)
    const key = this.pathKey(job.watchId, job.path)
    const jobs = this.paths.get(key) ?? []
    jobs.push(job)
    this.paths.set(key, jobs)
    this.waiting.push(job)
    if (job.live) this.liveCount++
  }

  private removeAt(index: number) {
    const job = this.waiting.splice(index, 1)[0]
    const key = this.pathKey(job.watchId, job.path)
    const jobs = this.paths.get(key)!
    jobs.splice(jobs.indexOf(job), 1)
    if (jobs.length === 0) this.paths.delete(key)
    if (job.live) this.liveCount--
    return job
  }

  add(watchId: string, path: string, force: boolean, live: boolean, quiet = false, kind: PendingJob['kind'] = 'inventory') {
    const id = normalizeFileId(path)
    const prior = this.paths.get(this.pathKey(watchId, id))?.[0]
    let readyAt = prior?.live ? prior.readyAt : 0
    if (quiet) readyAt = Date.now() + QUIET_INTERVAL_MS
    const change: PendingJob = {
      watchId,
      path: id,
      generation: ++this.generation,
      force: force || Boolean(prior?.force),
      kind,
      live: live || Boolean(prior?.live),
      readyAt,
      attempts: prior?.attempts ?? 0
    }
    if (prior) this.removeAt(this.waiting.indexOf(prior))
    this.ownerGenerations.set(watchId, change.generation)
    this.enqueue(change)
    return change
  }

  take() {
    const now = Date.now()
    let index = -1
    if (this.liveCount > 0) index = this.waiting.findIndex((entry) => entry.live && entry.readyAt <= now)
    if (index < 0) index = this.waiting.findIndex((entry) => entry.readyAt <= now)
    if (index < 0) return undefined
    return this.removeAt(index)
  }

  newer(job: PendingJob) {
    if (job.generation >= (this.ownerGenerations.get(job.watchId) ?? 0)) return false
    return this.waiting.some(
      (entry) =>
        entry.watchId === job.watchId &&
        entry.generation > job.generation &&
        (entry.force || (entry.live && job.kind === 'inventory' && !job.force)) &&
        (containsPath(entry.path, job.path) || (job.kind === 'hash' && containsPath(job.path, entry.path)))
    )
  }

  file(parent: PendingJob, path: string, force: boolean) {
    const id = normalizeFileId(path)
    const prior = this.paths.get(this.pathKey(parent.watchId, id))?.[0]
    if (prior) {
      prior.kind = 'hash'
      prior.force ||= force
      if (parent.live && !prior.live) {
        prior.live = true
        this.liveCount++
      }
      return prior
    }
    const job: PendingJob = { ...parent, path: id, force, kind: 'hash', attempts: 0 }
    this.enqueue(job)
    return job
  }

  forcePath(watchId: string, path: string) {
    return this.waiting.some((entry) => entry.watchId === watchId && entry.force && containsPath(entry.path, path))
  }

  retry(job: PendingJob) {
    if (this.newer(job)) return
    job.attempts++
    job.readyAt = Date.now() + Math.min(MAX_RETRY_MS, FIRST_RETRY_MS * 2 ** (job.attempts - 1))
    this.enqueue(job)
  }

  remove(watchId: string) {
    for (let index = this.waiting.length - 1; index >= 0; index--) {
      if (this.waiting[index].watchId === watchId) this.removeAt(index)
    }
    this.ownerGenerations.delete(watchId)
  }

  has(watchId: string) {
    return this.waiting.some((entry) => entry.watchId === watchId)
  }
}
