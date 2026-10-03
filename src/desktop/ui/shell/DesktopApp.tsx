import { useCallback, useRef, useState } from 'react'
import { Activity } from '../activity/Activity'
import { Brand } from '../brand/Brand'
import { Home } from '../home/Home'
import { Settings } from '../settings/Settings'
import type { ThemeName } from '../settings/themes'
import type { ActivityEntry } from '../sync/types'
import { Watch } from '../watch/Watch'
import { useConnection } from './useConnection'
import { useScans } from './useScans'
import './common.css'
import './shell.css'

const pages = ['Home', 'Watch', 'Activity', 'Settings'] as const
const retainedActivityEntries = 200
type Page = (typeof pages)[number]

export function DesktopApp() {
  const [page, setPage] = useState<Page>('Home')
  const [theme, setTheme] = useState<ThemeName>('graphite')
  const [advanced, setAdvanced] = useState(false)
  const [paused, setPaused] = useState(false)
  const [entries, setEntries] = useState<ActivityEntry[]>([])
  const content = useRef<HTMLElement>(null)
  const addActivity = useCallback((entry: Omit<ActivityEntry, 'id' | 'time'>) => {
    const time = new Date().toISOString()
    // ponytail: retain 200 recent entries; add persistent history when that phase starts.
    setEntries((current) => [{ ...entry, id: `${time}-${current.length}`, time }, ...current].slice(0, retainedActivityEntries))
  }, [])
  const onSelected = useCallback(() => setPage('Home'), [])
  const scans = useScans(onSelected, addActivity)
  const registration = useConnection(addActivity)
  const { connection } = registration

  function changePause(next: boolean) {
    setPaused(next)
    addActivity({ title: next ? 'Syncing paused' : 'Syncing resumed', detail: 'Local file processing continues.', severity: 'info' })
  }

  function navigate(next: Page) {
    setPage(next)
    content.current?.scrollTo({ top: 0 })
  }

  const connectionLabels = { connected: 'Connected', connecting: 'Connecting', disconnected: 'Disconnected', error: 'Connection error' }
  return (
    <div className="desktop" data-theme={theme}>
      <header className="desktop__header">
        <Brand />
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
        {page === 'Home' && (
          <Home
            locations={scans.locations}
            scans={scans.scans}
            picking={scans.picking}
            loading={scans.storage === 'loading'}
            error={scans.error}
            advanced={advanced}
            paused={paused}
            connection={connection}
            onPauseChange={changePause}
          />
        )}
        {page === 'Watch' && (
          <Watch
            items={scans.locations}
            scans={scans.scans}
            busy={scans.busy}
            picking={scans.picking}
            loading={scans.storage === 'loading'}
            available={scans.storage === 'ready'}
            error={scans.error}
            onSelect={scans.select}
            onRemove={scans.remove}
          />
        )}
        {page === 'Activity' && <Activity entries={entries} />}
        {page === 'Settings' && (
          <Settings
            theme={theme}
            onThemeChange={setTheme}
            advanced={advanced}
            onAdvancedChange={setAdvanced}
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
