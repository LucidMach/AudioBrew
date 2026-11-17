import { useRef } from 'react'

type Props = {
  value: number
  defaultValue: number
  onChange: (v: number) => void
  label: string
  valueText: string
  color: string
}

/** Horizontal fader. Drag or click to set; Shift drags finely; double-click resets. */
export function Fader({ value, defaultValue, onChange, label, valueText, color }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const drag = useRef<{ x: number; v: number } | null>(null)

  const clamp = (v: number) => Math.min(1, Math.max(0, v))

  return (
    <div
      ref={ref}
      className="fader"
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={value}
      aria-valuetext={valueText}
      style={{ '--fill': `${value * 100}%`, '--accent': color } as React.CSSProperties}
      onPointerDown={(e) => {
        const rect = ref.current!.getBoundingClientRect()
        const v = clamp((e.clientX - rect.left) / rect.width)
        if (!(e.target as HTMLElement).classList.contains('fader-thumb')) onChange(v)
        drag.current = { x: e.clientX, v: (e.target as HTMLElement).classList.contains('fader-thumb') ? value : v }
        ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
      }}
      onPointerMove={(e) => {
        if (!drag.current) return
        const width = ref.current!.getBoundingClientRect().width
        const scale = e.shiftKey ? 0.1 : 1
        onChange(clamp(drag.current.v + ((e.clientX - drag.current.x) / width) * scale))
      }}
      onPointerUp={() => (drag.current = null)}
      onDoubleClick={() => onChange(defaultValue)}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 0.01 : 0.05
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(clamp(value + step))
        else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(clamp(value - step))
        else return
        e.preventDefault()
      }}
    >
      <div className="fader-track">
        <div className="fader-fill" />
        <div className="fader-unity" style={{ left: `${defaultValue * 100}%` }} />
      </div>
      <div className="fader-thumb" />
    </div>
  )
}
