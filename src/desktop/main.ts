import { claimInstance, initializeDiagnostics, setWindowIcon } from '#drip-window-icon'
import { readEmbedded } from 'perry'
import { App, WebView, alert, widgetSetBackgroundColor } from 'perry/ui'
import { startScanBridge } from './scan/host'
import { recordDiagnostic } from './diagnostics'

const COLOR_CHANNEL_MAX = 255
const STARTUP_RED = 19
const STARTUP_GREEN = 22
const STARTUP_BLUE = 27

if (claimInstance()) startDesktop()

function startDesktop() {
  const diagnosticsReady = initializeDiagnostics()
  if (!diagnosticsReady) alert('Crash logging unavailable', 'Drip could not create crash logs. Check write access to the logs folder beside drip.exe.')
  recordDiagnostic('desktop.start')
  const html = readEmbedded('dist/desktop/index.html')
  let bridgeStarted = false
  process.env.WEBVIEW2_DEFAULT_BACKGROUND_COLOR = 'FF13161B'

  const webview: ReturnType<typeof WebView> = WebView({
    url: 'data:text/html;base64,' + html.toString('base64'),
    width: 1024,
    height: 768,
    onLoaded: () => {
      recordDiagnostic('desktop.webview.loaded')
      setWindowIcon()
      if (bridgeStarted) return
      bridgeStarted = true
      startScanBridge(webview)
    },
    onError: (code: number, message: string) => {
      recordDiagnostic('desktop.webview.error', String(code))
      alert('Drip could not load', `WebView2 error ${code}: ${message}`)
    }
  })

  widgetSetBackgroundColor(webview, STARTUP_RED / COLOR_CHANNEL_MAX, STARTUP_GREEN / COLOR_CHANNEL_MAX, STARTUP_BLUE / COLOR_CHANNEL_MAX, 1)
  App({
    title: 'DRIP',
    width: 1024,
    height: 768,
    body: webview
  })
}
