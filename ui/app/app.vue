<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import logo from '~/assets/logo.png'
import type { Cell, CellState, FrameLog, LoggedFrame, Matrix, Patch, Row } from '~/types'

const matrix = ref<Matrix | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)
const showAll = ref(false)
const pending = ref<string | null>(null)
/** Skip unlink: clicking toggles straight between link and mute. */
const binaryMode = ref(false)
const view = ref<'matrix' | 'protocol'>('matrix')

const PREFS = 'rodecaster-deck.prefs'
function restorePrefs() {
  try {
    const saved = JSON.parse(localStorage.getItem(PREFS) ?? '{}')
    binaryMode.value = !!saved.binaryMode
    showAll.value = !!saved.showAll
  } catch {
    // A missing or unreadable preference is not worth surfacing.
  }
}
watch([binaryMode, showAll], () => {
  try {
    localStorage.setItem(
      PREFS,
      JSON.stringify({ binaryMode: binaryMode.value, showAll: showAll.value }),
    )
  } catch {
    // Private browsing and locked-down profiles throw here; ignore.
  }
})

/**
 * Full cycle: link -> unlink -> mute -> link, the console's own three states.
 * Binary mode skips unlink entirely, so a cell is either carrying the fader or
 * silent — a cell already sitting on unlink goes to mute on the next click.
 */
const CYCLE: Record<CellState, CellState> = {
  linked: 'unlinked',
  unlinked: 'muted',
  muted: 'linked',
}

function nextState(current: CellState): CellState {
  if (!binaryMode.value) return CYCLE[current]
  return current === 'muted' ? 'linked' : 'muted'
}

async function load() {
  busy.value = true
  error.value = null
  try {
    matrix.value = await invoke<Matrix>('read_matrix')
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function cycle(row: Row, cell: Cell) {
  const next = nextState(cell.state)
  pending.value = `${row.row}:${cell.col}`
  // Optimistic, but the console is still the source of truth: set_cell returns
  // the real state and overwrites this.
  const previous = cell.state
  cell.state = next
  try {
    matrix.value = await invoke<Matrix>('set_cell', { row: row.row, col: cell.col, state: next })
  } catch (e) {
    cell.state = previous
    error.value = String(e)
  } finally {
    pending.value = null
  }
}

/**
 * Inputs run across the top and outputs down the side, matching the GoXLR
 * routing table. The console addresses cells by input row, so the data keeps
 * that shape and only the table is transposed.
 */
const inputColumns = computed(() => {
  const all = matrix.value?.rows ?? []
  const rows = showAll.value ? [...all] : all.filter(r => r.fader !== null)
  // Fader order, matching the console's channel strips left to right. Rows with
  // no fader keep source order and sit after the assigned ones.
  return rows.sort((a, b) => {
    if (a.fader === null && b.fader === null) return a.row - b.row
    if (a.fader === null) return 1
    if (b.fader === null) return -1
    return a.fader - b.fader
  })
})

// The backend only forwards wire frames while the protocol page is open: a
// fader slide produces one frame per cell per step.
watch(view, v => {
  invoke('set_frame_logging', { enabled: v === 'protocol' }).catch(() => {})
})

const frames = ref<LoggedFrame[]>([])
/** Only ever increments, so every logged frame keeps one identity for its life. */
let frameSeq = 0
const logPaused = ref(false)
const onlyApplied = ref(false)
/** Bounded so a long session cannot grow without limit. */
const LOG_LIMIT = 500

const visibleFrames = computed(() =>
  onlyApplied.value ? frames.value.filter(f => f.applied || f.dir === 'out') : frames.value,
)

/**
 * Apply one change in place.
 *
 * A fader slide sends an event per cell in the row; rebuilding the whole matrix
 * for each of those made the level bars stutter. Patching touches one cell and
 * one strip, so Vue only re-renders what actually moved.
 */
function applyPatch(p: Patch) {
  const m = matrix.value
  if (!m) return

  if (p.kind === 'cell') {
    const row = m.rows[p.row]
    const cell = row?.cells[p.col]
    if (!row || !cell) return
    cell.state = p.state
    cell.level = p.level
    cell.levelSteps = p.levelSteps
    // The strip's level derives from this row, so it rides along.
    const strip = m.channels.find(c => c.source === p.row)
    if (strip) {
      strip.level = p.stripLevel
      strip.levelSteps = p.stripLevelSteps
    }
    return
  }

  if (p.kind === 'outputMode') {
    const out = m.outputs[p.col]
    if (!out) return
    out.mode = p.mode
    out.modeLabel = p.modeLabel
    out.custom = p.custom
    return
  }

  if (p.kind === 'monitorMute') {
    m.info.monitorMute = p.mute
    return
  }

  if (p.kind === 'inputColour') {
    const row = m.rows[p.row]
    if (row) row.colour = p.colour
    const strip = m.channels.find(c => c.source === p.row)
    if (strip) strip.colour = p.colour
    return
  }

  if (p.kind === 'encoderColour') {
    m.info.encoderColour = p.colour
    return
  }

  if (p.kind === 'monitorLevel') {
    // While the slider is under the pointer the local value is the truth; the
    // console echoes each step back, and applying those would fight the drag.
    if (!monitorDragging.value) m.info.monitorLevel = p.level
    return
  }

  const strip = m.channels.find(c => c.index === p.index)
  if (strip) strip.mute = p.mute
  const row = m.rows[p.source]
  if (row) row.masterMute = p.mute
}

/**
 * The studio monitor mute. It belongs to the device rather than to a cell, so
 * it sits in the bar rather than the grid — and unlike a cell it is the one
 * control here that can put sound *into* the room, which is why the button
 * says which way it is about to go.
 */
/** Writes the console's own show state, so it persists and shows on the hardware. */
async function recolour(row: number, argb: string) {
  try {
    matrix.value = await invoke<Matrix>('set_input_colour', { row, colour: argb })
  } catch (e) {
    error.value = String(e)
  }
}

const monitorBusy = ref(false)
const monitorDragging = ref(false)

/**
 * Monitor volume. The console pushes an event per step, so a drag is throttled
 * rather than sent per frame — the same reason the matrix patches instead of
 * re-reading. A trailing send guarantees the final position lands.
 */
let levelTimer: ReturnType<typeof setTimeout> | undefined
let levelPending: number | null = null
onBeforeUnmount(() => clearTimeout(levelTimer))

async function pushLevel(level: number) {
  try {
    await invoke('set_monitor_level', { level })
  } catch (e) {
    error.value = String(e)
  }
}

function onLevelInput(e: Event) {
  const m = matrix.value
  if (!m) return
  const level = Number((e.target as HTMLInputElement).value)
  m.info.monitorLevel = level
  monitorDragging.value = true
  levelPending = level
  if (levelTimer) return
  levelTimer = setTimeout(function flush() {
    levelTimer = undefined
    if (levelPending === null) return
    const next = levelPending
    levelPending = null
    pushLevel(next)
    levelTimer = setTimeout(flush, 80)
  }, 80)
}

function onLevelCommit(e: Event) {
  monitorDragging.value = false
  clearTimeout(levelTimer)
  levelTimer = undefined
  levelPending = null
  pushLevel(Number((e.target as HTMLInputElement).value))
}
async function toggleMonitor() {
  const current = matrix.value?.info.monitorMute
  if (current === null || current === undefined || monitorBusy.value) return
  monitorBusy.value = true
  try {
    matrix.value = await invoke<Matrix>('set_monitor_mute', { mute: !current })
  } catch (e) {
    error.value = String(e)
  } finally {
    monitorBusy.value = false
  }
}

/**
 * The webview's own right-click menu offers reload, back and save-as. None of
 * them mean anything in a console utility, and reload drops the listener's
 * connection. Text fields keep theirs, so a name can still be pasted into a
 * rename.
 */
function suppressContextMenu(e: MouseEvent) {
  const el = e.target as Element | null
  if (el?.closest?.('input, textarea')) return
  e.preventDefault()
}
onMounted(() => window.addEventListener('contextmenu', suppressContextMenu))
onBeforeUnmount(() => window.removeEventListener('contextmenu', suppressContextMenu))

onMounted(async () => {
  restorePrefs()
  // The console pushes every state change, including ones made on its own
  // touchscreen or by the RØDECaster App, so the UI follows rather than polls.
  await listen<Patch>('matrix-patch', e => applyPatch(e.payload))
  // Emitted only on connect and reconnect, when the whole model is replaced.
  await listen('matrix-changed', () => {
    if (!busy.value) load()
  })
  await listen<FrameLog>('protocol-frame', e => {
    if (logPaused.value) return
    frames.value.unshift({ ...e.payload, seq: frameSeq++ })
    if (frames.value.length > LOG_LIMIT) frames.value.length = LOG_LIMIT
  })
  load()
})

const TAB = 'rounded-lg px-5 py-1.5 text-base font-semibold tracking-[0.01em] transition-colors'
</script>

<template>
  <!-- UApp supplies Reka's tooltip provider, which UTooltip requires. The
       toaster is off: unused here, and it is the only part that would put an
       element between this div and the body. -->
  <UApp :toaster="null" :tooltip="{ delayDuration: 300 }">
    <div class="flex h-full flex-col overflow-hidden bg-chrome text-ink-2">
    <TitleBar @error="error = $event" />

    <div
      class="flex flex-none flex-wrap items-center justify-between gap-x-5 gap-y-3 border-b border-white/5 px-4.5 py-3.5"
    >
      <div class="flex items-center gap-4.5">
        <div class="flex gap-0.5 rounded-[11px] bg-white/5 p-[3px]">
          <button
            type="button"
            :class="[TAB, view === 'matrix' ? 'bg-accent text-chrome' : 'text-ink-3 hover:text-ink']"
            @click="view = 'matrix'"
          >
            Matrix
          </button>
          <button
            type="button"
            :class="[
              TAB,
              view === 'protocol' ? 'bg-accent text-chrome' : 'text-ink-3 hover:text-ink',
            ]"
            @click="view = 'protocol'"
          >
            Protocol
          </button>
        </div>

        <template v-if="view === 'matrix'">
          <!-- Scope decides the shape of the grid, not just its length: nine
               faders get wide cells that say the word, thirty sources only fit
               as squares. -->
          <div class="flex gap-0.5 rounded-[11px] bg-white/5 p-[3px]">
            <UTooltip text="The nine strips on the console, in fader order.">
              <button
                type="button"
                :class="[TAB, !showAll ? 'bg-accent text-chrome' : 'text-ink-3 hover:text-ink']"
                @click="showAll = false"
              >
                Fader Inputs
              </button>
            </UTooltip>
            <UTooltip
              text="Every source the console reports, including the ones no fader drives. Some have no label the evidence supports, so they read as “source N”."
            >
              <button
                type="button"
                :class="[TAB, showAll ? 'bg-accent text-chrome' : 'text-ink-3 hover:text-ink']"
                @click="showAll = true"
              >
                All Inputs
              </button>
            </UTooltip>
          </div>

          <AppSwitch
            v-model="binaryMode"
            label="Link / Mute Only"
            title="Clicks skip unlink, so a cell is either carrying the fader or silent. A cell already on unlink goes to mute."
          />
        </template>
      </div>

      <div class="flex items-center gap-4.5">
        <UTooltip
          v-if="matrix && matrix.info.monitorMute !== null"
          :text="
            matrix.info.monitorMute
              ? 'Studio monitors are muted.'
              : 'Studio monitors are unmuted.'
          "
        >
          <button
            type="button"
            :disabled="monitorBusy"
            :class="[
              'flex items-center gap-2 rounded-[11px] px-3.5 py-1.5 text-[15px] font-semibold transition-colors disabled:opacity-50',
              matrix.info.monitorMute
                ? 'bg-mute/18 text-mute hover:bg-mute/26'
                : 'bg-link/16 text-link hover:bg-link/24',
            ]"
            @click="toggleMonitor"
          >
            <span class="size-[7px] flex-none rounded-full bg-current" />
            Monitor
          </button>
        </UTooltip>

        <UTooltip
          v-if="matrix && matrix.info.monitorLevel !== null"
          :text="
            matrix.info.monitorMute
              ? 'Monitor volume. The console only allows this while the monitor is unmuted.'
              : 'Monitor volume. Shown as position — this control has no dB scale.'
          "
        >
          <div class="flex items-center gap-2.5">
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              :value="matrix.info.monitorLevel"
              :disabled="!!matrix.info.monitorMute"
              class="monitor-slider w-24"
              @input="onLevelInput"
              @change="onLevelCommit"
            >
            <span
              class="w-9 flex-none tabular-nums text-right text-[15px] font-medium"
              :class="matrix.info.monitorMute ? 'text-ink-5' : 'text-ink-3'"
            >
              {{ Math.round((matrix.info.monitorLevel ?? 0) * 100) }}%
            </span>
          </div>
        </UTooltip>

      <div v-if="matrix" class="flex items-baseline gap-1.5">
        <span class="tabular-nums text-[17px] text-ink">
          {{ view === 'matrix' ? inputColumns.length : matrix.rows.length }}
        </span>
        <span class="text-sm text-ink-4">inputs</span>
        <span class="mx-1 text-ink-5">/</span>
        <span class="tabular-nums text-[17px] text-ink">{{ matrix.outputs.length }}</span>
        <span class="text-sm text-ink-4">outputs</span>
      </div>
      </div>
    </div>

    <div v-if="error || matrix?.warnings.length" class="flex flex-none flex-col gap-px">
      <div
        v-if="error"
        class="flex items-start gap-2.5 border-b border-white/5 bg-mute/12 px-4.5 py-2.5 text-sm"
      >
        <span class="mt-1 size-2 flex-none rounded-full bg-mute" />
        <span class="min-w-0 flex-1 selectable text-ink-2">
          <span class="font-semibold text-mute">Device Error</span> — {{ error }}
        </span>
        <button type="button" class="flex-none text-ink-4 hover:text-ink" @click="error = null">
          ✕
        </button>
      </div>
      <div
        v-for="w in matrix?.warnings ?? []"
        :key="w"
        class="flex items-start gap-2.5 border-b border-white/5 bg-warn/10 px-4.5 py-2.5 text-sm"
      >
        <span class="mt-1 size-2 flex-none rounded-full bg-warn" />
        <span class="min-w-0 flex-1 selectable text-ink-2">
          <span class="font-semibold text-warn">Addressing May Be Wrong</span> — {{ w }}
        </span>
      </div>
    </div>

    <main class="flex min-h-0 flex-1">
      <div class="flex min-w-0 flex-1 flex-col p-4">
        <MatrixPanel
          v-if="view === 'matrix' && matrix"
          :matrix="matrix"
          :columns="inputColumns"
          :scope="showAll ? 'all' : 'faders'"
          :pending="pending"
          @cycle="cycle"
        />
        <ProtocolPanel
          v-else-if="view === 'protocol'"
          v-model:paused="logPaused"
          v-model:only-applied="onlyApplied"
          :frames="visibleFrames"
          :limit="LOG_LIMIT"
          @clear="frames = []"
          @error="error = $event"
        />
        <section v-else class="panel flex flex-1 flex-col items-center justify-center gap-5">
          <img
            :src="logo"
            alt=""
            class="w-44 opacity-[0.12]"
            :class="busy ? 'animate-pulse' : ''"
          >
          <p class="text-ink-4">
            {{ busy ? 'Reading device state…' : 'Waiting for the console…' }}
          </p>
        </section>
      </div>

      <aside class="flex w-[330px] flex-none flex-col gap-4 overflow-y-auto py-4 pr-4">
        <template v-if="matrix">
          <ChannelStrips :matrix="matrix" @recolour="recolour" />
          <DevicePanel :info="matrix.info" />
        </template>
        <AutostartToggle @error="error = $event" />
      </aside>
    </main>

    <StatusBar :matrix="matrix" />
    </div>
  </UApp>
</template>
