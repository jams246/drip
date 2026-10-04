import type { Connection } from '../sync/types'
import { ThemePicker } from './ThemePicker'
import type { ThemeName } from './themes'
import './settings.css'

type SettingsProps = {
  theme: ThemeName
  onThemeChange: (theme: ThemeName) => void
  advanced: boolean
  onAdvancedChange: (advanced: boolean) => void
  connection: Connection
  onConnect: (url: string, code: string) => void
  serverUrl: string
  registrationCode: string
  onServerUrlChange: (value: string) => void
  onRegistrationCodeChange: (value: string) => void
}

export function Settings({
  theme,
  onThemeChange,
  advanced,
  onAdvancedChange,
  connection,
  onConnect,
  serverUrl,
  registrationCode,
  onServerUrlChange,
  onRegistrationCodeChange
}: SettingsProps) {
  const connecting = connection.status === 'connecting'
  const failed = connection.status === 'error'
  const statusText = {
    connected: 'Connected. Your files are ready to sync.',
    connecting: 'Connecting to your server…',
    disconnected: 'Connect to a server to get started.',
    error: 'Could not connect. Check your details and try again.'
  }[connection.status]

  return (
    <section className="settings" aria-labelledby="settings-title">
      <header className="page-heading">
        <h1 className="page-heading__title" id="settings-title">
          Settings
        </h1>
      </header>
      <div className="settings__layout">
        <section className="settings__panel" aria-labelledby="server-title">
          <h2 className="settings__title" id="server-title">
            Your server
          </h2>
          <p className="settings__description">Files travel one way, from this computer to your server.</p>
          <form
            className="settings__form"
            onSubmit={(event) => {
              event.preventDefault()
              onConnect(serverUrl, registrationCode)
            }}
          >
            <label className="settings__label" htmlFor="server-url">
              Server URL
            </label>
            <input
              className="settings__input"
              id="server-url"
              name="server-url"
              type="url"
              autoComplete="url"
              placeholder="https://sync.drip.example"
              value={serverUrl}
              onChange={(event) => onServerUrlChange(event.target.value)}
              disabled={connecting}
              required
            />
            <label className="settings__label" htmlFor="registration-code">
              Registration token
            </label>
            <input
              className="settings__input"
              id="registration-code"
              name="registration-code"
              type="password"
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              placeholder="Paste your registration token"
              value={registrationCode}
              onChange={(event) => onRegistrationCodeChange(event.target.value)}
              disabled={connecting}
              required
            />
            <p className="settings__hint">
              Generate a token on your server with <code>drip token create</code>.
            </p>
            <button className="button button--primary settings__connect" type="submit" disabled={connecting}>
              {connecting && <span className="settings__spinner" aria-hidden="true" />}
              {connecting ? 'Connecting…' : 'Connect'}
            </button>
            <div
              className={`settings__status settings__status--${connection.status}`}
              role={failed ? 'alert' : 'status'}
              aria-live={failed ? 'assertive' : 'polite'}
              aria-atomic="true"
            >
              <span className="settings__status-dot" aria-hidden="true" />
              <span>{connection.message || statusText}</span>
            </div>
          </form>
        </section>
        <section className="settings__panel settings__panel--appearance">
          <ThemePicker theme={theme} onThemeChange={onThemeChange} />
        </section>
        <section className="settings__panel" aria-labelledby="display-title">
          <h2 className="settings__title" id="display-title">
            Display
          </h2>
          <p className="settings__description" id="advanced-description">
            Show processing details and performance statistics on Home.
          </p>
          <label className="settings__toggle">
            <input
              type="checkbox"
              role="switch"
              checked={advanced}
              aria-checked={advanced}
              aria-describedby="advanced-description"
              onChange={(event) => onAdvancedChange(event.target.checked)}
            />
            <span>Advanced</span>
          </label>
        </section>
      </div>
    </section>
  )
}
