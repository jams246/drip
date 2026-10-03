import { isScanActive } from '../../scan/state'
import type { ScanLocation, ScanSnapshot } from '../../scan/types'
import './watch.css'

interface WatchProps {
  items: readonly ScanLocation[]
  scans: readonly ScanSnapshot[]
  busy: boolean
  picking: boolean
  error?: string
  onSelect: (kind: ScanLocation['kind']) => void
  onRemove: (id: string) => void
}

export function Watch({ items, scans, busy, picking, error, onSelect, onRemove }: WatchProps) {
  return (
    <section className="watch" aria-labelledby="watch-title">
      <div className="page-heading">
        <h1 className="page-heading__title" id="watch-title">
          Watch
        </h1>
        <div className="watch__actions">
          <button className="button button--secondary" type="button" disabled={picking} onClick={() => onSelect('file')}>
            Select file
          </button>
          <button className="button button--primary" type="button" disabled={picking} onClick={() => onSelect('folder')}>
            Select folder
          </button>
        </div>
      </div>
      {error && (
        <p className="watch__error" role="alert">
          {error}
        </p>
      )}
      {picking && <output className="watch__notice">Choose a file or folder in the selection dialog.</output>}
      {busy && !picking && <output className="watch__notice">Processing continues. Additional selections join the queue.</output>}
      <div className="watch__list surface">
        <div className="watch__list-heading">
          <h2 className="watch__list-title">Selected locations</h2>
          <span className="watch__count">
            {items.length} {items.length === 1 ? 'location' : 'locations'}
          </span>
        </div>
        {items.length === 0 ? (
          <div className="empty-state">
            <h3>No selected locations</h3>
            <p>Select a file or folder to scan its contents.</p>
          </div>
        ) : (
          <ul className="watch__items">
            {items.map((item) => {
              const scan = scans.find((entry) => entry.id === item.id)
              return (
                <li className="watch__item" key={item.id}>
                  <span className="watch__icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                      <path d={item.kind === 'folder' ? 'M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10H3Z' : 'M6 3h8l4 4v14H6ZM14 3v5h4M9 12h6M9 16h6'} />
                    </svg>
                  </span>
                  <div className="watch__item-copy">
                    <h3 className="watch__name">{item.name}</h3>
                    <p className="watch__path">{item.path}</p>
                  </div>
                  <span className="watch__type">{item.kind === 'folder' ? 'Folder' : 'File'}</span>
                  <button
                    className="button button--secondary watch__remove"
                    type="button"
                    disabled={scan !== undefined && isScanActive(scan)}
                    aria-label={`Remove ${item.name} from watch list`}
                    onClick={() => onRemove(item.id)}
                  >
                    Remove
                  </button>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </section>
  )
}
