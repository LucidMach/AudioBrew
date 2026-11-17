import { useState } from 'react'
import { faderToDb, formatDb, formatPan, type TrackState } from '../types'
import { Fader } from './Fader'
import { Knob } from './Knob'
import { Meter } from './Meter'

type Props = {
  track: TrackState
  index: number
  selected: boolean
  /** True when another track is soloed and this one is not, so it is silent. */
  silencedBySolo: boolean
  onSelect: () => void
  onChange: (patch: Partial<TrackState>) => void
}

export function TrackHeader({ track, index, selected, silencedBySolo, onSelect, onChange }: Props) {
  const [editing, setEditing] = useState(false)
  const db = formatDb(faderToDb(track.fader))
  const inactive = track.muted || silencedBySolo

  return (
    <div
      className={`track-header${selected ? ' is-selected' : ''}${inactive ? ' is-inactive' : ''}`}
      style={{ '--track-color': track.color } as React.CSSProperties}
      onPointerDown={onSelect}
    >
      <div className="track-color" />
      <div className="track-controls">
        <div className="track-row">
          <span className="track-number">{index + 1}</span>
          {editing ? (
            <input
              className="track-name-input"
              autoFocus
              defaultValue={track.name}
              aria-label="Track name"
              onBlur={(e) => {
                onChange({ name: e.target.value.trim() || track.name })
                setEditing(false)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') e.currentTarget.blur()
                if (e.key === 'Escape') setEditing(false)
              }}
            />
          ) : (
            <button className="track-name" title="Double-click to rename" onDoubleClick={() => setEditing(true)}>
              {track.name}
            </button>
          )}
          <div className="track-buttons">
            <ToggleButton label="M" title="Mute" kind="mute" on={track.muted} onToggle={() => onChange({ muted: !track.muted })} />
            <ToggleButton label="S" title="Solo" kind="solo" on={track.soloed} onToggle={() => onChange({ soloed: !track.soloed })} />
            <ToggleButton label="●" title="Record arm" kind="arm" on={track.armed} onToggle={() => onChange({ armed: !track.armed })} />
          </div>
        </div>
        <div className="track-row">
          <Fader
            label="Volume"
            value={track.fader}
            defaultValue={0.8}
            valueText={`${db} dB`}
            color={track.color}
            onChange={(fader) => onChange({ fader })}
          />
          <span className="readout readout-db">{db}</span>
          <Knob label="Pan" value={track.pan} valueText={formatPan(track.pan)} onChange={(pan) => onChange({ pan })} />
          <span className="readout readout-pan">{formatPan(track.pan)}</span>
        </div>
        <div className="track-row track-row-io">
          <select className="io-select" aria-label="Input" defaultValue="none">
            <option value="none">No input</option>
            <option value="in1">Input 1</option>
            <option value="in12">Input 1/2</option>
          </select>
          <button className="chip" title="Effects">FX</button>
          <button className="chip" title="Show automation">Auto</button>
        </div>
      </div>
      <Meter left={0} right={0} />
    </div>
  )
}

function ToggleButton(props: { label: string; title: string; kind: string; on: boolean; onToggle: () => void }) {
  return (
    <button
      className={`toggle toggle-${props.kind}${props.on ? ' is-on' : ''}`}
      title={props.title}
      aria-label={props.title}
      aria-pressed={props.on}
      onClick={props.onToggle}
    >
      {props.label}
    </button>
  )
}
