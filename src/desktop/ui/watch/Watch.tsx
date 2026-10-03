import type { WatchItem } from '../sync/types'
import './watch.css'

interface WatchProps {
  items: readonly WatchItem[]
  onRemove: (id: string) => void
}

export function Watch({ items, onRemove }: WatchProps) {
  return (
    <section className="watch" aria-labelledby="watch-title">
      <div className="page-heading">
        <h1 className="page-heading__title" id="watch-title">
          Watch
        </h1>
      </div>

      <div className="watch__list surface">
        <div className="watch__list-heading">
          <h2 className="watch__list-title">Watched locations</h2>
          <span className="watch__count">
            {items.length} {items.length === 1 ? 'location' : 'locations'}
          </span>
        </div>
        {items.length === 0 ? (
          <div className="empty-state">
            <h3>No watched locations</h3>
            <p>Files and folders you watch will appear here.</p>
          </div>
        ) : (
          <ul className="watch__items">
            {items.map((item) => (
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
                  aria-label={`Remove ${item.name} from watch list`}
                  onClick={() => onRemove(item.id)}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}
