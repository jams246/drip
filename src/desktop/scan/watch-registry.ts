import { watchAck, watchDrain, watchShutdown, watchStart, watchStop } from '#drip-window-icon'
import { normalizeFileId } from '../storage/files'
import { containsPath } from './pending'

const ERROR_NOTIFY_ENUM_DIR = 1022

export interface NativeRecord {
  id: number
  sequence: number
  action: number
  path: string
  error: number
  loss: boolean
}
export interface Registration {
  id: number
  path: string
  recursive: boolean
  owners: string[]
  loss: number
  covered: string[]
  retired: boolean
}
export interface WatchCoverage {
  id: number
  sequence: number
}

export function createWatchRegistry(
  changed: (registration: Registration, record: NativeRecord) => void,
  lost: (id: string) => void,
  failed: (owners: readonly string[], error: number) => void
) {
  const registrations: Registration[] = []
  let draining = false
  let drainAgain = false

  function attach(owner: string, path: string, recursive: boolean) {
    const normalized = normalizeFileId(path)
    const prior = registrations.find((entry) => !entry.retired && entry.recursive === recursive && normalizeFileId(entry.path) === normalized)
    if (prior) {
      if (!prior.owners.includes(owner)) {
        prior.owners.push(owner)
        if (prior.loss) lost(owner)
      }
      return
    }
    const id = watchStart(path, recursive)
    if (id <= 0) throw new Error(`Windows watcher error ${-id}: ${path}`)
    registrations.push({ id, path, recursive, owners: [owner], loss: 0, covered: [], retired: false })
  }

  function retire(registration: Registration) {
    if (registration.retired) return
    registration.retired = true
    watchStop(registration.id)
    drainAgain = true
  }

  function drain() {
    if (draining) {
      drainAgain = true
      return
    }
    draining = true
    try {
      do {
        drainAgain = false
        const records: NativeRecord[] = JSON.parse(watchDrain())
        for (const record of records) consume(record)
      } while (drainAgain)
    } finally {
      draining = false
    }
    for (let index = registrations.length - 1; index >= 0; index--) {
      if (registrations[index].retired && !registrations[index].loss) registrations.splice(index, 1)
    }
  }

  function consume(record: NativeRecord) {
    const registration = registrations.find((entry) => entry.id === record.id)
    if (!registration) return
    if ((record.loss || record.error) && record.sequence > registration.loss) {
      registration.loss = record.sequence
      registration.covered = []
      for (const owner of registration.owners) lost(owner)
    }
    if (record.error && record.error !== ERROR_NOTIFY_ENUM_DIR && !registration.retired) {
      retire(registration)
      failed(registration.owners, record.error)
    }
    if (record.path) changed(registration, record)
  }

  function detach(owner: string, affectedPath?: string) {
    drain()
    for (const registration of registrations) {
      if (registration.retired || !registration.owners.includes(owner) || (affectedPath && !containsPath(affectedPath, registration.path))) continue
      if (registration.owners.length === 1) retire(registration)
      else registration.owners = registration.owners.filter((entry) => entry !== owner)
    }
    drain()
  }

  function stop() {
    for (const registration of registrations) retire(registration)
    drain()
    watchShutdown()
  }

  function coverage(owner: string): WatchCoverage[] {
    return registrations.filter((entry) => entry.owners.includes(owner) && entry.loss > 0).map((entry) => ({ id: entry.id, sequence: entry.loss }))
  }

  function acknowledge(owner: string, covered: readonly WatchCoverage[]) {
    for (const snapshot of covered) {
      const registration = registrations.find((entry) => entry.id === snapshot.id)
      if (!registration || registration.loss !== snapshot.sequence || !registration.owners.includes(owner)) continue
      if (!registration.covered.includes(owner)) registration.covered.push(owner)
    }
    release(false)
  }

  function release(checkpoint: boolean) {
    for (let index = registrations.length - 1; index >= 0; index--) {
      const registration = registrations[index]
      const covered = registration.owners.every((owner) => registration.covered.includes(owner))
      if (!(checkpoint && registration.retired) && !covered) continue
      if (registration.loss) watchAck(registration.id, registration.loss)
      registration.loss = 0
      registration.covered = []
      if (registration.retired) registrations.splice(index, 1)
    }
  }

  return { attach, detach, drain, stop, coverage, acknowledge, checkpoint: () => release(true) }
}
