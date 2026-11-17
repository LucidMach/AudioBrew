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
  armed: boolean
}

export const TRACK_COLORS = ['#4f8cff', '#ff7a45', '#36c08e', '#c26bff', '#ffbf3c', '#ff5c8a']

/** Fader position 0.8 is unity gain; 1.0 is about +6 dB. */
export function faderToDb(p: number): number {
  if (p <= 0) return -Infinity
  return 60 * Math.log10(p / 0.8)
}

export function formatDb(db: number): string {
  if (!Number.isFinite(db) || db < -60) return '-∞'
  const r = Math.round(db * 10) / 10
  return `${r > 0 ? '+' : ''}${r.toFixed(1)}`
}

export function formatPan(pan: number): string {
  const v = Math.round(pan * 50)
  if (v === 0) return 'C'
  return v < 0 ? `${-v}L` : `${v}R`
}
