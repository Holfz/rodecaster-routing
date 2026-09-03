<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { CaptureDevice, RtaFrame, RtaInfo } from '~/types'

const emit = defineEmits<{ error: [string] }>()

type Smoothing = 'fast' | 'normal' | 'slow'

/** How long a band takes to fall, in seconds. Rising is always fast. */
const RELEASE: Record<Smoothing, number> = { fast: 0.05, normal: 0.18, slow: 0.5 }
const ATTACK = 0.02

/** The bottom of the display, not of the data: bands can and do read lower. */
const FLOOR = -96
const CEILING = 0

/** Peak hold: how long a peak stands, then how fast it falls. */
const HOLD_SECONDS = 1.5
const FALL_DB_PER_SECOND = 16

const GRID_HZ = [20, 30, 50, 100, 200, 300, 500, 1000, 2000, 3000, 5000, 10000, 20000]

/** The gridline the cursor's label sits on. */
const LABEL_DB = -24

const PREFS = 'rodecaster-deck.rta'

const devices = ref<CaptureDevice[]>([])
const info = ref<RtaInfo | null>(null)
const chosen = ref<string>('')
const starting = ref(false)
const frozen = ref(false)
const peakHold = ref(true)
const smoothing = ref<Smoothing>('normal')
const currentDb = ref(FLOOR)
const heldDb = ref(FLOOR)
const clipping = ref(false)

/**
 * Frames arrive about 23 times a second and carry 256 numbers each. None of
 * this is reactive: Vue would re-render the panel on every frame, and the only
 * thing that has to change is the canvas.
 */
let latest: number[] | null = null
let latestPeak = FLOOR
let latestClipped = false
let levels = new Float32Array(0)
let peaks = new Float32Array(0)
let ages = new Float32Array(0)
let cursorX: number | null = null

/** The two readouts, before they are throttled onto the refs above. */
let levelNow = FLOOR
let levelHeld = FLOOR
let heldAge = 0
let clipAge = Infinity
let lastReadout = 0

const canvas = ref<HTMLCanvasElement | null>(null)
const plot = ref<HTMLElement | null>(null)
let frame = 0
let lastDraw = 0
let observer: ResizeObserver | undefined
const stopListening: Array<() => void> = []

function readPrefs() {
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS) ?? '{}')
    if (typeof saved.device === 'string') chosen.value = saved.device
    if (saved.smoothing in RELEASE) smoothing.value = saved.smoothing
    peakHold.value = saved.peakHold !== false
  } catch {
    // A missing or unreadable preference is not worth surfacing.
  }
}

watch([chosen, smoothing, peakHold], () => {
  try {
    localStorage.setItem(
      PREFS,
      JSON.stringify({ device: chosen.value, smoothing: smoothing.value, peakHold: peakHold.value }),
    )
  } catch {
    // Private browsing and locked-down profiles throw here; ignore.
  }
})

/** The host's default is marked, the way a chat client marks it. */
const deviceItems = computed(() =>
  devices.value.map(d => ({
    label: d.default ? `${d.name} [System Default]` : d.name,
    value: d.name,
  })),
)

async function start() {
  starting.value = true
  info.value = null
  latest = null

  try {
    // A remembered endpoint can be gone since last time, so the backend picks
    // one itself rather than the call failing on a name that no longer exists.
    const wanted = devices.value.some(d => d.name === chosen.value) ? chosen.value : null
    const opened = await invoke<RtaInfo>('start_rta', { device: wanted })

    info.value = opened
    chosen.value = opened.device

    const bands = opened.centres.length
    levels = new Float32Array(bands).fill(FLOOR)
    peaks = new Float32Array(bands).fill(FLOOR)
    ages = new Float32Array(bands)

    latestPeak = FLOOR
    latestClipped = false
    levelNow = FLOOR
    levelHeld = FLOOR
    clipAge = Infinity
  } catch (e) {
    emit('error', String(e))
  } finally {
    starting.value = false
  }
}

async function restart() {
  await invoke('stop_rta').catch(() => {})
  await start()
}

onMounted(async () => {
  readPrefs()

  try {
    devices.value = await invoke<CaptureDevice[]>('list_capture_devices')
  } catch (e) {
    emit('error', String(e))
  }

  stopListening.push(
    await listen<RtaFrame>('rta-frame', e => {
      if (frozen.value) return
      latest = e.payload.db
      latestPeak = e.payload.peakDb
      latestClipped = e.payload.clipped
    }),
    // The stream can fail after it opened, typically the device going away.
    await listen<string>('rta-error', e => emit('error', `Capture stopped: ${e.payload}`)),
  )

  observer = new ResizeObserver(resize)
  if (plot.value) observer.observe(plot.value)
  resize()

  frame = requestAnimationFrame(draw)
  await start()
})

onBeforeUnmount(() => {
  cancelAnimationFrame(frame)
  observer?.disconnect()
  for (const off of stopListening) off()
  invoke('stop_rta').catch(() => {})
})

function resize() {
  const el = canvas.value
  const box = plot.value
  if (!el || !box) return

  const dpr = window.devicePixelRatio || 1
  el.width = Math.max(1, Math.round(box.clientWidth * dpr))
  el.height = Math.max(1, Math.round(box.clientHeight * dpr))
}

function onPointerMove(e: PointerEvent) {
  const box = plot.value
  if (box) cursorX = e.clientX - box.getBoundingClientRect().left
}

/** Read the theme's own colours rather than repeating them here. */
let palette: { accent: string; ink: string; grid: string; label: string } | null = null

function colours() {
  const token = (name: string, fallback: string) =>
    getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback

  palette ??= {
    accent: token('--color-accent', '#e8442f'),
    ink: token('--color-ink', '#eceef0'),
    grid: 'rgb(255 255 255 / 0.07)',
    label: token('--color-ink-5', '#61686e'),
  }
  return palette
}

/** Hex from the theme, at the partial alpha the canvas wants. */
function withAlpha(colour: string, alpha: number) {
  const hex = colour.replace('#', '')
  const full = hex.length === 3 ? hex.replace(/./g, c => c + c) : hex
  const n = Number.parseInt(full, 16)
  if (Number.isNaN(n)) return colour

  return `rgb(${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255} / ${alpha})`
}

/**
 * Move every band towards the last frame.
 *
 * Fast up and slow down is the RTA convention, and it is what makes a peak
 * readable: a transient rises to its true height, then falls at a rate the eye
 * can follow rather than flickering with each frame.
 */
function advance(seconds: number) {
  if (!latest || frozen.value) return

  const rise = 1 - Math.exp(-seconds / ATTACK)
  const fall = 1 - Math.exp(-seconds / RELEASE[smoothing.value])

  for (let i = 0; i < levels.length; i++) {
    const target = latest[i] ?? FLOOR
    const current = levels[i] ?? FLOOR
    const next = current + (target - current) * (target > current ? rise : fall)
    levels[i] = next

    const peak = peaks[i] ?? FLOOR
    if (next >= peak) {
      peaks[i] = next
      ages[i] = 0
      continue
    }

    const age = (ages[i] ?? 0) + seconds
    ages[i] = age
    if (age > HOLD_SECONDS) peaks[i] = peak - FALL_DB_PER_SECOND * seconds
  }

  advanceReadout(seconds, rise, fall)
}

/**
 * The level readout: the loudest sample in the last frame, and the highest that
 * has been in the seconds since.
 *
 * Held on the same terms as the curve's peak trace. A number that only ever
 * showed the current frame would be unreadable at 23 frames a second, and the
 * held one is what a gain is set against.
 */
function advanceReadout(seconds: number, rise: number, fall: number) {
  levelNow += (latestPeak - levelNow) * (latestPeak > levelNow ? rise : fall)

  if (levelNow >= levelHeld) {
    levelHeld = levelNow
    heldAge = 0
  } else {
    heldAge += seconds
    if (heldAge > HOLD_SECONDS) levelHeld -= FALL_DB_PER_SECOND * seconds
  }

  // A frame is 43 ms, so an unlatched indicator would light for less time than
  // it takes to look at it.
  clipAge = latestClipped ? 0 : clipAge + seconds
}

type Scale = (v: number) => number

function draw(now: number) {
  frame = requestAnimationFrame(draw)

  const el = canvas.value
  const ctx = el?.getContext('2d')
  if (!el || !ctx) return

  const seconds = lastDraw ? Math.min((now - lastDraw) / 1000, 0.25) : 0
  lastDraw = now
  advance(seconds)

  // Text, unlike the canvas, costs a Vue render to change. Ten times a second
  // is already faster than a number can be read.
  if (now - lastReadout > 100) {
    lastReadout = now
    currentDb.value = levelNow
    heldDb.value = levelHeld
    clipping.value = clipAge < HOLD_SECONDS
  }

  const dpr = window.devicePixelRatio || 1
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

  const w = el.width / dpr
  const h = el.height / dpr
  ctx.clearRect(0, 0, w, h)

  const centres = info.value?.centres
  if (!centres?.length || levels.length < 2) return

  // Band centres are geometric, so log frequency is linear in band index and
  // the curve can step across the width without a lookup per point.
  const low = Math.log(centres[0] ?? 20)
  const span = Math.log(centres[centres.length - 1] ?? 20000) - low
  const x = (hz: number) => ((Math.log(hz) - low) / span) * w
  const y = (db: number) => ((CEILING - db) / (CEILING - FLOOR)) * h

  drawGrid(ctx, w, h, x, y)
  drawCurve(ctx, w, h, y)
  if (cursorX !== null) drawCursor(ctx, w, h, centres, x, y)
}

function drawGrid(ctx: CanvasRenderingContext2D, w: number, h: number, x: Scale, y: Scale) {
  const c = colours()

  ctx.lineWidth = 1
  ctx.strokeStyle = c.grid
  ctx.fillStyle = c.label
  ctx.font = '11px Rajdhani, system-ui, sans-serif'

  ctx.textAlign = 'center'
  ctx.textBaseline = 'bottom'
  for (const hz of GRID_HZ) {
    // The scale runs between band centres, so 20 Hz and 20 kHz fall half a band
    // outside it and are pulled back in. A device sampling too slowly for
    // 20 kHz puts it far outside, and that one is dropped.
    const exact = x(hz)
    if (exact < -8 || exact > w + 8) continue
    const at = Math.min(Math.max(Math.round(exact), 0), w - 1) + 0.5

    ctx.beginPath()
    ctx.moveTo(at, 0)
    ctx.lineTo(at, h - 14)
    ctx.stroke()

    // The line stays on the edge; only the text comes in, or half of it would
    // be cut off.
    const label = hz < 1000 ? String(hz) : `${hz / 1000}k`
    ctx.fillText(label, Math.min(Math.max(at, 14), w - 14), h - 2)
  }

  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  for (let db = CEILING; db > FLOOR; db -= 12) {
    const at = Math.round(y(db)) + 0.5

    ctx.beginPath()
    ctx.moveTo(0, at)
    ctx.lineTo(w, at)
    ctx.stroke()
    if (db < CEILING) ctx.fillText(String(db), 4, at - 7)
  }
}

function drawCurve(ctx: CanvasRenderingContext2D, w: number, h: number, y: Scale) {
  const c = colours()
  const step = w / (levels.length - 1)

  const line = trace(levels, step, y)

  // Closing the line along the bottom makes the shape read as level rather
  // than as a line wandering across an empty panel.
  const area = new Path2D(line)
  area.lineTo(w, h)
  area.lineTo(0, h)
  area.closePath()

  // Anchored to the top of the curve, not of the panel: a quiet signal sits
  // near the bottom, and a canvas-wide gradient leaves it with no fill at all.
  let crest = h
  for (let i = 0; i < levels.length; i++) crest = Math.min(crest, y(levels[i] ?? FLOOR))

  const gradient = ctx.createLinearGradient(0, crest, 0, h)
  gradient.addColorStop(0, withAlpha(c.accent, 0.45))
  gradient.addColorStop(1, withAlpha(c.accent, 0.05))
  ctx.fillStyle = gradient
  ctx.fill(area)

  ctx.strokeStyle = c.accent
  ctx.lineWidth = 1.5
  ctx.lineJoin = 'round'
  ctx.stroke(line)

  if (!peakHold.value) return

  ctx.strokeStyle = withAlpha(c.ink, 0.3)
  ctx.lineWidth = 1
  ctx.stroke(trace(peaks, step, y))
}

/**
 * One band per point, drawn as a curve rather than a polyline.
 *
 * Each segment is a quadratic through the midpoint between two bands, with the
 * band itself as the control point. Straight segments put a corner on every
 * band, and at 256 bands across the panel those corners read as steps that are
 * not in the audio.
 */
function trace(values: Float32Array, step: number, y: Scale) {
  const at = (i: number) => y(values[i] ?? FLOOR)
  const last = values.length - 1

  let d = `M 0 ${at(0)}`
  for (let i = 1; i < last; i++) {
    const mid = (i + 0.5) * step
    d += ` Q ${i * step} ${at(i)} ${mid} ${(at(i) + at(i + 1)) / 2}`
  }

  return new Path2D(`${d} L ${last * step} ${at(last)}`)
}

/** The reading under the pointer, which is what an EQ move is aimed at. */
function drawCursor(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  centres: number[],
  x: Scale,
  y: Scale,
) {
  const c = colours()
  const band = Math.round(((cursorX ?? 0) / w) * (centres.length - 1))
  const hz = centres[Math.min(Math.max(band, 0), centres.length - 1)]
  if (hz === undefined) return

  const at = x(hz)
  const level = levels[band] ?? FLOOR

  ctx.strokeStyle = withAlpha(c.ink, 0.26)
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.moveTo(at, 0)
  ctx.lineTo(at, h - 14)
  ctx.stroke()

  ctx.fillStyle = c.accent
  ctx.beginPath()
  ctx.arc(at, y(level), 3, 0, Math.PI * 2)
  ctx.fill()

  const label = `${hz < 1000 ? hz.toFixed(0) : `${(hz / 1000).toFixed(2)}k`} Hz   ${level.toFixed(1)} dB`
  ctx.font = '600 13px Rajdhani, system-ui, sans-serif'
  const width = ctx.measureText(label).width + 16

  // Parked on one gridline while the curve is live, where a label that moved
  // with the reading would be hard to read. Frozen, the reading holds still, so
  // it returns to the curve and points at the place it is reporting.
  const boxX = Math.min(Math.max(at + 8, 2), w - width - 2)
  const boxY = frozen.value
    ? Math.min(Math.max(y(level) - 32, 2), h - 40)
    : y(LABEL_DB) - 11

  ctx.fillStyle = 'rgb(8 9 9 / 0.92)'
  ctx.beginPath()
  ctx.roundRect(boxX, boxY, width, 22, 6)
  ctx.fill()

  ctx.fillStyle = c.ink
  ctx.textAlign = 'left'
  ctx.textBaseline = 'middle'
  ctx.fillText(label, boxX + 8, boxY + 11)
}

function dbLabel(db: number) {
  return db <= FLOOR ? '—' : `${db.toFixed(1)} dBFS`
}

/** The device select, wearing the same greys as the buttons beside it. */
const SELECT = {
  base: 'w-[430px] rounded-[9px] bg-white/5 text-ink-2 hover:bg-white/9',
  placeholder: 'text-ink-5',
  trailingIcon: 'text-ink-4',
  content: 'bg-panel rounded-[11px] ring ring-white/8 shadow-xl',
  viewport: 'p-1 divide-y-0',
  item: 'rounded-lg text-ink-3 data-highlighted:text-ink data-highlighted:before:bg-white/7',
  itemTrailingIcon: 'text-accent',
}

const BUTTON =
  'rounded-[9px] px-4 py-1.5 text-[15px] font-medium transition-colors disabled:opacity-40'
const QUIET = 'bg-white/5 text-ink-3 hover:bg-white/9 hover:text-ink-2'
</script>

<template>
  <section class="panel flex min-h-0 flex-1 flex-col">
    <div
      class="flex flex-none flex-wrap items-center justify-between gap-x-4 gap-y-3 border-b border-white/5 px-4.5 py-3.5"
    >
      <div class="flex items-center gap-3.5">
        <h2 class="text-[17px] font-semibold tracking-[0.01em] text-ink">Real-Time Analyzer</h2>
        <UTooltip
          text="Windows capture endpoints, the same list a chat client offers. The console's Chat endpoint carries the USB 1 Comms column of the matrix; Main Multitrack carries USB 1 Main."
        >
          <!-- Nuxt UI's own neutral palette is a lighter grey than this app's,
               so every slot that carries a colour is given one from the theme. -->
          <USelect
            v-model="chosen"
            :items="deviceItems"
            :placeholder="devices.length ? 'Choose a capture device' : 'No capture device'"
            variant="none"
            size="lg"
            :ui="SELECT"
            @update:model-value="restart"
          />
        </UTooltip>
      </div>

      <div class="flex items-center gap-2.5">
        <UTooltip
          text="How quickly a band falls back. Fast follows every syllable; slow shows the shape of a whole phrase, which is the one to set an EQ against."
        >
          <div class="flex gap-0.5 rounded-[10px] bg-white/5 p-[3px]">
            <button
              v-for="option in (['fast', 'normal', 'slow'] as const)"
              :key="option"
              type="button"
              class="rounded-lg px-3 py-1 text-[14px] font-semibold capitalize transition-colors"
              :class="smoothing === option ? 'bg-accent text-chrome' : 'text-ink-4 hover:text-ink-2'"
              @click="smoothing = option"
            >
              {{ option }}
            </button>
          </div>
        </UTooltip>

        <AppSwitch
          v-model="peakHold"
          label="Peak Hold"
          title="Keeps the highest reading of each band for a moment, then lets it fall. A short resonance stays visible long enough to read off."
        />

        <button
          type="button"
          :class="[BUTTON, frozen ? 'bg-accent text-chrome' : QUIET]"
          @click="frozen = !frozen"
        >
          {{ frozen ? 'Frozen' : 'Freeze' }}
        </button>
      </div>
    </div>

    <div
      ref="plot"
      class="relative min-h-0 flex-1 bg-well"
      @pointermove="onPointerMove"
      @pointerleave="cursorX = null"
    >
      <canvas ref="canvas" class="absolute inset-0 size-full" />

      <div v-if="!info" class="absolute inset-0 flex items-center justify-center text-ink-5">
        {{ starting ? 'Opening the capture device…' : 'No capture device is running.' }}
      </div>

      <!-- Left, clear of the dB scale, which starts one gridline down. -->
      <div class="pointer-events-none absolute left-3 top-2.5 flex flex-col items-start gap-1">
        <div class="tabular-nums text-[13px] leading-tight">
          <div class="flex gap-2">
            <span class="w-14 text-ink-5">current</span>
            <span class="text-ink-3">{{ dbLabel(currentDb) }}</span>
          </div>
          <div class="flex gap-2">
            <span class="w-14 text-ink-5">peak</span>
            <span class="text-ink-2">{{ dbLabel(heldDb) }}</span>
          </div>
        </div>
        <span
          v-if="clipping"
          class="rounded-md bg-mute/20 px-2 py-0.5 text-[13px] font-semibold text-mute"
        >
          Clipping
        </span>
      </div>
    </div>

    <div
      class="flex-none space-y-1 border-t border-white/5 px-4.5 py-2.5 text-[13px] text-ink-5"
    >
      <p v-if="info" class="text-ink-4">
        Device: {{ info.device }} · {{ (info.sampleRate / 1000).toFixed(1) }} kHz ·
        {{ info.channels === 1 ? 'mono' : 'stereo, summed' }} · {{ info.centres.length }} bands
      </p>
      <p>
        This reads the audio the console sends back over USB, so it hears whatever the matrix routes
        to that output. For the microphone on its own, leave only that source linked on the matching
        column. Nothing here writes to the console.
      </p>
    </div>
  </section>
</template>
