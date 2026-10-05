import { useCallback, useEffect, useRef, useState } from 'react'
import { Activity } from '../activity/Activity'
import { Brand } from '../brand/Brand'
import { ConfirmationDialog } from '../confirmation/ConfirmationDialog'
import { useConfirmation } from '../confirmation/useConfirmation'
import { Home } from '../home/Home'
import { Settings } from '../settings/Settings'
import type { ThemeName } from '../settings/themes'
import type { ActivityEntry } from '../sync/types'
import { Watch } from '../watch/Watch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/tabs'
import { Badge } from '../components/badge'
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
  useEffect(() => {
    document.documentElement.dataset.theme = theme
    document.documentElement.classList.toggle('dark', theme === 'graphite')
  }, [theme])
  const [advanced, setAdvanced] = useState(false)
  const [entries, setEntries] = useState<ActivityEntry[]>([])
  const content = useRef<HTMLElement>(null)
  const navigation = useRef<HTMLButtonElement>(null)
  const addActivity = useCallback((entry: Omit<ActivityEntry, 'id' | 'time'>) => {
    const time = new Date().toISOString()
    // ponytail: retain 200 recent entries; add persistent history when that phase starts.
    setEntries((current) => [{ ...entry, id: `${time}-${current.length}`, time }, ...current].slice(0, retainedActivityEntries))
  }, [])
  const scans = useScans(addActivity)
  const confirmation = useConfirmation(scans, entries, () => setEntries([]))
  const registration = useConnection(scans.sync, scans.connect)
  const { connection } = registration

  function changePause(next: boolean) {
    scans.pause(next)
    addActivity({
      title: next ? 'Watching paused' : 'Watching resumed',
      detail: next ? 'File watching and hashing stop.' : 'File watching and metadata reconciliation resume.',
      severity: 'info'
    })
  }

  function navigate(next: Page) {
    setPage(next)
    content.current?.scrollTo({ top: 0 })
  }

  const connectionLabels = { connected: 'Connected', connecting: 'Connecting', disconnected: 'Disconnected', error: 'Connection error' }
  return (
    <Tabs
      className="desktop"
      value={page}
      onValueChange={(next) => {
        const nextPage = pages.find((name) => name === next)
        if (nextPage) navigate(nextPage)
      }}
    >
      <header className="desktop__header">
        <Brand />
      </header>
      <nav className="navigation" aria-label="Main navigation">
        <TabsList className="navigation__list" aria-label="Pages">
          {pages.map((name) => (
            <TabsTrigger
              ref={page === name ? navigation : undefined}
              key={name}
              value={name}
              className="navigation__button"
              aria-current={page === name ? 'page' : undefined}
            >
              {name}
            </TabsTrigger>
          ))}
        </TabsList>
      </nav>
      <main className="desktop__content" id="main-content" ref={content} tabIndex={-1}>
        <TabsContent value={page} key={page} className="desktop__page" tabIndex={undefined}>
          {page === 'Home' && (
            <Home
              locations={scans.locations}
              scans={scans.scans}
              sync={scans.sync}
              picking={scans.picking}
              loading={scans.storage === 'loading'}
              error={scans.error}
              advanced={advanced}
              paused={scans.paused}
              verifying={scans.verifying}
              health={scans.health}
              onVerify={confirmation.verify}
              onPauseChange={changePause}
            />
          )}
          {page === 'Watch' && (
            <Watch
              items={scans.locations}
              busy={scans.busy}
              picking={scans.picking}
              loading={scans.storage === 'loading'}
              available={scans.storage === 'ready'}
              error={scans.error}
              onSelect={scans.select}
              onRemove={confirmation.remove}
            />
          )}
          {page === 'Activity' && <Activity entries={entries} onClear={confirmation.clear} />}
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
        </TabsContent>
      </main>
      <footer className={`desktop__footer${connection.status === 'error' ? ' desktop__footer--error' : ''}`}>
        <output className={`desktop__connection desktop__connection--${connection.status}`} role={connection.status === 'error' ? 'alert' : 'status'}>
          <Badge variant="outline" className="desktop__connection-badge">
            <span className="desktop__connection-dot" aria-hidden="true" />
            {connectionLabels[connection.status]}
          </Badge>
        </output>
        <span>
          DRIP <span className="desktop__version">0.1.0</span>
        </span>
      </footer>
      <ConfirmationDialog
        confirmation={confirmation.confirmation}
        available={confirmation.available}
        fallbackFocus={navigation}
        onConfirm={confirmation.confirm}
        onDismiss={confirmation.dismiss}
      />
    </Tabs>
  )
}
