import { useEffect, useRef } from 'react'
import type { ActivityEntry } from '../sync/types'
import { Badge } from '../components/badge'
import { Button } from '../components/button'
import { Card } from '../components/card'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '../components/empty'
import './activity.css'

interface ActivityProps {
  entries: readonly ActivityEntry[]
  highlightedId?: string
  onClear: () => void
}

const timeFormat = new Intl.DateTimeFormat(undefined, {
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit'
})

const severityLabels = {
  info: 'Info',
  success: 'Success',
  warning: 'Warning',
  error: 'Error'
}

export function Activity({ entries, highlightedId, onClear }: ActivityProps) {
  const highlightedEntry = useRef<HTMLLIElement>(null)

  useEffect(() => {
    if (!highlightedId) return
    highlightedEntry.current?.focus({ preventScroll: true })
    highlightedEntry.current?.scrollIntoView({ block: 'center' })
  }, [highlightedId])

  return (
    <section className="activity" aria-labelledby="activity-title">
      <div className="page-heading">
        <h1 className="page-heading__title" id="activity-title">
          Activity
        </h1>
        <Button variant="outline" type="button" disabled={entries.length === 0} onClick={onClear}>
          Clear all activity
        </Button>
      </div>

      <Card className="activity__list">
        <div className="activity__list-heading">
          <h2 className="activity__list-title">Recent activity</h2>
        </div>
        {entries.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyTitle>No activity yet</EmptyTitle>
              <EmptyDescription>Scan results and connection events will appear here.</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <ol className="activity__entries">
            {entries.map((entry) => {
              const isHighlighted = entry.id === highlightedId
              return (
                <li
                  className={`activity__entry activity__entry--${entry.severity}${isHighlighted ? ' activity__entry--highlighted' : ''}`}
                  key={entry.id}
                  ref={isHighlighted ? highlightedEntry : undefined}
                  tabIndex={isHighlighted ? -1 : undefined}
                >
                  <time className="activity__time" dateTime={entry.time}>
                    {timeFormat.format(new Date(entry.time))}
                  </time>
                  <Badge variant="outline" className="activity__severity">
                    {severityLabels[entry.severity]}
                  </Badge>
                  <div className="activity__entry-copy">
                    <h3 className="activity__title">{entry.title}</h3>
                    {entry.path && <p className="activity__path">{entry.path}</p>}
                    <p className="activity__detail">{entry.detail}</p>
                  </div>
                </li>
              )
            })}
          </ol>
        )}
      </Card>
    </section>
  )
}
