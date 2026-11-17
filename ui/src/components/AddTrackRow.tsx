import { PlusIcon } from './Icons'

/** Full-width row under the last track: adds a new empty track. Shown only below the track limit. */
export function AddTrackRow({ onAdd, max }: { onAdd: () => void; max: number }) {
  return (
    <div className="add-track-row">
      <button
        className="add-track"
        data-tip={`Add track\nAdds an empty track below the last one (up to ${max})\nTracks you add can be deleted again`}
        onClick={onAdd}
      >
        <PlusIcon />
        Add track
      </button>
    </div>
  )
}
