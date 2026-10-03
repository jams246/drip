import type { ScanLocation, ScanSnapshot } from '../../scan/types'
import type { Connection } from '../sync/types'
import { FileRow } from './FileRow'
import './home.css'

interface HomeProps {
  locations: readonly ScanLocation[]
  scans: readonly ScanSnapshot[]
  picking: boolean
  error?: string
  advanced: boolean
  paused: boolean
  connection: Connection
  onPauseChange: (paused: boolean) => void
}

export function Home({ locations, scans, picking, error, advanced, paused, connection, onPauseChange }: HomeProps) {
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
        <button
          className="button button--secondary"
          type="button"
          disabled={connection.status !== 'connected' || locations.length === 0}
          aria-pressed={paused}
          onClick={() => onPauseChange(!paused)}
        >
          <span aria-hidden="true">{paused ? '▷' : 'Ⅱ'}</span>
          {paused ? 'Resume Syncing' : 'Pause Syncing'}
        </button>
      </header>
      {error && (
        <p className="home__error" role="alert">
          {error}
        </p>
      )}
      {picking && <output className="home__notice">Choose a file or folder in the selection dialog.</output>}
      {paused && <output className="home__notice">Syncing is paused. Local file processing continues.</output>}
      <div className="home__summary" aria-label="Scan summary">
        {(['scanning', 'pending', 'queued', 'completed', 'error'] as const).map((state) => (
          <div className="home__stat" key={state}>
            <span className="home__stat-value">{counts[state]}</span>
            <span className="home__stat-label">
              {
                { scanning: advanced ? 'Scanning' : 'Processing', pending: 'Preparing', queued: 'Queued', completed: 'Completed', error: 'Needs attention' }[
                  state
                ]
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
        {locations.length === 0 && (
          <div className="empty-state">
            <h2>No scans yet</h2>
            <p>Select a file or folder from Watch to start scanning.</p>
          </div>
        )}
        <ul className="home__file-list">
          {locations.map((location) => {
            const scan = scans.find((entry) => entry.id === location.id)
            return scan && <FileRow key={location.id} location={location} scan={scan} advanced={advanced} />
          })}
        </ul>
      </div>
    </section>
  )
}
