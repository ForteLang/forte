import type { Device, DeviceKind } from './types'
import { Polymer } from '../audio/Polymer'

let counter = 0
export function uid(prefix = 'id'): string {
  counter += 1
  return `${prefix}_${Date.now().toString(36)}_${counter}`
}

const FX_DEFAULTS: Record<Exclude<DeviceKind, 'polymer'>, Record<string, number>> = {
  filter: { type: 0, cutoff: 0.6, reso: 0.2 },
  delay: { time: 0.3, feedback: 0.35, mix: 0.3 },
  reverb: { size: 0.5, decay: 0.5, mix: 0.25 },
  eq: { low: 0.5, mid: 0.5, high: 0.5 },
  drive: { drive: 0.3 },
}

export const DEVICE_LABELS: Record<DeviceKind, string> = {
  polymer: 'Polymer',
  filter: 'Filter+',
  delay: 'Delay-4',
  reverb: 'Reverb',
  eq: 'EQ-5',
  drive: 'Distortion',
}

export function createDevice(kind: DeviceKind): Device {
  const params =
    kind === 'polymer' ? Polymer.defaults() : { ...FX_DEFAULTS[kind] }
  return {
    id: uid('dev'),
    kind,
    name: DEVICE_LABELS[kind],
    enabled: true,
    params,
    modulators: [],
  }
}

/** Parameter metadata for rendering knobs in the device panel. */
export const PARAM_META: Record<DeviceKind, { key: string; label: string; type?: 'select'; options?: string[] }[]> = {
  polymer: [
    { key: 'wave', label: 'Wave', type: 'select', options: ['Sine', 'Saw', 'Square', 'Tri'] },
    { key: 'cutoff', label: 'Cutoff' },
    { key: 'reso', label: 'Reso' },
    { key: 'attack', label: 'Attack' },
    { key: 'decay', label: 'Decay' },
    { key: 'sustain', label: 'Sustain' },
    { key: 'release', label: 'Release' },
    { key: 'detune', label: 'Detune' },
    { key: 'subOsc', label: 'Sub' },
  ],
  filter: [
    { key: 'type', label: 'Type', type: 'select', options: ['LP', 'HP', 'BP', 'Notch'] },
    { key: 'cutoff', label: 'Cutoff' },
    { key: 'reso', label: 'Reso' },
  ],
  delay: [
    { key: 'time', label: 'Time' },
    { key: 'feedback', label: 'Fdbk' },
    { key: 'mix', label: 'Mix' },
  ],
  reverb: [
    { key: 'size', label: 'Size' },
    { key: 'decay', label: 'Decay' },
    { key: 'mix', label: 'Mix' },
  ],
  eq: [
    { key: 'low', label: 'Low' },
    { key: 'mid', label: 'Mid' },
    { key: 'high', label: 'High' },
  ],
  drive: [
    { key: 'drive', label: 'Drive' },
  ],
}
