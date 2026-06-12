import { create } from 'zustand'
import type {
  Track, Scene, Device, DeviceKind, Note, Clip, ArrangerClip,
  KeySignature, ScaleName, View, TrackKind, Modulator, ModSource,
} from './types'
import { createDevice, uid } from './devices'
import { engine } from '../audio/AudioEngine'
import { pickTrackColor } from '../theme/theme'

const SCENE_COUNT = 8

function emptySlots(n: number) {
  return Array.from({ length: n }, () => ({ clip: null as Clip | null }))
}

function makeNotes(pattern: [number, number, number][], len: number): Note[] {
  // pattern entries: [pitch, startBeat, lengthBeats]
  return pattern.map(([pitch, start, length]) => ({
    id: uid('n'), pitch, start, length, velocity: 100,
  }))
}

function demoClip(name: string, color: string, pattern: [number, number, number][], len: number): Clip {
  return { id: uid('clip'), name, color, length: len, notes: makeNotes(pattern, len) }
}

function makeTrack(name: string, kind: TrackKind, color: string, withInstrument: boolean): Track {
  const devices: Device[] = []
  if (withInstrument) devices.push(createDevice('polymer'))
  return {
    id: uid('trk'),
    name, kind, color,
    volume: 0.8, pan: 0, mute: false, solo: false, armed: false,
    devices,
    slots: emptySlots(SCENE_COUNT),
    arrangerClips: [],
  }
}

function defaultProject() {
  const scenes: Scene[] = Array.from({ length: SCENE_COUNT }, (_, i) => ({
    id: uid('scene'), name: `Scene ${i + 1}`, color: '#3a3a3a',
  }))

  const bass = makeTrack('Bass', 'instrument', pickTrackColor(5), true)
  bass.devices[0].params.wave = 1
  bass.devices[0].params.cutoff = 0.45
  bass.slots[0].clip = demoClip('Bass A', bass.color, [
    [36, 0, 0.9], [36, 1, 0.9], [43, 2, 0.9], [41, 3, 0.9],
  ], 4)

  const keys = makeTrack('Keys', 'instrument', pickTrackColor(6), true)
  keys.devices[0].params.wave = 2
  keys.devices.push(createDevice('reverb'))
  keys.slots[0].clip = demoClip('Chords', keys.color, [
    [60, 0, 2], [64, 0, 2], [67, 0, 2],
    [62, 2, 2], [65, 2, 2], [69, 2, 2],
  ], 4)

  const lead = makeTrack('Lead', 'instrument', pickTrackColor(1), true)
  lead.devices[0].params.wave = 1
  lead.devices.push(createDevice('delay'))
  lead.slots[1].clip = demoClip('Riff', lead.color, [
    [72, 0, 0.5], [75, 0.5, 0.5], [79, 1, 0.5], [72, 1.5, 0.5],
    [74, 2, 0.5], [77, 2.5, 0.5], [81, 3, 1],
  ], 4)

  const drums = makeTrack('Drums', 'instrument', pickTrackColor(0), true)
  drums.devices[0].params.wave = 0
  drums.devices[0].params.attack = 0
  drums.devices[0].params.decay = 0.12
  drums.devices[0].params.sustain = 0
  drums.devices[0].params.release = 0.05
  drums.slots[0].clip = demoClip('Beat', drums.color, [
    [36, 0, 0.2], [36, 1, 0.2], [36, 2, 0.2], [36, 3, 0.2],
  ], 4)

  const tracks = [drums, bass, keys, lead]
  return { tracks, scenes }
}

interface StoreState {
  tracks: Track[]
  scenes: Scene[]
  bpm: number
  timeSig: [number, number]
  playing: boolean
  metronome: boolean
  view: View
  beat: number
  key: KeySignature

  selectedTrackId: string | null
  selectedDeviceId: string | null
  /** clip currently open in the detail editor: track + scene (launcher) */
  editing: { trackId: string; scene: number } | null

  // transport
  togglePlay: () => void
  stop: () => void
  setBpm: (bpm: number) => void
  toggleMetronome: () => void
  setView: (v: View) => void
  setBeat: (b: number) => void

  // key signature (Bitwig 6)
  setKeyRoot: (root: number) => void
  setScale: (s: ScaleName) => void

  // tracks
  addTrack: (kind?: TrackKind) => void
  removeTrack: (id: string) => void
  selectTrack: (id: string) => void
  renameTrack: (id: string, name: string) => void
  setTrackProp: (id: string, prop: Partial<Pick<Track, 'volume' | 'pan' | 'mute' | 'solo' | 'armed'>>) => void

  // devices
  addDevice: (trackId: string, kind: DeviceKind) => void
  removeDevice: (trackId: string, deviceId: string) => void
  toggleDevice: (trackId: string, deviceId: string) => void
  selectDevice: (deviceId: string | null) => void
  setParam: (trackId: string, deviceId: string, key: string, value: number) => void

  // modulators
  addModulator: (trackId: string, deviceId: string, source: ModSource) => void
  setModRoute: (trackId: string, deviceId: string, modId: string, targetParam: string, amount: number) => void

  // clips / launcher
  createClip: (trackId: string, scene: number) => void
  clearSlot: (trackId: string, scene: number) => void
  openEditor: (trackId: string, scene: number) => void
  closeEditor: () => void
  updateNotes: (trackId: string, scene: number, notes: Note[]) => void

  triggerSlot: (trackId: string, scene: number) => void
  stopTrack: (trackId: string) => void
  triggerScene: (scene: number) => void
}

function sync(tracks: Track[]) {
  engine.syncTracks(tracks)
}

const init = defaultProject()

export const useStore = create<StoreState>((set, get) => ({
  tracks: init.tracks,
  scenes: init.scenes,
  bpm: 120,
  timeSig: [4, 4],
  playing: false,
  metronome: false,
  view: 'arrange',
  beat: 0,
  key: { root: 0, scale: 'minor' },

  selectedTrackId: init.tracks[0]?.id ?? null,
  selectedDeviceId: init.tracks[0]?.devices[0]?.id ?? null,
  editing: null,

  togglePlay: () => {
    const playing = get().playing
    if (playing) {
      engine.stop()
      set({ playing: false })
    } else {
      sync(get().tracks)
      engine.setBpm(get().bpm)
      engine.play(0, 'launcher')
      set({ playing: true })
    }
  },
  stop: () => {
    engine.stop()
    set({ playing: false, beat: 0 })
  },
  setBpm: (bpm) => {
    const v = Math.max(20, Math.min(300, bpm))
    engine.setBpm(v)
    set({ bpm: v })
  },
  toggleMetronome: () => set((s) => ({ metronome: !s.metronome })),
  setView: (view) => set({ view }),
  setBeat: (beat) => set({ beat }),

  setKeyRoot: (root) => set((s) => ({ key: { ...s.key, root } })),
  setScale: (scale) => set((s) => ({ key: { ...s.key, scale } })),

  addTrack: (kind = 'instrument') => set((s) => {
    const color = pickTrackColor(s.tracks.length)
    const t = makeTrack(`Track ${s.tracks.length + 1}`, kind, color, kind === 'instrument')
    const tracks = [...s.tracks, t]
    sync(tracks)
    return { tracks, selectedTrackId: t.id, selectedDeviceId: t.devices[0]?.id ?? null }
  }),
  removeTrack: (id) => set((s) => {
    const tracks = s.tracks.filter((t) => t.id !== id)
    sync(tracks)
    return {
      tracks,
      selectedTrackId: s.selectedTrackId === id ? tracks[0]?.id ?? null : s.selectedTrackId,
    }
  }),
  selectTrack: (id) => set((s) => {
    const t = s.tracks.find((x) => x.id === id)
    return { selectedTrackId: id, selectedDeviceId: t?.devices[0]?.id ?? null }
  }),
  renameTrack: (id, name) => set((s) => {
    const tracks = s.tracks.map((t) => (t.id === id ? { ...t, name } : t))
    return { tracks }
  }),
  setTrackProp: (id, prop) => set((s) => {
    const tracks = s.tracks.map((t) => (t.id === id ? { ...t, ...prop } : t))
    sync(tracks)
    return { tracks }
  }),

  addDevice: (trackId, kind) => set((s) => {
    const dev = createDevice(kind)
    const tracks = s.tracks.map((t) =>
      t.id === trackId ? { ...t, devices: [...t.devices, dev] } : t)
    sync(tracks)
    return { tracks, selectedDeviceId: dev.id }
  }),
  removeDevice: (trackId, deviceId) => set((s) => {
    const tracks = s.tracks.map((t) =>
      t.id === trackId ? { ...t, devices: t.devices.filter((d) => d.id !== deviceId) } : t)
    sync(tracks)
    return { tracks }
  }),
  toggleDevice: (trackId, deviceId) => set((s) => {
    const tracks = s.tracks.map((t) =>
      t.id === trackId
        ? { ...t, devices: t.devices.map((d) => (d.id === deviceId ? { ...d, enabled: !d.enabled } : d)) }
        : t)
    sync(tracks)
    return { tracks }
  }),
  selectDevice: (deviceId) => set({ selectedDeviceId: deviceId }),
  setParam: (trackId, deviceId, key, value) => set((s) => {
    const tracks = s.tracks.map((t) =>
      t.id === trackId
        ? { ...t, devices: t.devices.map((d) =>
            d.id === deviceId ? { ...d, params: { ...d.params, [key]: value } } : d) }
        : t)
    sync(tracks)
    return { tracks }
  }),

  addModulator: (trackId, deviceId, source) => set((s) => {
    const mod: Modulator = {
      id: uid('mod'), source,
      params: source === 'lfo' ? { rate: 0.3, shape: 0 } : { sens: 0.5 },
      routes: [],
    }
    const tracks = s.tracks.map((t) =>
      t.id === trackId
        ? { ...t, devices: t.devices.map((d) =>
            d.id === deviceId ? { ...d, modulators: [...d.modulators, mod] } : d) }
        : t)
    sync(tracks)
    return { tracks }
  }),
  setModRoute: (trackId, deviceId, modId, targetParam, amount) => set((s) => {
    const tracks = s.tracks.map((t) => {
      if (t.id !== trackId) return t
      return {
        ...t,
        devices: t.devices.map((d) => {
          if (d.id !== deviceId) return d
          return {
            ...d,
            modulators: d.modulators.map((m) => {
              if (m.id !== modId) return m
              const existing = m.routes.find((r) => r.targetParam === targetParam)
              let routes
              if (Math.abs(amount) < 0.001) {
                routes = m.routes.filter((r) => r.targetParam !== targetParam)
              } else if (existing) {
                routes = m.routes.map((r) => r.targetParam === targetParam ? { ...r, amount } : r)
              } else {
                routes = [...m.routes, { id: uid('route'), targetDeviceId: deviceId, targetParam, amount }]
              }
              return { ...m, routes }
            }),
          }
        }),
      }
    })
    sync(tracks)
    return { tracks }
  }),

  createClip: (trackId, scene) => set((s) => {
    const t = s.tracks.find((x) => x.id === trackId)!
    const clip: Clip = {
      id: uid('clip'), name: `Clip`, color: t.color, length: 4, notes: [],
    }
    const tracks = s.tracks.map((tr) =>
      tr.id === trackId
        ? { ...tr, slots: tr.slots.map((sl, i) => (i === scene ? { clip } : sl)) }
        : tr)
    sync(tracks)
    return { tracks, editing: { trackId, scene } }
  }),
  clearSlot: (trackId, scene) => set((s) => {
    const tracks = s.tracks.map((tr) =>
      tr.id === trackId
        ? { ...tr, slots: tr.slots.map((sl, i) => (i === scene ? { clip: null } : sl)) }
        : tr)
    sync(tracks)
    return { tracks }
  }),
  openEditor: (trackId, scene) => set({ editing: { trackId, scene } }),
  closeEditor: () => set({ editing: null }),
  updateNotes: (trackId, scene, notes) => set((s) => {
    const tracks = s.tracks.map((tr) => {
      if (tr.id !== trackId) return tr
      return {
        ...tr,
        slots: tr.slots.map((sl, i) =>
          i === scene && sl.clip ? { clip: { ...sl.clip, notes } } : sl),
      }
    })
    sync(tracks)
    return { tracks }
  }),

  triggerSlot: (trackId, scene) => {
    if (!get().playing) {
      sync(get().tracks)
      engine.setBpm(get().bpm)
      set({ playing: true })
    }
    engine.triggerSlot(trackId, scene)
  },
  stopTrack: (trackId) => engine.stopSlot(trackId),
  triggerScene: (scene) => {
    if (!get().playing) {
      sync(get().tracks)
      engine.setBpm(get().bpm)
      set({ playing: true })
    }
    engine.triggerScene(scene)
  },
}))

// keep engine graph in sync on first load
sync(init.tracks)
