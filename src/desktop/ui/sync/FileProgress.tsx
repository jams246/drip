import './progress.css'

const percentageMaximum = 100

interface FileProgressProps {
  value?: number
  max: number
  label: string
}

export function FileProgress({ value, max, label }: FileProgressProps) {
  const hasValue = value !== undefined && Number.isFinite(value) && Number.isFinite(max) && max > 0
  const boundedValue = hasValue ? Math.min(max, Math.max(0, value)) : undefined
  const percent = boundedValue === undefined ? undefined : Math.round((boundedValue / max) * percentageMaximum)
  return (
    <div className="file-progress">
      <div className="file-progress__label">
        <span>{label}</span>
        {percent !== undefined && <span className="file-progress__value">{percent}%</span>}
      </div>
      <div className={`file-progress__track${hasValue ? '' : ' file-progress__track--indeterminate'}`}>
        <progress className="file-progress__bar" value={boundedValue} max={hasValue ? max : percentageMaximum} aria-label={label} />
      </div>
    </div>
  )
}
