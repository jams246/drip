import { claimInstance, initializeDiagnostics, setWindowIcon } from '#drip-window-icon'
import { readEmbedded } from 'perry'
import { App, WebView, alert } from 'perry/ui'
import { startScanBridge } from './scan/host'
import { recordDiagnostic } from './diagnostics'

if (claimInstance()) startDesktop()

function startDesktop() {
  const diagnosticsReady = initializeDiagnostics()
  if (!diagnosticsReady) alert('Crash logging unavailable', 'Drip could not create crash logs. Check write access to the logs folder beside drip.exe.')
  recordDiagnostic('desktop.start')
  const html = readEmbedded('dist/desktop/index.html')
  let bridgeStarted = false

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

  App({
    title: 'DRIP',
    width: 1024,
    height: 768,
    body: webview
  })
}
