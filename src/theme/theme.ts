/**
 * Bitwig Studio 6 visual language.
 *
 * v6 introduced a refreshed, permanently-dark interface with rounded edges.
 * These tokens approximate Bitwig's signature near-black panels, subtle
 * panel separators, and the warm accent palette used for tracks/devices.
 */

export const theme = {
  // Surfaces — Bitwig's layered near-black greys
  bg: '#1a1a1a',
  panel: '#222222',
  panelAlt: '#2a2a2a',
  panelRaised: '#303030',
  header: '#181818',
  slot: '#262626',
  slotEmpty: '#1e1e1e',

  // Lines & separators
  border: '#0d0d0d',
  borderSoft: '#383838',
  grid: '#2e2e2e',
  gridStrong: '#3a3a3a',

  // Text
  text: '#d8d8d8',
  textDim: '#8a8a8a',
  textFaint: '#5c5c5c',

  // Bitwig signature orange accent (transport/record/selection)
  accent: '#ff8a00',
  accentDim: '#b35f00',
  record: '#ff3b30',
  play: '#5ac85a',

  // Rounded corners (v6)
  radius: 4,
  radiusLg: 8,
} as const

/** Default Bitwig-ish track colour swatches (used when creating tracks). */
export const trackColors = [
  '#e0584f', // red
  '#e08a3c', // orange
  '#e0c64f', // yellow
  '#9bcf52', // green
  '#4fb6c8', // cyan
  '#5a8ad0', // blue
  '#9a6fd0', // purple
  '#d066a8', // magenta
  '#8a8a8a', // grey
]

export function pickTrackColor(index: number): string {
  return trackColors[index % trackColors.length]
}
