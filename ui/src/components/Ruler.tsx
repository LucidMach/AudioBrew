type Props = {
  bars: number
  barWidth: number
}

/** Bar/beat ruler (4/4). Bar numbers on every bar, beat ticks in between. */
export function Ruler({ bars, barWidth }: Props) {
  return (
    <div className="ruler" style={{ width: bars * barWidth }}>
      {Array.from({ length: bars }, (_, i) => (
        <div key={i} className="ruler-bar" style={{ left: i * barWidth, width: barWidth }}>
          <span className="ruler-label">{i + 1}</span>
        </div>
      ))}
    </div>
  )
}
