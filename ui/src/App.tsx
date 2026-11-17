import { useState } from 'react'
import { Lane } from './components/Lane'
import { Ruler } from './components/Ruler'
import { TrackHeader } from './components/TrackHeader'
import { TRACK_COLORS, type TrackState } from './types'

const BARS = 64
const BAR_WIDTH = 96

function newTrack(id: number): TrackState {
  return {
    id,
    name: `Track ${id}`,
    color: TRACK_COLORS[(id - 1) % TRACK_COLORS.length],
    fader: 0.8,
    pan: 0,
    muted: false,
    soloed: false,
    armed: false,
  }
}

export default function App() {
  const [tracks, setTracks] = useState<TrackState[]>(() => [newTrack(1), newTrack(2)])
  const [selectedId, setSelectedId] = useState(1)
  const anySolo = tracks.some((t) => t.soloed)

  const update = (id: number, patch: Partial<TrackState>) =>
    setTracks((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)))

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
              />
              <Lane selected={selected} inactive={track.muted || silencedBySolo} width={BARS * BAR_WIDTH} />
            </div>
          )
        })}
      </div>
    </main>
  )
}
