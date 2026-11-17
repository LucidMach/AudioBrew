import { useState } from 'react'
import { Lane } from './components/Lane'
import { Ruler } from './components/Ruler'
import { TooltipLayer } from './components/TooltipLayer'
import { TrackHeader } from './components/TrackHeader'
import { AddTrackRow } from './components/AddTrackRow'
import { MAX_TRACKS, MIN_TRACKS, TRACK_COLORS, TRACK_DEFAULTS, type TrackState } from './types'

const BARS = 64
const BAR_WIDTH = 96

/** A new track takes the first palette colour no other track is using. */
function newTrack(id: number, existing: TrackState[]): TrackState {
  const used = new Set(existing.map((t) => t.color))
  return {
    id,
    name: `Track ${id}`,
    color: TRACK_COLORS.find((c) => !used.has(c)) ?? TRACK_COLORS[(id - 1) % TRACK_COLORS.length],
    ...TRACK_DEFAULTS,
  }
}

export default function App() {
  const [tracks, setTracks] = useState<TrackState[]>(() => {
    const first = newTrack(1, [])
    return [first, newTrack(2, [first])]
  })
  const [selectedId, setSelectedId] = useState(1)
  const anySolo = tracks.some((t) => t.soloed)

  const update = (id: number, patch: Partial<TrackState>) =>
    setTracks((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)))

  const addTrack = () => {
    if (tracks.length >= MAX_TRACKS) return
    const id = Math.max(0, ...tracks.map((t) => t.id)) + 1
    setTracks([...tracks, newTrack(id, tracks)])
    setSelectedId(id)
  }

  /** Remove a track (never below MIN_TRACKS); selection moves to the track above. */
  const deleteTrack = (id: number) => {
    if (tracks.length <= MIN_TRACKS) return
    const i = tracks.findIndex((t) => t.id === id)
    const rest = tracks.filter((t) => t.id !== id)
    setTracks(rest)
    if (selectedId === id) setSelectedId(rest[Math.max(0, i - 1)].id)
  }

  return (
    <main className="app">
      <div
        className="arrangement"
        style={{ '--bar-width': `${BAR_WIDTH}px`, '--beat-width': `${BAR_WIDTH / 4}px` } as React.CSSProperties}
      >
        <div className="corner" />
        <Ruler bars={BARS} barWidth={BAR_WIDTH} />
        {tracks.map((track, i) => {
          const selected = track.id === selectedId
          const silencedBySolo = anySolo && !track.soloed
          return (
            <div className="track" key={track.id} onPointerDown={() => setSelectedId(track.id)}>
              <TrackHeader
                track={track}
                index={i}
                selected={selected}
                silencedBySolo={silencedBySolo}
                onSelect={() => setSelectedId(track.id)}
                onChange={(patch) => update(track.id, patch)}
                onDelete={i >= MIN_TRACKS ? () => deleteTrack(track.id) : undefined}
              />
              <Lane
                selected={selected}
                inactive={track.muted || silencedBySolo}
                width={BARS * BAR_WIDTH}
                fileName={track.fileName}
                onFile={(file) => update(track.id, { fileName: file.name })}
              />
            </div>
          )
        })}
        {tracks.length < MAX_TRACKS && <AddTrackRow onAdd={addTrack} max={MAX_TRACKS} />}
      </div>
      <TooltipLayer />
    </main>
  )
}
