import { useRef } from 'react'

type Props = {
  /** -1 .. 1 */
  value: number
  onChange: (v: number) => void
  label: string
  valueText: string
}

const SWEEP = 135 // degrees either side of centre

/** Bipolar knob (pan). Drag up/down; Shift drags finely; double-click centres. */
export function Knob({ value, onChange, label, valueText }: Props) {
  const drag = useRef<{ y: number; v: number } | null>(null)
  const clamp = (v: number) => Math.min(1, Math.max(-1, v))
  const angle = value * SWEEP

  const r = 11
  const arc = (a: number) => {
    const rad = ((a - 90) * Math.PI) / 180
    return [14 + r * Math.cos(rad), 14 + r * Math.sin(rad)]
  }
  const [x0, y0] = arc(0)
  const [x1, y1] = arc(angle)
  const large = Math.abs(angle) > 180 ? 1 : 0
  const sweep = angle > 0 ? 1 : 0

  return (
    <div
      className="knob"
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={-1}
      aria-valuemax={1}
      aria-valuenow={value}
      aria-valuetext={valueText}
      title={`${label}: ${valueText}`}
      onPointerDown={(e) => {
        drag.current = { y: e.clientY, v: value }
        e.currentTarget.setPointerCapture(e.pointerId)
      }}
      onPointerMove={(e) => {
        if (!drag.current) return
        const scale = e.shiftKey ? 0.002 : 0.01
        onChange(clamp(drag.current.v + (drag.current.y - e.clientY) * scale))
      }}
      onPointerUp={() => (drag.current = null)}
      onDoubleClick={() => onChange(0)}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 0.02 : 0.1
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(clamp(value + step))
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(clamp(value - step))
        else return
        e.preventDefault()
      }}
    >
      <svg viewBox="0 0 28 28" width="28" height="28" aria-hidden>
        <circle cx="14" cy="14" r={r} className="knob-ring" />
        {angle !== 0 && (
          <path d={`M ${x0} ${y0} A ${r} ${r} 0 ${large} ${sweep} ${x1} ${y1}`} className="knob-arc" />
        )}
        <circle cx="14" cy="14" r="7.5" className="knob-cap" />
        <line x1="14" y1="14" x2="14" y2="8" className="knob-pointer" transform={`rotate(${angle} 14 14)`} />
      </svg>
    </div>
  )
}
