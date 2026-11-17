/** The first MIN_TRACKS tracks are always there; tracks added after them can be deleted. */
export const MIN_TRACKS = 2
/** Most tracks a project can have; one palette colour each. */
export const MAX_TRACKS = 6

export type TrackState = {
  id: number
  name: string
  color: string
  /** Fader position 0..1; see faderToDb. */
  fader: number
  /** -1 (left) .. 1 (right). */
  pan: number
  muted: boolean
  soloed: boolean
  /** Name of the loaded audio file; not decoded yet. */
  fileName?: string
}

export const TRACK_COLORS = ['#4f8cff', '#ff7a45', '#36c08e', '#c26bff', '#ffbf3c', '#ff5c8a']

/** Fader position of unity gain (0 dB). */
export const FADER_UNITY = 0.8
/** What the reset button restores. Name and colour are kept. */
export const TRACK_DEFAULTS = { fader: FADER_UNITY, pan: 0, muted: false, soloed: false } satisfies Partial<TrackState>

export function isAtDefaults(t: TrackState): boolean {
  return (Object.keys(TRACK_DEFAULTS) as (keyof typeof TRACK_DEFAULTS)[]).every((k) => t[k] === TRACK_DEFAULTS[k])
}

/** Gain at the top of the fader. */
export const FADER_MAX_DB = 6

/** Chosen so the top of the fader (position 1) is exactly FADER_MAX_DB. */
const DB_PER_DECADE = FADER_MAX_DB / Math.log10(1 / FADER_UNITY)

/** Position 0 is silence, FADER_UNITY is 0 dB, 1 is +6 dB. */
export function faderToDb(p: number): number {
  if (p <= 0) return -Infinity
  return DB_PER_DECADE * Math.log10(p / FADER_UNITY)
}

export function dbToFader(db: number): number {
  return FADER_UNITY * 10 ** (db / DB_PER_DECADE)
}

/**
 * Volume break points (dB) the fader snaps to while dragging: every 1.5 dB
 * near unity where mixing happens, every 3 dB below -6, then wider steps.
 */
export const VOLUME_DETENTS = [
  -48, -36, -30, -24, -21, -18, -15, -12, -9, -6, -4.5, -3, -1.5, 0, 1.5, 3, 4.5, 6,
].map(dbToFader)

/** Pan break points: 25L, centre, 25R (the ends are the slider limits). */
export const PAN_DETENTS = [-0.5, 0, 0.5]

export function formatDb(db: number): string {
  if (!Number.isFinite(db) || db < -60) return '-∞'
  const r = Math.round(db * 10) / 10
  return `${r > 0 ? '+' : ''}${r.toFixed(1)}`
}

export function formatPan(pan: number): string {
  const v = Math.round(pan * 50)
  if (v === 0) return '0'
  return v < 0 ? `${-v}L` : `${v}R`
}

/** Lowest volume shown as a number; anything below is silence (-∞). */
export const MIN_DB = -60

/** Volume step for nudging a typed value: arrows 1 dB, Shift+arrows 0.1 dB. */
export const VOLUME_NUDGE = { normal: 1, fine: 0.1 }
/** Pan step in readout units (0..50 each side): arrows 5, Shift+arrows 1. */
export const PAN_NUDGE = { normal: 5, fine: 1 }

/** Nudge a dB value; stepping up from silence starts at MIN_DB, below MIN_DB is silence. */
export function nudgeDb(db: number, delta: number): number {
  if (!Number.isFinite(db) && delta < 0) return -Infinity
  const from = Number.isFinite(db) ? Math.round(db * 10) / 10 : MIN_DB - delta
  const next = Math.min(FADER_MAX_DB, Math.round((from + delta) * 10) / 10)
  return next < MIN_DB ? -Infinity : next
}

/** Parse typed volume: "-6", "-6 dB", "+3", "-inf", "-∞". Null if unreadable. */
export function parseDb(text: string): number | null {
  const s = text.trim().toLowerCase().replace(/\s*db$/, '')
  if (s === '-inf' || s === '-∞' || s === 'inf' || s === '∞') return -Infinity
  const n = Number(s)
  if (s === '' || !Number.isFinite(n)) return null
  if (n < MIN_DB) return -Infinity
  return Math.min(FADER_MAX_DB, n)
}

export function dbToFaderClamped(db: number): number {
  return Number.isFinite(db) ? Math.min(1, dbToFader(db)) : 0
}

/** Nudge pan by readout units (50 = fully left/right). */
export function nudgePan(pan: number, delta: number): number {
  const units = Math.round(pan * 50) + delta
  return Math.max(-50, Math.min(50, units)) / 50
}

/** Parse typed pan: "0", "c", "center", "20L", "L20", "20 r", "-20" (left), "+20" (right). */
export function parsePan(text: string): number | null {
  const s = text.trim().toLowerCase().replace(/\s+/g, '')
  if (s === 'c' || s === 'center' || s === 'centre') return 0
  let m = s.match(/^(\d+(?:\.\d+)?)([lr])$/) ?? s.match(/^([lr])(\d+(?:\.\d+)?)$/)
  let units: number
  if (m) {
    const [num, side] = /\d/.test(m[1]) ? [m[1], m[2]] : [m[2], m[1]]
    units = Number(num) * (side === 'l' ? -1 : 1)
  } else {
    m = s.match(/^[+-]?\d+(?:\.\d+)?$/)
    if (!m) return null
    units = Number(s)
  }
  return Math.max(-50, Math.min(50, Math.round(units))) / 50
}
