import { memo } from 'react'
import { FileProgress } from '../sync/FileProgress'
import type { SyncFile } from '../sync/types'

interface FileRowProps {
  file: SyncFile
  paused: boolean
  blocked: boolean
  onViewActivity: (id: string) => void
}

export const FileRow = memo(function FileRow({ file, paused, blocked, onViewActivity }: FileRowProps) {
  const active = file.status === 'preparing' || file.status === 'syncing'
  const progressLabel = blocked ? 'Waiting for connection' : file.stage
  return (
    <li className={`file-row file-row--${file.status}${file.stage === 'Hashing file' ? ' file-row--hashing' : ''}`}>
      <span className="file-row__icon" aria-hidden="true">
        {file.kind === 'folder' ? (
          <svg viewBox="0 0 24 24">
            <path d="M3 7h7l2-2h9v14H3Z" />
          </svg>
        ) : (
          <svg viewBox="0 0 24 24">
            <path d="M6 3h8l4 4v14H6Z" />
            <path d="M14 3v5h4" />
          </svg>
        )}
      </span>
      <div className="file-row__identity">
        <span className="file-row__name">{file.name}</span>
        <span className="file-row__path">{file.path}</span>
      </div>
      <span className="file-row__size">{file.detail}</span>
      <div className="file-row__state">
        {active && <FileProgress value={file.value} max={file.max} label={progressLabel} paused={paused && !blocked} />}
        {file.status === 'synced' && (
          <span className="status-label status-label--success">
            <span aria-hidden="true">✓</span> Synced
          </span>
        )}
        {file.status === 'error' && (
          <>
            <span className="status-label status-label--error">Needs attention</span>
            <button className="file-row__activity" type="button" onClick={() => onViewActivity(file.errorActivityId ?? file.id)}>
              View activity <span aria-hidden="true">↗</span>
            </button>
          </>
        )}
      </div>
    </li>
  )
})
