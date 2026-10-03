interface ScanNoticesProps {
  loading: boolean
  picking: boolean
  loadingMessage: string
  className: string
}

export function ScanNotices({ loading, picking, loadingMessage, className }: ScanNoticesProps) {
  return (
    <>
      {loading && <output className={className}>{loadingMessage}</output>}
      {picking && <output className={className}>Choose a file or folder in the selection dialog, then wait for the watch location to save.</output>}
    </>
  )
}
