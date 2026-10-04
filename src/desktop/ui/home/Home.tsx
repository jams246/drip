import type { ScanLocation, ScanSnapshot, WatchHealth } from '../../scan/types'
import type { SyncStatus } from '../../sync/types'
import { ScanNotices } from '../ScanNotices'
import { FileRow } from './FileRow'
import './home.css'

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
  const counts = { scanning: 0, pending: 0, queued: 0, completed: 0, error: 0 }
  for (const scan of scans) {
    if (scan.state === 'pending' || scan.state === 'scanning' || scan.state === 'queued') counts[scan.state] += 1
    else if (scan.state === 'error' || scan.state === 'completed-with-errors') counts.error += 1
    else counts.completed += 1
  }
  return (
    <section className="home" aria-labelledby="home-title">
      <header className="page-heading">
        <h1 id="home-title" className="page-heading__title">
          Home
        </h1>
        <button className="button button--secondary" type="button" disabled={loading} aria-pressed={paused} onClick={() => onPauseChange(!paused)}>
          <span aria-hidden="true">{paused ? '▷' : 'Ⅱ'}</span>
          {paused ? 'Resume Watching' : 'Pause Watching'}
        </button>
        <button className="button button--secondary" type="button" disabled={loading || paused || verifying || locations.length === 0} onClick={onVerify}>
          {verifying ? 'Verifying contents' : 'Verify all'}
        </button>
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
        {(['scanning', 'pending', 'queued', 'completed', 'error'] as const).map((state) => (
          <div className="home__stat" key={state}>
            <span className="home__stat-value">{counts[state]}</span>
            <span className="home__stat-label">
              {
                {
                  scanning: advanced ? 'Scanning' : 'Preparing for syncronization',
                  pending: 'Preparing',
                  queued: 'Queued',
                  completed: 'Completed',
                  error: 'Needs attention'
                }[state]
              }
            </span>
            <span className={`home__stat-mark home__stat-mark--${state}`} />
          </div>
        ))}
      </div>
      <div className="surface home__files">
        <div className="home__list-heading">
          <h2>File status</h2>
          <span>{locations.length} locations</span>
        </div>
        {locations.length === 0 && !loading && (
          <div className="empty-state">
            <h2>No scans yet</h2>
            <p>Select a file or folder from Watch to start scanning.</p>
          </div>
        )}
        <ul className="home__file-list">
          {locations.map((location) => {
            const scan = scans.find((entry) => entry.id === location.id)
            return (
              scan && (
                <FileRow key={location.id} location={location} scan={scan} advanced={advanced} health={health.find((entry) => entry.id === location.id)} />
              )
            )
          })}
        </ul>
      </div>
    </section>
  )
}
