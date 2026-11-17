import { useState, type ReactNode } from 'react'
import {
  FADER_UNITY,
  PAN_DETENTS,
  PAN_NUDGE,
  TRACK_DEFAULTS,
  VOLUME_DETENTS,
  VOLUME_NUDGE,
  dbToFaderClamped,
  faderToDb,
  formatDb,
  formatPan,
  isAtDefaults,
  nudgeDb,
  nudgePan,
  parseDb,
  parsePan,
  type TrackState,
} from '../types'
import { EditableValue } from './EditableValue'
import { HeadphonesIcon, MuteIcon, PanIcon, ResetIcon, TrashIcon, VolumeIcon } from './Icons'
import { Meter } from './Meter'
import { Slider } from './Slider'

type Props = {
  track: TrackState
  index: number
  selected: boolean
  /** True when another track is soloed and this one is not, so it is silent. */
  silencedBySolo: boolean
  onSelect: () => void
  onChange: (patch: Partial<TrackState>) => void
  /** Present only for tracks that may be deleted (not the first two). */
  onDelete?: () => void
}

export function TrackHeader({ track, index, selected, silencedBySolo, onSelect, onChange, onDelete }: Props) {
  const [editing, setEditing] = useState(false)
  const db = formatDb(faderToDb(track.fader))
  const inactive = track.muted || silencedBySolo
  const atDefaults = isAtDefaults(track)

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
            <button
              className="track-name"
              data-tip={`${track.name}\nDouble-click to rename`}
              onDoubleClick={() => setEditing(true)}
            >
              {track.name}
            </button>
          )}
          <button
            className="icon-button"
            aria-label="Reset track"
            aria-disabled={atDefaults}
            data-tip={
              atDefaults
                ? 'Reset track\nNothing to reset: already at 0 dB, centred, not muted or soloed'
                : 'Reset track\nVolume back to 0 dB, pan to centre, mute and solo off\nThe name and audio are kept'
            }
            onClick={() => !atDefaults && onChange(TRACK_DEFAULTS)}
          >
            <ResetIcon />
          </button>
          {onDelete && (
            <button
              className="icon-button icon-button-danger"
              aria-label="Delete track"
              data-tip={'Delete track\nRemoves this track and its audio from the mix\nThe first two tracks can\'t be deleted'}
              onClick={onDelete}
            >
              <TrashIcon />
            </button>
          )}
          <div className="track-buttons">
            <ToggleButton
              label={<MuteIcon />}
              title="Mute"
              tip={
                track.muted
                  ? 'Muted\nThis track is silent. Click to hear it again'
                  : 'Mute\nSilence this track. Click again to unmute'
              }
              kind="mute"
              on={track.muted}
              onToggle={() => onChange({ muted: !track.muted })}
            />
            <ToggleButton
              label={<HeadphonesIcon />}
              title="Solo"
              tip={
                track.soloed
                  ? 'Soloed\nOnly soloed tracks play. Click to stop soloing'
                  : 'Solo\nPlay only this track (and any other soloed tracks)\nClick again to hear everything'
              }
              kind="solo"
              on={track.soloed}
              onToggle={() => onChange({ soloed: !track.soloed })}
            />
          </div>
        </div>
        <div className="track-row">
          <span className="row-icon" data-tip={'Volume\nHow loud this track is in the mix'}>
            <VolumeIcon />
          </span>
          <Slider
            label="Volume"
            value={track.fader}
            min={0}
            max={1}
            defaultValue={FADER_UNITY}
            detents={VOLUME_DETENTS}
            valueText={`${db} dB`}
            tip={'Volume\nDrag to change. It catches on the marked points\nShift + drag: fine control, no catching\nDouble-click: back to 0 dB (original level)'}
            onChange={(fader) => onChange({ fader })}
          />
          <EditableValue
            label="Volume"
            value={faderToDb(track.fader)}
            format={formatDb}
            unit="dB"
            tip={`Volume: ${db} dB\nClick to type a value, e.g. -6 or +3 (-inf for silence)\nWhile typing: ↑ / ↓ nudge by ${VOLUME_NUDGE.normal} dB\nShift + ↑ / ↓ nudge by ${VOLUME_NUDGE.fine} dB\nEnter to set · Esc to cancel`}
            parse={parseDb}
            nudge={(v, dir, fine) => nudgeDb(v, dir * (fine ? VOLUME_NUDGE.fine : VOLUME_NUDGE.normal))}
            onChange={(v) => onChange({ fader: dbToFaderClamped(v) })}
          />
        </div>
        <div className="track-row">
          <span className="row-icon" data-tip={'Pan\nWhere this track sits between the left and right speakers'}>
            <PanIcon />
          </span>
          <span className="pan-end" data-tip="Left speaker">
            L
          </span>
          <Slider
            label="Pan"
            value={track.pan}
            min={-1}
            max={1}
            defaultValue={0}
            fillFromDefault
            detents={PAN_DETENTS}
            valueText={formatPan(track.pan)}
            tip={'Pan\nDrag toward L or R to move the sound to that speaker\nIt catches at centre and at 25L / 25R\nShift + drag: fine control, no catching\nDouble-click: back to centre'}
            onChange={(pan) => onChange({ pan })}
          />
          <span className="pan-end" data-tip="Right speaker">
            R
          </span>
          <EditableValue
            label="Pan"
            className="readout-pan"
            value={track.pan}
            format={formatPan}
            tip={`Pan: ${formatPan(track.pan)}\nClick to type a value, e.g. 20L, 15R or 0 for centre (50 is fully to one side)\nWhile typing: ↑ / ↓ nudge by ${PAN_NUDGE.normal}\nShift + ↑ / ↓ nudge by ${PAN_NUDGE.fine}\nEnter to set · Esc to cancel`}
            parse={parsePan}
            nudge={(v, dir, fine) => nudgePan(v, dir * (fine ? PAN_NUDGE.fine : PAN_NUDGE.normal))}
            onChange={(pan) => onChange({ pan })}
          />
        </div>
      </div>
      <Meter left={0} right={0} />
    </div>
  )
}

function ToggleButton(props: { label: ReactNode; title: string; tip: string; kind: string; on: boolean; onToggle: () => void }) {
  return (
    <button
      className={`toggle toggle-${props.kind}${props.on ? ' is-on' : ''}`}
      data-tip={props.tip}
      aria-label={props.title}
      aria-pressed={props.on}
      onClick={props.onToggle}
    >
      {props.label}
    </button>
  )
}
