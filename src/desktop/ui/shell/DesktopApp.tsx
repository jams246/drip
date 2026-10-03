import { type Dispatch, type SetStateAction, useCallback, useRef, useState } from 'react'
import { Activity } from '../activity/Activity'
import { Brand } from '../brand/Brand'
import { Home } from '../home/Home'
import { Settings } from '../settings/Settings'
import type { ThemeName } from '../settings/themes'
import { sampleActivity, sampleWatches } from '../sync/samples'
import type { ActivityEntry, SyncFile } from '../sync/types'
import { Watch } from '../watch/Watch'
import { useConnection } from './useConnection'
import './common.css'
import './shell.css'

const pages = ['Home', 'Watch', 'Activity', 'Settings'] as const
type Page = (typeof pages)[number]

interface DesktopAppProps {
  files: SyncFile[]
  onFilesChange: Dispatch<SetStateAction<SyncFile[]>>
  paused: boolean
  onPauseChange: (paused: boolean) => void
}

export function DesktopApp({ files, onFilesChange, paused, onPauseChange }: DesktopAppProps) {
  const [page, setPage] = useState<Page>('Home')
  const [theme, setTheme] = useState<ThemeName>('graphite')
  const [watches, setWatches] = useState(sampleWatches)
  const [entries, setEntries] = useState(sampleActivity)
  const [highlightedId, setHighlightedId] = useState<string>()
  const content = useRef<HTMLElement>(null)
  const addActivity = useCallback((entry: Omit<ActivityEntry, 'id' | 'time'>) => {
    const time = new Date().toISOString()
    setEntries((current) => [{ ...entry, id: `${time}-${current.length}`, time }, ...current])
  }, [])
  const registration = useConnection(paused, onPauseChange, addActivity)
  const { connection } = registration
  const viewActivity = useCallback((id: string) => {
    setHighlightedId(id)
    setPage('Activity')
  }, [])

  function changePause(next: boolean) {
    onPauseChange(next)
    addActivity({
      title: next ? 'Syncing paused' : 'Syncing resumed',
      detail: next ? 'Current progress is saved. Resume when you are ready.' : 'Watched files can continue uploading.',
      severity: 'info'
    })
  }

  function removeWatch(id: string) {
    const item = watches.find((watch) => watch.id === id)
    if (!item) return
    setWatches((current) => current.filter((watch) => watch.id !== id))
    onFilesChange((current) => current.filter((file) => file.watchId !== id))
    addActivity({
      title: 'Item removed from watch list',
      detail: 'DRIP will no longer watch this item. The local file or folder is unchanged.',
      path: item.path,
      severity: 'info'
    })
  }

  function navigate(next: Page) {
    setPage(next)
    setHighlightedId(undefined)
    content.current?.scrollTo({ top: 0 })
  }

  const connectionLabels = { connected: 'Connected', connecting: 'Connecting', disconnected: 'Disconnected', error: 'Connection error' }
  return (
    <div className="desktop" data-theme={theme}>
      <header className="desktop__header">
        <Brand />
        <span className="desktop__sample">Sample data</span>
      </header>
      <nav className="navigation" aria-label="Main navigation">
        {pages.map((name) => (
          <button
            key={name}
            className={`navigation__button${page === name ? ' navigation__button--active' : ''}`}
            type="button"
            aria-current={page === name ? 'page' : undefined}
            onClick={() => navigate(name)}
          >
            {name}
          </button>
        ))}
      </nav>
      <main className="desktop__content" id="main-content" ref={content}>
        {page === 'Home' && <Home files={files} paused={paused} connection={connection} onPauseChange={changePause} onViewActivity={viewActivity} />}
        {page === 'Watch' && <Watch items={watches} onRemove={removeWatch} />}
        {page === 'Activity' && <Activity entries={entries} highlightedId={highlightedId} />}
        {page === 'Settings' && (
          <Settings
            theme={theme}
            onThemeChange={setTheme}
            connection={connection}
            serverUrl={registration.serverUrl}
            registrationCode={registration.registrationCode}
            onServerUrlChange={registration.setServerUrl}
            onRegistrationCodeChange={registration.setRegistrationCode}
            onConnect={registration.connect}
          />
        )}
      </main>
      <footer className={`desktop__footer${connection.status === 'error' ? ' desktop__footer--error' : ''}`}>
        <output className={`desktop__connection desktop__connection--${connection.status}`} role={connection.status === 'error' ? 'alert' : 'status'}>
          {connectionLabels[connection.status]}
        </output>
        <span>
          DRIP <span className="desktop__version">0.1.0</span>
        </span>
      </footer>
    </div>
  )
}
