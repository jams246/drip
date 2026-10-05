import type { ScanLocation } from '../../scan/types'
import { ScanNotices } from '../ScanNotices'
import { Alert, AlertDescription } from '../components/alert'
import { Badge } from '../components/badge'
import { Button } from '../components/button'
import { Card } from '../components/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '../components/empty'
import './watch.css'

interface WatchProps {
  items: readonly ScanLocation[]
  busy: boolean
  picking: boolean
  loading?: boolean
  available?: boolean
  error?: string
  onSelect: (kind: ScanLocation['kind']) => void
  onRemove: (id: string) => void
}

export function Watch({ items, busy, picking, loading = false, available = true, error, onSelect, onRemove }: WatchProps) {
  return (
    <section className="watch" aria-labelledby="watch-title">
      <div className="page-heading">
        <h1 className="page-heading__title" id="watch-title">
          Watch
        </h1>
        <div className="watch__actions">
          <Button variant="outline" type="button" disabled={picking || !available} onClick={() => onSelect('file')}>
            Select file
          </Button>
          <Button type="button" disabled={picking || !available} onClick={() => onSelect('folder')}>
            Select folder
          </Button>
        </div>
      </div>
      {error && (
        <Alert className="watch__error" variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <ScanNotices loading={loading} picking={picking} loadingMessage="Loading saved watch locations." className="watch__notice" />
      {busy && !picking && !loading && (
        <Alert asChild className="watch__notice">
          <output>
            <AlertDescription>Processing continues. Additional selections join the queue.</AlertDescription>
          </output>
        </Alert>
      )}
      <Card className="watch__list">
        <div className="watch__list-heading">
          <h2 className="watch__list-title">Selected locations</h2>
          <span className="watch__count">
            {items.length} {items.length === 1 ? 'location' : 'locations'}
          </span>
        </div>
        {items.length === 0 && !loading ? (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No selected locations</EmptyTitle>
              <EmptyDescription>Select a file or folder to scan its contents.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <ul className="watch__items">
            {items.map((item) => {
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
                  <Badge variant="outline" className="watch__type">
                    {item.kind === 'folder' ? 'Folder' : 'File'}
                  </Badge>
                  <Button
                    variant="outline"
                    className="watch__remove"
                    type="button"
                    aria-label={`Remove ${item.name} from watch list`}
                    onClick={() => onRemove(item.id)}
                  >
                    Remove
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </Card>
    </section>
  )
}
