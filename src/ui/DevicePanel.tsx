import { useStore } from '../state/store'
import { Knob } from './Knob'
import { PARAM_META } from '../state/devices'
import type { Track, Device } from '../state/types'

function ModSection({ track, device }: { track: Track; device: Device }) {
  const addModulator = useStore((s) => s.addModulator)
  const setModRoute = useStore((s) => s.setModRoute)
  const meta = PARAM_META[device.kind].filter((m) => m.type !== 'select')

  return (
    <div className="modulators">
      <div className="hint" style={{ marginBottom: 2 }}>MODULATORS</div>
      {device.modulators.map((mod) => (
        <div key={mod.id} className="mod-chip">
          <div className="mtitle">
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: 'var(--play)' }} />
            {mod.source === 'lfo' ? 'LFO' : 'Env Follow'}
          </div>
          {mod.source === 'lfo' && (
            <div className="mod-route">
              <span style={{ width: 30 }}>Rate</span>
              <input
                type="range" min={0} max={1} step={0.01}
                value={mod.params.rate}
                onChange={(e) => {
                  mod.params.rate = Number(e.target.value)
                  // trigger re-sync via a no-op route refresh
                  useStore.setState((s) => ({ tracks: [...s.tracks] }))
                }}
              />
            </div>
          )}
          <div className="hint" style={{ margin: '3px 0 1px' }}>Routes</div>
          {meta.map((m) => {
            const route = mod.routes.find((r) => r.targetParam === m.key)
            return (
              <div key={m.key} className="mod-route">
                <span style={{ width: 44 }}>{m.label}</span>
                <input
                  type="range" min={-1} max={1} step={0.01}
                  value={route?.amount ?? 0}
                  onChange={(e) => setModRoute(track.id, device.id, mod.id, m.key, Number(e.target.value))}
                />
              </div>
            )
          })}
        </div>
      ))}
      <button className="add-mod" onClick={() => addModulator(track.id, device.id, 'lfo')}>+ LFO</button>
    </div>
  )
}

function DeviceView({ track, device, selected }: { track: Track; device: Device; selected: boolean }) {
  const setParam = useStore((s) => s.setParam)
  const toggleDevice = useStore((s) => s.toggleDevice)
  const removeDevice = useStore((s) => s.removeDevice)
  const selectDevice = useStore((s) => s.selectDevice)
  const meta = PARAM_META[device.kind]

  // collect modulated params for visual indication
  const modulated = new Set<string>()
  device.modulators.forEach((m) => m.routes.forEach((r) => modulated.add(r.targetParam)))

  return (
    <div
      className={`device${selected ? ' sel' : ''}${device.enabled ? '' : ' disabled'}`}
      onClick={() => selectDevice(device.id)}
    >
      <div className="device-head">
        <div
          className={`led${device.enabled ? ' on' : ''}`}
          onClick={(e) => { e.stopPropagation(); toggleDevice(track.id, device.id) }}
          title="Bypass"
        />
        <div className="dname">{device.name}</div>
        <div className="dclose" onClick={(e) => { e.stopPropagation(); removeDevice(track.id, device.id) }}>✕</div>
      </div>
      <div style={{ display: 'flex', flex: 1 }}>
        <div className="device-body">
          {meta.map((m) => {
            if (m.type === 'select') {
              return (
                <div key={m.key} className="knob-wrap">
                  <select
                    className="param-select"
                    value={Math.round(device.params[m.key])}
                    onChange={(e) => setParam(track.id, device.id, m.key, Number(e.target.value))}
                  >
                    {m.options!.map((opt, i) => <option key={opt} value={i}>{opt}</option>)}
                  </select>
                  <span className="klabel">{m.label}</span>
                </div>
              )
            }
            return (
              <Knob
                key={m.key}
                label={m.label}
                value={device.params[m.key]}
                modulated={modulated.has(m.key)}
                onChange={(v) => setParam(track.id, device.id, m.key, v)}
              />
            )
          })}
        </div>
        <ModSection track={track} device={device} />
      </div>
    </div>
  )
}

export function DevicePanel() {
  const tracks = useStore((s) => s.tracks)
  const selectedTrackId = useStore((s) => s.selectedTrackId)
  const selectedDeviceId = useStore((s) => s.selectedDeviceId)
  const track = tracks.find((t) => t.id === selectedTrackId)

  return (
    <div className="device-panel">
      <div className="panel-title">
        <span>Device Chain {track ? `— ${track.name}` : ''}</span>
        <span className="hint">drop devices from the Browser ▸</span>
      </div>
      <div className="device-strip">
        {track && track.devices.length > 0 ? (
          track.devices.map((d) => (
            <DeviceView key={d.id} track={track} device={d} selected={d.id === selectedDeviceId} />
          ))
        ) : (
          <div className="empty-state">
            <div>No devices on this track</div>
            <div className="hint">Add one from the Browser on the right</div>
          </div>
        )}
      </div>
    </div>
  )
}
