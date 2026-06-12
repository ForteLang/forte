import { useRef, useState } from 'react'
import { useStore } from '../state/store'
import { engine } from '../audio/AudioEngine'
import { useRaf } from './useRaf'
import { noteName, isBlackKey, inScale } from '../state/music'
import { uid } from '../state/devices'
import type { Note } from '../state/types'

const LOW = 36          // C2
const HIGH = 84         // C6
const ROW_H = 13
const BEAT_W = 56
const GRID = 0.25       // 1/16 snap

const PITCHES: number[] = []
for (let p = HIGH; p >= LOW; p--) PITCHES.push(p)

type Drag =
  | { type: 'move'; id: string; startX: number; startY: number; origStart: number; origPitch: number }
  | { type: 'resize'; id: string; startX: number; origLen: number }
  | null

export function PianoRoll() {
  const editing = useStore((s) => s.editing)!
  const tracks = useStore((s) => s.tracks)
  const updateNotes = useStore((s) => s.updateNotes)
  const closeEditor = useStore((s) => s.closeEditor)
  const key = useStore((s) => s.key)

  const track = tracks.find((t) => t.id === editing.trackId)
  const clip = track?.slots[editing.scene]?.clip
  const keysRef = useRef<HTMLDivElement>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const drag = useRef<Drag>(null)
  const [playhead, setPlayhead] = useState(0)

  useRaf(() => {
    if (engine.playing && engine.getActiveScene(editing.trackId) === editing.scene && clip) {
      setPlayhead((engine.currentBeat() % clip.length))
    } else {
      setPlayhead(-1)
    }
  })

  if (!track || !clip) {
    return (
      <div className="pianoroll">
        <div className="pr-toolbar"><button onClick={closeEditor}>Close ✕</button></div>
        <div className="empty-state">No clip selected</div>
      </div>
    )
  }

  const beats = clip.length
  const gridWidth = beats * BEAT_W
  const gridHeight = PITCHES.length * ROW_H

  const snap = (b: number) => Math.round(b / GRID) * GRID

  const addNote = (e: React.MouseEvent) => {
    if (drag.current) return
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const x = e.clientX - rect.left + (e.currentTarget as HTMLElement).scrollLeft
    const y = e.clientY - rect.top
    const start = Math.max(0, Math.min(beats - GRID, snap(x / BEAT_W)))
    const idx = Math.floor(y / ROW_H)
    const pitch = PITCHES[Math.max(0, Math.min(PITCHES.length - 1, idx))]
    const note: Note = { id: uid('n'), pitch, start, length: 1, velocity: 100 }
    updateNotes(track.id, editing.scene, [...clip.notes, note])
    setSelected(note.id)
    engine.noteOn(track.id, pitch, 100)
    setTimeout(() => engine.noteOff(track.id, pitch), 150)
  }

  const onNoteDown = (e: React.PointerEvent, note: Note, mode: 'move' | 'resize') => {
    e.stopPropagation()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    setSelected(note.id)
    if (mode === 'move') {
      drag.current = { type: 'move', id: note.id, startX: e.clientX, startY: e.clientY, origStart: note.start, origPitch: note.pitch }
    } else {
      drag.current = { type: 'resize', id: note.id, startX: e.clientX, origLen: note.length }
    }
  }

  const onMove = (e: React.PointerEvent) => {
    const d = drag.current
    if (!d) return
    const note = clip.notes.find((n) => n.id === d.id)
    if (!note) return
    let next: Note
    if (d.type === 'move') {
      const dx = (e.clientX - d.startX) / BEAT_W
      const dy = Math.round((e.clientY - d.startY) / ROW_H)
      const start = Math.max(0, Math.min(beats - note.length, snap(d.origStart + dx)))
      const pitch = Math.max(LOW, Math.min(HIGH, d.origPitch - dy))
      next = { ...note, start, pitch }
    } else {
      const dx = (e.clientX - d.startX) / BEAT_W
      const length = Math.max(GRID, snap(d.origLen + dx))
      next = { ...note, length: Math.min(length, beats - note.start) }
    }
    updateNotes(track.id, editing.scene, clip.notes.map((n) => (n.id === d.id ? next : n)))
  }

  const onUp = () => { drag.current = null }

  const delNote = (e: React.MouseEvent, id: string) => {
    e.preventDefault()
    e.stopPropagation()
    updateNotes(track.id, editing.scene, clip.notes.filter((n) => n.id !== id))
  }

  const setLength = (len: number) => {
    useStore.setState((s) => ({
      tracks: s.tracks.map((t) => t.id === track.id ? {
        ...t, slots: t.slots.map((sl, i) => i === editing.scene && sl.clip ? { clip: { ...sl.clip, length: len } } : sl),
      } : t),
    }))
    engine.syncTracks(useStore.getState().tracks)
  }

  return (
    <div className="pianoroll">
      <div className="pr-toolbar">
        <strong>{clip.name}</strong>
        <span className="hint">{track.name}</span>
        <span className="sep" style={{ width: 1, height: 16, background: 'var(--border-soft)' }} />
        <label className="hint">Length</label>
        <select value={beats} onChange={(e) => setLength(Number(e.target.value))}>
          {[1, 2, 4, 8, 16].map((b) => <option key={b} value={b}>{b} beats</option>)}
        </select>
        <span className="hint">Click grid to add · drag to move · right-click to delete</span>
        <div style={{ flex: 1 }} />
        <button onClick={closeEditor}>Close ✕</button>
      </div>

      <div className="pr-body">
        <div className="pr-keys" ref={keysRef}>
          {PITCHES.map((p, i) => (
            <div
              key={p}
              className={`pr-key ${isBlackKey(p) ? 'black' : 'white'}${inScale(p, key.root, key.scale) ? ' inscale' : ''}`}
              style={{ top: i * ROW_H, height: ROW_H }}
            >
              {p % 12 === 0 ? noteName(p) : ''}
            </div>
          ))}
        </div>

        <div className="pr-grid-wrap" onScroll={(e) => {
          if (keysRef.current) keysRef.current.scrollTop = (e.target as HTMLElement).scrollTop
        }}>
          <div
            className="pr-grid"
            style={{ width: gridWidth, height: gridHeight }}
            onClick={addNote}
            onPointerMove={onMove}
            onPointerUp={onUp}
          >
            {PITCHES.map((p, i) => (
              <div
                key={`bg${p}`}
                className={`pr-rowbg ${isBlackKey(p) ? 'black' : ''}${inScale(p, key.root, key.scale) ? ' inscale' : ''}`}
                style={{ top: i * ROW_H, height: ROW_H }}
              />
            ))}
            {Array.from({ length: Math.floor(beats / GRID) + 1 }, (_, i) => i * GRID).map((b) => (
              <div
                key={`bl${b}`}
                className={`pr-beatline${b % 1 === 0 ? ' bar' : ''}`}
                style={{ left: b * BEAT_W, opacity: b % 1 === 0 ? 1 : 0.4 }}
              />
            ))}
            {clip.notes.map((n) => {
              const idx = HIGH - n.pitch
              return (
                <div
                  key={n.id}
                  className={`pr-note${selected === n.id ? ' sel' : ''}`}
                  style={{
                    left: n.start * BEAT_W,
                    top: idx * ROW_H + 1,
                    width: n.length * BEAT_W - 1,
                    height: ROW_H - 2,
                    background: track.color,
                  }}
                  onPointerDown={(e) => onNoteDown(e, n, 'move')}
                  onContextMenu={(e) => delNote(e, n.id)}
                >
                  <div className="resize" onPointerDown={(e) => onNoteDown(e, n, 'resize')} />
                </div>
              )
            })}
            {playhead >= 0 && (
              <div className="pr-playhead" style={{ left: playhead * BEAT_W }} />
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
