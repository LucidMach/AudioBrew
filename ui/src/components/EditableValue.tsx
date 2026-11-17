import { useRef, useState } from 'react'

type Props<T> = {
  value: T
  /** Text shown in the readout and in the field while editing. */
  format: (v: T) => string
  /** Unit shown after the value, outside the field (e.g. "dB"). */
  unit?: string
  parse: (text: string) => T | null
  /** One nudge step; fine = Shift held. */
  nudge: (v: T, direction: 1 | -1, fine: boolean) => T
  onChange: (v: T) => void
  label: string
  /** Tooltip shown on hover (see TooltipLayer). */
  tip: string
  className?: string
}

/**
 * A readout that becomes a text field on click. Enter or clicking away commits,
 * Escape restores the value from before editing, unreadable text is ignored.
 * While editing, Up/Down nudge the value (Shift = fine) and apply it live.
 */
export function EditableValue<T>({ value, format, unit, parse, nudge, onChange, label, tip, className = '' }: Props<T>) {
  const [text, setText] = useState<string | null>(null)
  const before = useRef(value)
  const cancelled = useRef(false)
  const editing = text !== null

  const start = () => {
    before.current = value
    cancelled.current = false
    setText(format(value))
  }

  const commit = () => {
    if (!cancelled.current && text !== null) {
      const parsed = parse(text)
      if (parsed !== null) onChange(parsed)
    }
    setText(null)
  }

  if (!editing) {
    return (
      <button className={`readout readout-button ${className}`} aria-label={`${label}: ${format(value)}${unit ? ` ${unit}` : ''}`}
        data-tip={tip} onClick={start}>
        {format(value)}
        {unit && ` ${unit}`}
      </button>
    )
  }

  return (
    <span className={`readout readout-editing ${className}`} data-tip={tip}>
      <input
        className="readout-input"
        autoFocus
        aria-label={label}
        value={text}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.currentTarget.blur()
          } else if (e.key === 'Escape') {
            cancelled.current = true
            onChange(before.current)
            e.currentTarget.blur()
          } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault()
            const current = parse(text) ?? value
            const next = nudge(current, e.key === 'ArrowUp' ? 1 : -1, e.shiftKey)
            onChange(next)
            setText(format(next))
          }
        }}
      />
      {unit && <span className="readout-unit">{unit}</span>}
    </span>
  )
}
