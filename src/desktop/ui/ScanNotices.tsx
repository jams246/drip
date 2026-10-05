import { Alert, AlertDescription } from './components/alert'
import { Spinner } from './components/spinner'

interface ScanNoticesProps {
  loading: boolean
  picking: boolean
  loadingMessage: string
  className: string
}

export function ScanNotices({ loading, picking, loadingMessage, className }: ScanNoticesProps) {
  return (
    <>
      {loading && (
        <Alert asChild className={className}>
          <output>
            <Spinner aria-hidden="true" />
            <AlertDescription>{loadingMessage}</AlertDescription>
          </output>
        </Alert>
      )}
      {picking && (
        <Alert asChild className={className}>
          <output>
            <Spinner aria-hidden="true" />
            <AlertDescription>Choose a file or folder in the selection dialog, then wait for the watch location to save.</AlertDescription>
          </output>
        </Alert>
      )}
    </>
  )
}
