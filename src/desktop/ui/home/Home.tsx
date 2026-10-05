import { useState } from 'react'
import { isScanActive } from '../../scan/state'
import type { ScanLocation, ScanSnapshot, WatchHealth } from '../../scan/types'
import type { SyncStatus } from '../../sync/types'
import { ScanNotices } from '../ScanNotices'
import { FileRow } from './FileRow'
import { Alert, AlertDescription } from '../components/alert'
import { Button } from '../components/button'
import { Card } from '../components/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '../components/empty'
import { ToggleGroup, ToggleGroupItem } from '../components/toggle-group'
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
          <Button variant="outline" type="button" disabled={loading} aria-pressed={paused} onClick={() => onPauseChange(!paused)}>
            <span aria-hidden="true">{paused ? '▷' : 'Ⅱ'}</span>
            {paused ? 'Resume watching' : 'Pause watching'}
          </Button>
          <Button variant="outline" type="button" disabled={loading || paused || verifying || locations.length === 0} onClick={onVerify}>
            {verifying ? 'Verifying contents' : 'Verify all'}
          </Button>
        </div>
      </header>
      {error && (
        <Alert className="home__error" variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <ScanNotices loading={loading} picking={picking} loadingMessage="Loading saved scan results." className="home__notice" />
      {paused && (
        <Alert asChild className="home__notice">
          <output>
            <AlertDescription>Watching and hashing are paused.</AlertDescription>
          </output>
        </Alert>
      )}
      {sync.state !== 'idle' && (
        <Alert className="home__sync" variant={sync.state === 'error' ? 'destructive' : 'default'} role={sync.state === 'error' ? 'alert' : 'status'}>
          <AlertDescription>{sync.message}</AlertDescription>
        </Alert>
      )}
      <ToggleGroup
        type="single"
        value={selectedCategory}
        onValueChange={(next) => {
          const category = categories.find((value) => value === next)
          if (category) setSelectedCategory(category)
        }}
        className="home__summary"
        aria-label="Scan summary"
      >
        {categories.map((state) => (
          <ToggleGroupItem className="home__stat" key={state} value={state} aria-controls="home-file-list">
            <span className="home__stat-value">{counts[state]}</span>
            <span className="home__stat-label">{labels[state]}</span>
            <span className={`home__stat-mark home__stat-mark--${state}`} aria-hidden="true" />
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <Card className="home__files">
        <div className="home__list-heading">
          <h2>File status</h2>
          <output>{rows.length} locations</output>
        </div>
        {locations.length === 0 && !loading && (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No scans yet</EmptyTitle>
              <EmptyDescription>Select a file or folder from Watch to start scanning.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
        {locations.length > 0 && rows.length === 0 && !loading && (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No matching locations</EmptyTitle>
              <EmptyDescription>No files or folders have this status.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        )}
        <ul className="home__file-list" id="home-file-list">
          {rows.map(({ location, scan }) => (
            <FileRow key={location.id} location={location} scan={scan} advanced={advanced} health={health.find((entry) => entry.id === location.id)} />
          ))}
        </ul>
      </Card>
    </section>
  )
}
