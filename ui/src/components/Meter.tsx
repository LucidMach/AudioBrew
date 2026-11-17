type Props = {
  /** Peak level per channel, 0..1 (linear, already mapped to the meter scale). */
  left: number
  right: number
}

/** Stereo peak meter: two thin vertical bars. Silent until audio is wired up. */
export function Meter({ left, right }: Props) {
  return (
    <div className="meter" aria-hidden>
      <div className="meter-bar">
        <div className="meter-level" style={{ height: `${left * 100}%` }} />
      </div>
      <div className="meter-bar">
        <div className="meter-level" style={{ height: `${right * 100}%` }} />
      </div>
    </div>
  )
}
