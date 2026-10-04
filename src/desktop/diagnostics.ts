import { writeDiagnostic } from '#drip-window-icon'

export function recordDiagnostic(event: string, details = ''): void {
  try {
    writeDiagnostic(JSON.stringify({ event, details }))
  } catch {
    // Diagnostics must not interrupt scanning or synchronization.
  }
}
