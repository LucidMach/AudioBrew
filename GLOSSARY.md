# audioBrew

A desktop app for making audio mashups: the user loads audio files, aligns and tempo-matches them, shapes each one by frequency range, and hears the combined result.

## Language

**Track**:
One loaded audio file, shown as a lane with its own waveform. At least two Track lanes are always visible; a lane may be empty.
_Avoid_: Channel, stem, layer, source

**Channel**:
A stereo left or right lane inside an audio signal. Never used to mean a Track.
_Avoid_: Using it for Track

**Mashup**:
The combined output of all Tracks after their tempo, offset, loop and Band adjustments, shown as its own lane beneath the Tracks.
_Avoid_: Mix, master, output

**Focus**:
The Track the user has selected, which reveals its Bands.
_Avoid_: Selection, active track

**Band**:
A frequency range of a Track with its own gain, mute and solo. A Track has Low, Mid and High Bands by default; the user can change their frequency ranges and add or remove Bands.
_Avoid_: Channel, filter, EQ band

**Mashup BPM**:
The single tempo every Track is stretched to. Defaults to the first loaded Track's tempo and can be edited.
_Avoid_: Project tempo, master tempo

**Beat grid**:
A Track's detected beats and downbeat. The user can correct its tempo (halve, double, type in) and drag its downbeat.
_Avoid_: Click track, tempo map

**Offset**:
How far a Track's start is shifted along the timeline relative to the start of the Mashup.
_Avoid_: Delay, shift

**Loop region**:
A beat-grid-snapped span of a Track that repeats to fill the Mashup.
_Avoid_: Loop, sample

**End marker**:
The draggable point that sets where the Mashup ends. It defaults to where the longest Track ends, and repeating Tracks repeat until it.
_Avoid_: Trim point, mashup length

**Playhead**:
The single shared position across all lanes.
_Avoid_: Cursor, scrubber

**Key badge**:
A colour-coded indicator on a Track showing whether its detected musical key is compatible with the first Track's.
_Avoid_: Key tag

**Fader**:
A Track's overall volume control, separate from its Band gains.
_Avoid_: Volume knob

**UI labels**:
The interface shows plain words for some terms: Mashup BPM is "Speed", Offset is "Nudge", Loop region is "Repeat". The glossary terms remain canonical in code.

**Solo audition**:
Playing one lane alone, from the shared Playhead, with the same tempo, Offset, loop and Band processing it has in the Mashup.
_Avoid_: Preview, solo (which means a Band's solo)
