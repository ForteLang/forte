import { useStore } from '../state/store'
import { engine } from '../audio/AudioEngine'
import { useRaf } from './useRaf'
import { useState } from 'react'
import type { Track, Clip } from '../state/types'

function MiniNotes({ clip }: { clip: Clip }) {
  if (clip.notes.length === 0) return null
  const pitches = clip.notes.map((n) => n.pitch)
  const lo = Math.min(...pitches)
  const hi = Math.max(...pitches) + 1
  const range = Math.max(1, hi - lo)
  return (
    <div className="mini-notes">
      {clip.notes.map((n) => (
        <i
          key={n.id}
          style={{
            left: `${(n.start / clip.length) * 100}%`,
            width: `${Math.max(3, (n.length / clip.length) * 100)}%`,
            bottom: `${((n.pitch - lo) / range) * 100}%`,
          }}
        />
      ))}
    </div>
  )
}

function ClipCell({ track, scene }: { track: Track; scene: number }) {
  const clip = track.slots[scene]?.clip
  const triggerSlot = useStore((s) => s.triggerSlot)
  const createClip = useStore((s) => s.createClip)
  const openEditor = useStore((s) => s.openEditor)
  const clearSlot = useStore((s) => s.clearSlot)
  const selectTrack = useStore((s) => s.selectTrack)
  const [active, setActive] = useState(false)

  useRaf(() => {
    const isActive = engine.getActiveScene(track.id) === scene && engine.playing
    if (isActive !== active) setActive(isActive)
  })

  if (!clip) {
    return (
      <div className="clip-cell">
        <div
          className="clip-empty"
          onClick={() => { selectTrack(track.id); createClip(track.id, scene) }}
        />
      </div>
    )
  }

  return (
    <div className="clip-cell">
      <div
        className={`clip${active ? ' playing' : ''}`}
        style={{ background: clip.color }}
        onClick={() => { selectTrack(track.id); triggerSlot(track.id, scene) }}
        onDoubleClick={() => openEditor(track.id, scene)}
        onContextMenu={(e) => { e.preventDefault(); clearSlot(track.id, scene) }}
        title="Click: launch · Double-click: edit · Right-click: delete"
      >
        <div className="cname">{clip.name}</div>
        <div className="play-tri">{active ? '▶ playing' : '▷'}</div>
        <MiniNotes clip={clip} />
      </div>
    </div>
  )
}

function TrackColumn({ track, selected }: { track: Track; selected: boolean }) {
  const scenes = useStore((s) => s.scenes)
  const selectTrack = useStore((s) => s.selectTrack)
  const setTrackProp = useStore((s) => s.setTrackProp)
  const stopTrack = useStore((s) => s.stopTrack)

  return (
    <div className="track-col">
      <div
        className={`track-head${selected ? ' sel' : ''}`}
        onClick={() => selectTrack(track.id)}
      >
        <div className="colorbar" style={{ background: track.color }} />
        <div className="tname">{track.name}</div>
        <div className="mini-mix">
          <button
            className={`mbtn m${track.mute ? ' on' : ''}`}
            onClick={(e) => { e.stopPropagation(); setTrackProp(track.id, { mute: !track.mute }) }}
          >M</button>
          <button
            className={`mbtn s${track.solo ? ' on' : ''}`}
            onClick={(e) => { e.stopPropagation(); setTrackProp(track.id, { solo: !track.solo }) }}
          >S</button>
          <button
            className="mbtn"
            onClick={(e) => { e.stopPropagation(); stopTrack(track.id) }}
            title="Stop track"
          >■</button>
        </div>
      </div>
      {scenes.map((_, i) => (
        <ClipCell key={i} track={track} scene={i} />
      ))}
    </div>
  )
}

export function ClipLauncher() {
  const tracks = useStore((s) => s.tracks)
  const scenes = useStore((s) => s.scenes)
  const selectedTrackId = useStore((s) => s.selectedTrackId)
  const triggerScene = useStore((s) => s.triggerScene)
  const addTrack = useStore((s) => s.addTrack)

  return (
    <div className="launcher-wrap">
      <div className="track-grid">
        <div className="scene-col">
          <div className="corner" />
          {scenes.map((sc, i) => (
            <div key={sc.id} className="scene-cell" onClick={() => triggerScene(i)} title={`Launch ${sc.name}`}>
              ▶ {i + 1}
            </div>
          ))}
        </div>

        {tracks.map((t) => (
          <TrackColumn key={t.id} track={t} selected={t.id === selectedTrackId} />
        ))}

        <div className="add-track-col">
          <button onClick={() => addTrack('instrument')} title="Add instrument track">+</button>
        </div>
      </div>
    </div>
  )
}
