export type SyncErrorCode = 'invalid_request' | 'not_found' | 'conflict' | 'revision_conflict' | 'regions_missing' | 'retired' | 'storage_failure'

export class SyncError extends Error {
  constructor(
    readonly code: SyncErrorCode,
    message: string
  ) {
    super(message)
    this.name = 'SyncError'
  }
}
