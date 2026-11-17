import { useRef, useState } from 'react'

type Props = {
  selected: boolean
  inactive: boolean
  width: number
  fileName?: string
  onFile: (file: File) => void
}

const AUDIO_EXTENSIONS = /\.(wav|aiff?|mp3|m4a|aac|flac|ogg|opus|webm)$/i

function isAudio(file: File) {
  return file.type.startsWith('audio/') || AUDIO_EXTENSIONS.test(file.name)
}

/** The timeline area of a track: click to choose an audio file, or drop one on it. */
export function Lane({ selected, inactive, width, fileName, onFile }: Props) {
  const input = useRef<HTMLInputElement>(null)
  const [dragOver, setDragOver] = useState(false)

  return (
    <div
      className={`lane${selected ? ' is-selected' : ''}${inactive ? ' is-inactive' : ''}${dragOver ? ' is-drag-over' : ''}`}
      style={{ width }}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes('Files')) return
        e.preventDefault()
        e.dataTransfer.dropEffect = 'copy'
        setDragOver(true)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node)) setDragOver(false)
      }}
      onDrop={(e) => {
        e.preventDefault()
        setDragOver(false)
        const file = Array.from(e.dataTransfer.files).find(isAudio)
        if (file) onFile(file)
      }}
    >
      <div className="lane-viewport">
        {fileName ? (
          <span className="lane-file">{fileName}</span>
        ) : (
          <button
            className="lane-drop"
            data-tip={'Load audio into this track\nClick to browse your files\nOr drag an audio file from your computer onto this track\nWAV, AIFF, MP3, M4A, FLAC, OGG'}
            onClick={() => input.current?.click()}
          >
            {dragOver ? 'Drop to load' : 'Click to choose an audio file, or drag and drop one here'}
          </button>
        )}
      </div>
      <input
        ref={input}
        type="file"
        accept="audio/*,.wav,.aif,.aiff,.mp3,.m4a,.aac,.flac,.ogg,.opus"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) onFile(file)
          e.target.value = ''
        }}
      />
    </div>
  )
}
