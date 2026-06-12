import { useRef } from 'react'

interface KnobProps {
  value: number          // 0..1
  label: string
  display?: string
  modulated?: boolean
  onChange: (v: number) => void
}

/** Vertical-drag rotary knob, Bitwig-style (270° sweep). */
export function Knob({ value, label, display, modulated, onChange }: KnobProps) {
  const startY = useRef(0)
  const startVal = useRef(0)

  const onPointerDown = (e: React.PointerEvent) => {
    e.preventDefault()
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
    startY.current = e.clientY
    startVal.current = value
  }
  const onPointerMove = (e: React.PointerEvent) => {
    if (!(e.buttons & 1)) return
    const dy = startY.current - e.clientY
    const speed = e.shiftKey ? 0.001 : 0.005
    const next = Math.max(0, Math.min(1, startVal.current + dy * speed))
    onChange(next)
  }
  const onDouble = () => onChange(0.5)

  const angle = -135 + value * 270
  return (
    <div className="knob-wrap">
      <div
        className={`knob${modulated ? ' modulated' : ''}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onDoubleClick={onDouble}
      >
        <div className="ind" style={{ transform: `rotate(${angle}deg)` }} />
      </div>
      <span className="klabel">{label}</span>
      <span className="kval">{display ?? Math.round(value * 100)}</span>
    </div>
  )
}
