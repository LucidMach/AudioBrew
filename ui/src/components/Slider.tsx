import { useRef } from 'react'

type Props = {
  value: number
  min: number
  max: number
  /** Where double-click resets to; also drawn as a tick on the track. */
  defaultValue: number
  /** Fill grows from defaultValue (e.g. pan from centre) instead of from min. */
  fillFromDefault?: boolean
  /** Break points: dragging near one snaps to it. Shift-drag ignores them. */
  detents?: number[]
  onChange: (v: number) => void
  label: string
  valueText: string
  /** Tooltip shown on hover (see TooltipLayer). */
  tip?: string
}

/**
 * How close (in pixels) the thumb must come to a break point to snap onto it.
 * Shrunk where break points are close together, so there is always free
 * travel between them and the slider never feels stepped.
 */
const SNAP_PX = 6
const SNAP_SHARE_OF_GAP = 0.3

/**
 * Horizontal slider. Click or drag to set, snapping to break points;
 * Shift drags finely without snapping; double-click resets; arrow keys step.
 */
export function Slider({ value, min, max, defaultValue, fillFromDefault, detents = [], onChange, label, valueText, tip }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef<{ x: number; t: number } | null>(null)

  const span = max - min
  const toT = (v: number) => (v - min) / span
  const fromT = (t: number) => min + Math.min(1, Math.max(0, t)) * span

  const detentTs = detents.map(toT)

  /** Snap t onto the nearest break point whose snap zone it is in. */
  const snap = (t: number, width: number) => {
    let best = t
    let bestPx = Infinity
    detentTs.forEach((d, i) => {
      const gaps = [detentTs[i - 1], detentTs[i + 1]]
        .filter((n) => n !== undefined)
        .map((n) => Math.abs(n - d) * width)
      const radius = Math.min(SNAP_PX, ...gaps.map((g) => g * SNAP_SHARE_OF_GAP))
      const px = Math.abs(t - d) * width
      if (px <= radius && px < bestPx) {
        best = d
        bestPx = px
      }
    })
    return best
  }

  const t = toT(value)
  const tDefault = toT(defaultValue)
  const fillStart = fillFromDefault ? Math.min(t, tDefault) : 0
  const fillEnd = fillFromDefault ? Math.max(t, tDefault) : t

  return (
    <div
      ref={ref}
      className="slider"
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={valueText}
      data-tip={tip}
      style={{ '--pos': `${t * 100}%` } as React.CSSProperties}
      onPointerDown={(e) => {
        const onThumb = (e.target as HTMLElement).classList.contains('slider-thumb')
        let start = t
        if (!onThumb) {
          const rect = ref.current!.getBoundingClientRect()
          start = (e.clientX - rect.left) / rect.width
          onChange(fromT(e.shiftKey ? start : snap(start, rect.width)))
        }
        drag.current = { x: e.clientX, t: start }
        e.currentTarget.setPointerCapture(e.pointerId)
      }}
      onPointerMove={(e) => {
        if (!drag.current) return
        const width = ref.current!.getBoundingClientRect().width
        if (e.shiftKey) {
          onChange(fromT(drag.current.t + ((e.clientX - drag.current.x) / width) * 0.1))
        } else {
          onChange(fromT(snap(drag.current.t + (e.clientX - drag.current.x) / width, width)))
        }
      }}
      onPointerUp={() => (drag.current = null)}
      onDoubleClick={() => onChange(defaultValue)}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 0.01 : 0.05
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(fromT(t + step))
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(fromT(t - step))
        else if (e.key === 'Home') onChange(min)
        else if (e.key === 'End') onChange(max)
        else return
        e.preventDefault()
      }}
    >
      <div className="slider-track">
        <div className="slider-fill" style={{ left: `${fillStart * 100}%`, width: `${(fillEnd - fillStart) * 100}%` }} />
        {detentTs.map((d) => (
          <div key={d} className={`slider-tick${d === tDefault ? ' is-default' : ''}`} style={{ left: `${d * 100}%` }} />
        ))}
        {!detentTs.includes(tDefault) && <div className="slider-tick is-default" style={{ left: `${tDefault * 100}%` }} />}
      </div>
      <div className="slider-thumb" />
    </div>
  )
}
