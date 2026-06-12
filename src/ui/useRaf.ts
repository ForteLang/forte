import { useEffect, useRef } from 'react'

/** Run a callback every animation frame while mounted. */
export function useRaf(cb: () => void) {
  const ref = useRef(cb)
  ref.current = cb
  useEffect(() => {
    let raf = 0
    const loop = () => {
      ref.current()
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [])
}
