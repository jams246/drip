import { FileRow } from './FileRow'
import type { Connection, SyncFile } from '../sync/types'
import './home.css'

interface HomeProps {
  files: readonly SyncFile[]
  paused: boolean
  connection: Connection
  onPauseChange: (paused: boolean) => void
  onViewActivity: (id: string) => void
}

export function Home({ files, paused, connection, onPauseChange, onViewActivity }: HomeProps) {
  const counts = { syncing: 0, preparing: 0, synced: 0, error: 0 }
  for (const file of files) counts[file.status] += 1
  const blocked = connection.status !== 'connected'
  return (
    <section className="home" aria-labelledby="home-title">
      <header className="page-heading">
        <h1 id="home-title" className="page-heading__title">
          Home
        </h1>
        <button className="button button--secondary" type="button" disabled={blocked || files.length === 0} onClick={() => onPauseChange(!paused)}>
          <span aria-hidden="true">{paused ? '▷' : 'Ⅱ'}</span>
          {paused ? 'Resume syncing' : 'Pause syncing'}
        </button>
      </header>
      <div className="home__summary" aria-label="Sync summary">
        <div className="home__stat">
          <span className="home__stat-value">{counts.syncing}</span>
          <span className="home__stat-label">Syncing</span>
          <span className="home__stat-mark home__stat-mark--syncing" />
        </div>
        <div className="home__stat">
          <span className="home__stat-value">{counts.preparing}</span>
          <span className="home__stat-label">Preparing</span>
          <span className="home__stat-mark home__stat-mark--preparing" />
        </div>
        <div className="home__stat">
          <span className="home__stat-value">{counts.synced}</span>
          <span className="home__stat-label">Synced</span>
          <span className="home__stat-mark home__stat-mark--synced" />
        </div>
        <div className="home__stat">
          <span className="home__stat-value">{counts.error}</span>
          <span className="home__stat-label">Needs attention</span>
          <span className="home__stat-mark home__stat-mark--error" />
        </div>
      </div>
      <div className="surface home__files">
        <div className="home__list-heading">
          <h2>File status</h2>
          <span>{files.length} items</span>
        </div>
        {files.length === 0 && (
          <div className="empty-state">
            <h2>No watched files</h2>
            <p>There are no files or folders in your watch list.</p>
          </div>
        )}
        <ul className="home__file-list">
          {files.map((file) => (
            <FileRow key={file.id} file={file} paused={paused} blocked={blocked} onViewActivity={onViewActivity} />
          ))}
        </ul>
      </div>
    </section>
  )
}
