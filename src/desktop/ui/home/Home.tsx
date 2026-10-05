import { useState } from 'react'
import { isScanActive } from '../../scan/state'
import type { ScanLocation, ScanSnapshot, WatchHealth } from '../../scan/types'
import type { SyncStatus } from '../../sync/types'
import { ScanNotices } from '../ScanNotices'
import { FileRow } from './FileRow'
import './home.css'

const categories = ['scanning', 'queued', 'completed', 'error'] as const
type ScanCategory = (typeof categories)[number]

function getScanCategory(scan: ScanSnapshot): ScanCategory {
  if (isScanActive(scan)) return 'scanning'
  if (scan.state === 'queued') return 'queued'
  if (scan.state === 'error' || scan.state === 'completed-with-errors') return 'error'
  return 'completed'
}

interface HomeProps {
  locations: readonly ScanLocation[]
  scans: readonly ScanSnapshot[]
  sync: SyncStatus
  picking: boolean
  loading?: boolean
  error?: string
  advanced: boolean
  paused: boolean
  verifying: boolean
  health: WatchHealth[]
  onVerify: () => void
  onPauseChange: (paused: boolean) => void
}

export function Home({ locations, scans, sync, picking, loading = false, error, advanced, paused, verifying, health, onVerify, onPauseChange }: HomeProps) {
  const [selectedCategory, setSelectedCategory] = useState<ScanCategory>('completed')
  const counts = { scanning: 0, queued: 0, completed: 0, error: 0 }
  for (const scan of scans) counts[getScanCategory(scan)] += 1
  const rows = locations.flatMap((location) => {
    const scan = scans.find((entry) => entry.id === location.id)
    if (!scan || getScanCategory(scan) !== selectedCategory) return []
    return [{ location, scan }]
  })
  const labels = {
    scanning: advanced ? 'Scanning' : 'Preparing for synchronization',
    queued: 'Queued',
    completed: 'Completed',
    error: 'Needs attention'
  }
  return (
    <section className="home" aria-labelledby="home-title">
      <header className="page-heading">
        <h1 id="home-title" className="page-heading__title">
          Home
        </h1>
        <div className="home__actions">
          <button className="button button--secondary" type="button" disabled={loading} aria-pressed={paused} onClick={() => onPauseChange(!paused)}>
            <span aria-hidden="true">{paused ? '▷' : 'Ⅱ'}</span>
            {paused ? 'Resume watching' : 'Pause watching'}
          </button>
          <button className="button button--secondary" type="button" disabled={loading || paused || verifying || locations.length === 0} onClick={onVerify}>
            {verifying ? 'Verifying contents' : 'Verify all'}
          </button>
        </div>
      </header>
      {error && (
        <p className="home__error" role="alert">
          {error}
        </p>
      )}
      <ScanNotices loading={loading} picking={picking} loadingMessage="Loading saved scan results." className="home__notice" />
      {paused && <output className="home__notice">Watching and hashing are paused.</output>}
      {sync.state !== 'idle' && (
        <output className={`home__sync${sync.state === 'error' ? ' home__sync--error' : ''}`} role={sync.state === 'error' ? 'alert' : 'status'}>
          {sync.message}
        </output>
      )}
      <div className="home__summary" aria-label="Scan summary">
        {categories.map((state) => (
          <button
            className={`home__stat${selectedCategory === state ? ' home__stat--selected' : ''}`}
            key={state}
            type="button"
            aria-pressed={selectedCategory === state}
            aria-controls="home-file-list"
            onClick={() => setSelectedCategory(state)}
          >
            <span className="home__stat-value">{counts[state]}</span>
            <span className="home__stat-label">{labels[state]}</span>
            <span className={`home__stat-mark home__stat-mark--${state}`} aria-hidden="true" />
          </button>
        ))}
      </div>
      <div className="surface home__files">
        <div className="home__list-heading">
          <h2>File status</h2>
          <output>{rows.length} locations</output>
        </div>
        {locations.length === 0 && !loading && (
          <div className="empty-state">
            <h2>No scans yet</h2>
            <p>Select a file or folder from Watch to start scanning.</p>
          </div>
        )}
        {locations.length > 0 && rows.length === 0 && !loading && (
          <div className="empty-state">
            <h2>No matching locations</h2>
            <p>No files or folders have this status.</p>
          </div>
        )}
        <ul className="home__file-list" id="home-file-list">
          {rows.map(({ location, scan }) => (
            <FileRow key={location.id} location={location} scan={scan} advanced={advanced} health={health.find((entry) => entry.id === location.id)} />
          ))}
        </ul>
      </div>
    </section>
  )
}
