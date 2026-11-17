import { useEffect, useLayoutEffect, useRef, useState } from 'react'

/**
 * One tooltip for the whole app, drawn by us so it looks the same on every
 * platform (native `title` tooltips differ per OS and browser).
 *
 * Any element with `data-tip={'Heading\nline\nline'}` gets a tooltip on hover
 * or keyboard focus. The first line is the heading. Touch input is ignored.
 * (Use a JS string in braces: JSX "quoted" attributes do not turn \n into a newline.)
 */

const SHOW_DELAY_MS = 450
const GAP = 8
const MARGIN = 8

type Tip = { text: string; anchor: DOMRect }

export function TooltipLayer() {
  const [tip, setTip] = useState<Tip | null>(null)
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let timer: number | undefined
    let current: Element | null = null

    const hide = () => {
      window.clearTimeout(timer)
      current = null
      setTip(null)
      setPos(null)
    }

    const showFor = (el: Element | null, delay: number) => {
      if (el === current) return
      window.clearTimeout(timer)
      current = el
      setTip(null)
      setPos(null)
      if (!el) return
      timer = window.setTimeout(() => {
        const text = el.getAttribute('data-tip')
        if (text && el.isConnected) setTip({ text, anchor: el.getBoundingClientRect() })
      }, delay)
    }

    const tipTarget = (t: EventTarget | null) => (t instanceof Element ? t.closest('[data-tip]') : null)

    const onOver = (e: PointerEvent) => {
      if (e.pointerType === 'touch') return
      showFor(tipTarget(e.target), SHOW_DELAY_MS)
    }
    const onFocus = (e: FocusEvent) => {
      const el = tipTarget(e.target)
      if (el && (e.target as Element).matches(':focus-visible')) showFor(el, SHOW_DELAY_MS)
    }

    document.addEventListener('pointerover', onOver)
    document.addEventListener('focusin', onFocus)
    document.addEventListener('focusout', hide)
    document.addEventListener('pointerdown', hide, true)
    document.addEventListener('keydown', hide, true)
    window.addEventListener('scroll', hide, true)
    window.addEventListener('blur', hide)
    return () => {
      window.clearTimeout(timer)
      document.removeEventListener('pointerover', onOver)
      document.removeEventListener('focusin', onFocus)
      document.removeEventListener('focusout', hide)
      document.removeEventListener('pointerdown', hide, true)
      document.removeEventListener('keydown', hide, true)
      window.removeEventListener('scroll', hide, true)
      window.removeEventListener('blur', hide)
    }
  }, [])

  // Place above the control, or below if there is no room; keep inside the window.
  useLayoutEffect(() => {
    if (!tip || !box.current) return
    const { width, height } = box.current.getBoundingClientRect()
    const a = tip.anchor
    let top = a.top - GAP - height
    if (top < MARGIN) top = a.bottom + GAP
    const left = Math.min(Math.max(a.left + a.width / 2 - width / 2, MARGIN), window.innerWidth - width - MARGIN)
    setPos({ left, top })
  }, [tip])

  if (!tip) return null
  const [heading, ...lines] = tip.text.split('\n')

  return (
    <div
      ref={box}
      className="tooltip"
      role="tooltip"
      style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: 'hidden' }}
    >
      <div className="tooltip-heading">{heading}</div>
      {lines.map((line, i) => (
        <div key={i} className="tooltip-line">
          {line}
        </div>
      ))}
    </div>
  )
}
