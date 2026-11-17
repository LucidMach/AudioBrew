const base = {
  width: 14,
  height: 14,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 2,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
  'aria-hidden': true,
}

/** Speaker with a cross: mute. */
export function MuteIcon() {
  return (
    <svg {...base}>
      <path d="M11 5 6 9H3v6h3l5 4V5z" fill="currentColor" />
      <line x1="16" y1="9" x2="22" y2="15" />
      <line x1="22" y1="9" x2="16" y2="15" />
    </svg>
  )
}

/** Speaker with sound waves: volume. */
export function VolumeIcon() {
  return (
    <svg {...base}>
      <path d="M11 5 6 9H3v6h3l5 4V5z" fill="currentColor" />
      <path d="M15.5 8.5a5 5 0 0 1 0 7" />
      <path d="M18.5 5.5a9 9 0 0 1 0 13" />
    </svg>
  )
}

/**
 * Pan: a listener with sound arriving from the left and the right,
 * the usual "stereo / spatial audio" picture.
 */
export function PanIcon() {
  return (
    <svg {...base} strokeWidth={1.75}>
      <circle cx="12" cy="9.5" r="3" />
      <path d="M7.5 20a4.5 4.5 0 0 1 9 0" />
      <path d="M5.5 7.5a6 6 0 0 0 0 7" />
      <path d="M18.5 7.5a6 6 0 0 1 0 7" />
      <path d="M2.5 5a10 10 0 0 0 0 12" />
      <path d="M21.5 5a10 10 0 0 1 0 12" />
    </svg>
  )
}

/** Counter-clockwise arrow: reset. */
export function ResetIcon() {
  return (
    <svg {...base}>
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
      <path d="M3 3v5h5" />
    </svg>
  )
}

/** Bin: delete. */
export function TrashIcon() {
  return (
    <svg {...base}>
      <path d="M3 6h18" />
      <path d="M8 6V4h8v2" />
      <path d="M19 6l-1 14H6L5 6" />
      <line x1="10" y1="11" x2="10" y2="16" />
      <line x1="14" y1="11" x2="14" y2="16" />
    </svg>
  )
}

/** Plus: add. */
export function PlusIcon() {
  return (
    <svg {...base}>
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  )
}

/** Headphones: solo. */
export function HeadphonesIcon() {
  return (
    <svg {...base}>
      <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
      <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3z" fill="currentColor" />
      <path d="M3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" fill="currentColor" />
    </svg>
  )
}
