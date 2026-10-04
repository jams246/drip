import { claimInstance, setWindowIcon } from '#drip-window-icon'
import { readEmbedded } from 'perry'
import { App, WebView, alert } from 'perry/ui'
import { startScanBridge } from './scan/host'

if (claimInstance()) startDesktop()

function startDesktop() {
  const html = readEmbedded('dist/desktop/index.html')
  let bridgeStarted = false

  const webview: ReturnType<typeof WebView> = WebView({
    url: 'data:text/html;base64,' + html.toString('base64'),
    width: 1024,
    height: 768,
    onLoaded: () => {
      setWindowIcon()
      if (bridgeStarted) return
      bridgeStarted = true
      startScanBridge(webview)
    },
    onError: (code: number, message: string) => alert('Drip could not load', `WebView2 error ${code}: ${message}`)
  })

  App({
    title: 'DRIP',
    width: 1024,
    height: 768,
    body: webview
  })
}
