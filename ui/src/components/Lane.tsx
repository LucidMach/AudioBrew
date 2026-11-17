type Props = {
  selected: boolean
  inactive: boolean
  width: number
}

/** The timeline area of a track. Empty for now: grid plus a drop hint. */
export function Lane({ selected, inactive, width }: Props) {
  return (
    <div className={`lane${selected ? ' is-selected' : ''}${inactive ? ' is-inactive' : ''}`} style={{ width }}>
      <span className="lane-hint">Drop audio here</span>
    </div>
  )
}
