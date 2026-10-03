import { memo } from 'react'
import { isScanActive, isScanFinished } from '../../scan/state'
import type { ScanLocation, ScanSnapshot } from '../../scan/types'
import { FileProgress } from '../sync/FileProgress'
import './file-row.css'

const kibibyte = 1024
const mebibyte = kibibyte * kibibyte
const gibibyte = mebibyte * kibibyte
const millisecondsPerSecond = 1000

const stateLabels: Record<ScanSnapshot['state'], string> = {
  queued: 'Queued',
  pending: 'Preparing scan',
  scanning: 'Chunking and hashing',
  completed: 'Completed',
  empty: 'No file data',
  error: 'Scan failed',
  'completed-with-errors': 'Completed with errors'
}

function formatBytes(bytes: number) {
  if (bytes < kibibyte) return `${bytes} B`
  if (bytes < mebibyte) return `${(bytes / kibibyte).toFixed(1)} KiB`
  if (bytes < gibibyte) return `${(bytes / mebibyte).toFixed(1)} MiB`
  return `${(bytes / gibibyte).toFixed(2)} GiB`
}

interface FileRowProps {
  location: ScanLocation
  scan: ScanSnapshot
  advanced: boolean
}

export const FileRow = memo(function FileRow({ location, scan, advanced }: FileRowProps) {
  const active = isScanActive(scan)
  const failed = scan.state === 'error' || scan.state === 'completed-with-errors'
  const throughput = scan.elapsedMs > 0 ? (scan.bytes * millisecondsPerSecond) / scan.elapsedMs : 0
  let label = stateLabels[scan.state]
  if (!advanced && scan.state === 'scanning') label = 'Processing'
  const currentFileBytes = scan.currentPath ? scan.currentBytes : undefined
  const currentFileName = scan.currentPath.split(/[\\/]/).pop()
  let statusClass = 'neutral'
  if (isScanFinished(scan)) statusClass = failed ? 'error' : 'success'
  return (
    <li className={`file-row file-row--${scan.state}`}>
      <span className="file-row__icon" aria-hidden="true">
        <svg viewBox="0 0 24 24">
          <path d={location.kind === 'folder' ? 'M3 7h7l2-2h9v14H3Z' : 'M6 3h8l4 4v14H6ZM14 3v5h4'} />
        </svg>
      </span>
      <div className="file-row__identity">
        <span className="file-row__name">{location.name}</span>
        {active && location.kind === 'folder' && currentFileName && <span className="file-row__current">Current file: {currentFileName}</span>}
        {advanced && (
          <dl className="file-row__metrics">
            <div>
              <dt>Processed</dt>
              <dd>{formatBytes(scan.bytes)}</dd>
            </div>
            <div>
              <dt>Chunks</dt>
              <dd>{scan.chunks}</dd>
            </div>
            <div>
              <dt>Files</dt>
              <dd>{scan.files}</dd>
            </div>
            <div>
              <dt>Skipped</dt>
              <dd>{scan.skipped}</dd>
            </div>
            <div>
              <dt>Errors</dt>
              <dd>{scan.errors}</dd>
            </div>
            <div>
              <dt>Elapsed</dt>
              <dd>{(scan.elapsedMs / millisecondsPerSecond).toFixed(1)} s</dd>
            </div>
            <div>
              <dt>Speed</dt>
              <dd>{formatBytes(throughput)}/s</dd>
            </div>
          </dl>
        )}
        {scan.error && (
          <p className="file-row__error" role="alert">
            {scan.error}
          </p>
        )}
      </div>
      <div className="file-row__state">
        {active ? (
          <FileProgress value={currentFileBytes} max={scan.currentSize} label={label} />
        ) : (
          <output className={`status-label status-label--${statusClass}`}>{label}</output>
        )}
        {advanced && active && scan.currentPath && (
          <span className="file-row__bytes">
            {formatBytes(scan.currentBytes)} / {formatBytes(scan.currentSize)}
          </span>
        )}
      </div>
    </li>
  )
})
