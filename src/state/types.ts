/** Core data model — mirrors Bitwig's track / clip / scene / device concepts. */

export type TrackKind = 'instrument' | 'audio' | 'effect' | 'group' | 'master'

export interface Note {
  id: string
  /** MIDI pitch 0-127 */
  pitch: number
  /** start time in beats, relative to clip start */
  start: number
  /** length in beats */
  length: number
  /** velocity 0-127 */
  velocity: number
}

export interface Clip {
  id: string
  name: string
  color: string
  /** length in beats */
  length: number
  notes: Note[]
  /** alias source clip id — Bitwig 6 clip aliases share a fingerprint */
  aliasOf?: string
}

/** A slot in the launcher grid: one cell per (track, scene). */
export interface LauncherSlot {
  clip: Clip | null
}

export type DeviceKind =
  | 'polymer'      // built-in synth instrument
  | 'filter'
  | 'delay'
  | 'reverb'
  | 'eq'
  | 'drive'

export type ModSource = 'lfo' | 'envFollow'

export interface Modulation {
  id: string
  source: ModSource
  /** device id + param key this modulator targets */
  targetDeviceId: string
  targetParam: string
  /** modulation depth, bipolar -1..1 */
  amount: number
}

export interface Device {
  id: string
  kind: DeviceKind
  name: string
  enabled: boolean
  /** flat parameter bag, meaning depends on kind */
  params: Record<string, number>
  /** modulators living on this device (Bitwig: modulators attach per-device) */
  modulators: Modulator[]
}

export interface Modulator {
  id: string
  source: ModSource
  /** params for the modulator itself (e.g. lfo rate) */
  params: Record<string, number>
  routes: ModRoute[]
}

export interface ModRoute {
  id: string
  targetDeviceId: string
  targetParam: string
  amount: number // bipolar -1..1
}

export interface Track {
  id: string
  name: string
  kind: TrackKind
  color: string
  volume: number // 0..1 (linear gain mapping handled in engine)
  pan: number    // -1..1
  mute: boolean
  solo: boolean
  armed: boolean
  devices: Device[]
  /** launcher slots, indexed parallel to scenes array */
  slots: LauncherSlot[]
  /** clips placed on the arranger timeline */
  arrangerClips: ArrangerClip[]
}

export interface ArrangerClip extends Clip {
  /** start position on the timeline, in beats */
  startBeat: number
}

export interface Scene {
  id: string
  name: string
  color: string
}

export interface KeySignature {
  root: number // 0..11, C=0
  scale: ScaleName
}

export type ScaleName =
  | 'major'
  | 'minor'
  | 'dorian'
  | 'phrygian'
  | 'lydian'
  | 'mixolydian'
  | 'locrian'
  | 'harmonicMinor'
  | 'chromatic'

export type View = 'arrange' | 'mix'
