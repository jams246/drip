import { recordDiagnostic } from '../diagnostics'
import { SyncHttpError } from './http'

export function recordSyncError(event: string, error: unknown, details = ''): void {
  recordDiagnostic(event, `${details} status=${error instanceof SyncHttpError ? error.status : 0} error=${error instanceof Error ? error.name : 'unknown'}`)
}
