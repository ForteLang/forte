import { useEffect } from 'react'
import { useStore } from '../state/store'
import { engine } from '../audio/AudioEngine'
import { useRaf } from './useRaf'
import { TransportBar } from './TransportBar'
import { Inspector } from './Inspector'
import { Browser } from './Browser'
import { ClipLauncher } from './ClipLauncher'
import { Mixer } from './Mixer'
import { DevicePanel } from './DevicePanel'
import { PianoRoll } from './PianoRoll'

// computer-keyboard → MIDI mapping (Bitwig-style, starting at C)
const KEY_MAP: Record<string, number> = {
  a: 60, w: 61, s: 62, e: 63, d: 64, f: 65, t: 66,
  g: 67, y: 68, h: 69, u: 70, j: 71, k: 72, o: 73, l: 74, p: 75,
}

export function App() {
  const view = useStore((s) => s.view)
  const editing = useStore((s) => s.editing)
  const selectedTrackId = useStore((s) => s.selectedTrackId)
  const setBeat = useStore((s) => s.setBeat)

  // sync engine transport position → store for playheads
  useRaf(() => {
    if (engine.playing) setBeat(engine.currentBeat())
  })

  // resume audio context on first user gesture
  useEffect(() => {
    const resume = () => engine.resume()
    window.addEventListener('pointerdown', resume, { once: true })
    window.addEventListener('keydown', resume, { once: true })
    return () => {
      window.removeEventListener('pointerdown', resume)
      window.removeEventListener('keydown', resume)
    }
  }, [])

  // computer keyboard MIDI input
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.repeat) return
      const tag = (e.target as HTMLElement).tagName
      if (tag === 'INPUT' || tag === 'SELECT') return
      if (e.code === 'Space') {
        e.preventDefault()
        useStore.getState().togglePlay()
        return
      }
      const pitch = KEY_MAP[e.key.toLowerCase()]
      if (pitch != null && selectedTrackId) {
        engine.noteOn(selectedTrackId, pitch, 100)
      }
    }
    const up = (e: KeyboardEvent) => {
      const pitch = KEY_MAP[e.key.toLowerCase()]
      if (pitch != null && selectedTrackId) engine.noteOff(selectedTrackId, pitch)
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
    }
  }, [selectedTrackId])

  return (
    <div className="app">
      <TransportBar />
      <div className="main-area">
        <Inspector />
        <div className="center">
          <div className="workspace">
            {view === 'arrange' ? <ClipLauncher /> : <Mixer />}
          </div>
          {editing ? <PianoRoll /> : <DevicePanel />}
        </div>
        <Browser />
      </div>
    </div>
  )
}
