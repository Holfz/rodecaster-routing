export type CellState = 'linked' | 'unlinked' | 'muted'

export interface Cell {
  col: number
  id: number
  state: CellState
  level: number | null
  levelSteps: number | null
}

export interface Row {
  row: number
  label: string
  colour: string
  fader: number | null
  masterMute: boolean
  cells: Cell[]
}

export interface Output {
  col: number
  label: string
  mode: number
  modeLabel: string
  custom: boolean
}

export interface Channel {
  index: number
  label: string
  colour: string
  source: number
  mute: boolean
  cue: boolean
  talkback: boolean
  bypassProcessing: boolean
  pan: number | null
  fxPreset: number
  level: number | null
  levelSteps: number | null
}

export interface Info {
  firmware: string | null
  serial: string | null
  mixerBuild: string | null
  sampleRate: number | null
  bufferSize: number | null
  recordLabel: string
  storage: string
  network: string | null
  ssid: string | null
  show: string | null
  usb1Connected: boolean | null
  /** `outputMonMute` — the studio monitor mute. Null until the console reports it. */
  monitorMute: boolean | null
  /**
   * `outputMonLevel` — monitor volume, 0..1. Position only: it is neither the
   * faders' 127 steps nor a whole percentage, so no scale is claimed for it.
   */
  monitorLevel: number | null
  /**
   * `encoderColour` — an index, not a colour. `inputColour` arrives as an ARGB
   * string; this arrives as a bare int, and which index is which colour is not
   * established, so it is shown as the number the console reports.
   */
  encoderColour: number | null
}

export interface Matrix {
  /** The sixteen `aarrggbb` colours the console accepts. Anything else is refused. */
  palette: string[]
  outputs: Output[]
  rows: Row[]
  channels: Channel[]
  info: Info
  warnings: string[]
  mixBase: number
  readMs: number
}

export interface FrameLog {
  at: number
  dir: 'in' | 'out'
  name: string
  id: string
  idNum: number
  values: string
  hex: string
  applied: boolean
}

/**
 * A logged frame plus a stable client-side identity.
 *
 * Frames are unshifted, so the array index changes for every row on every
 * incoming frame. Keying a list on that index re-creates every row each time —
 * which throws away the row under the pointer, so a tooltip never lives long
 * enough to open. `seq` is assigned once, when the frame arrives.
 */
export interface LoggedFrame extends FrameLog {
  seq: number
}

export type Patch =
  | {
      kind: 'cell'
      row: number
      col: number
      state: CellState
      level: number | null
      levelSteps: number | null
      stripLevel: number | null
      stripLevelSteps: number | null
    }
  | { kind: 'outputMode'; col: number; mode: number; modeLabel: string; custom: boolean }
  | { kind: 'channelMute'; index: number; source: number; mute: boolean }
  | { kind: 'monitorMute'; mute: boolean }
  | { kind: 'monitorLevel'; level: number }
  | { kind: 'encoderColour'; colour: number }
  | { kind: 'inputColour'; row: number; colour: string }

/**
 * 89 of 127 is unity gain.
 *
 * RØDE's own UI puts no dB figure on a fader: unity is a marked position — the
 * white marks on the hardware, small arrows in the app — and the dBFS numbers
 * in Broadcast metering belong to the level meters, not the fader. So this
 * shows position, and names unity the way RØDE does.
 */
export const UNITY_STEPS = 89

export function levelLabel(steps: number | null): string {
  if (steps === null) return '—'
  if (steps === UNITY_STEPS) return 'Unity'
  return `${((steps / 127) * 100).toFixed(1)}%`
}

/** channelPan runs 0..1 with 0.5 centred, so report it as L/C/R. */
export function panLabel(pan: number): string {
  const offset = Math.round((pan - 0.5) * 200)
  if (offset === 0) return 'C'
  return offset < 0 ? `L${-offset}` : `R${offset}`
}

/** inputColour arrives as AARRGGBB straight from the console. */
export function swatch(argb: string): string {
  return `#${argb?.length === 8 ? argb.slice(2) : '888888'}`
}



/** A Windows capture endpoint, the same list a chat client offers. */
export interface CaptureDevice {
  name: string
  /** The host's default input. */
  default: boolean
}

/** What the analyser opened, sent once when capture starts. */
export interface RtaInfo {
  device: string
  sampleRate: number
  channels: number
  /** Band centre frequencies, ascending, one per value in a frame's `db`. */
  centres: number[]
}

/** One analysis frame, about 23 a second while the RTA page is open. */
export interface RtaFrame {
  /** dBFS per band, in `RtaInfo.centres` order. A full-scale sine reads 0. */
  db: number[]
  peakDb: number
  clipped: boolean
}
