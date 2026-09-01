<script setup lang="ts">
import type { Cell, Matrix, Output, Row } from '~/types'
import { levelLabel, swatch } from '~/types'

const props = defineProps<{
  matrix: Matrix
  /** Input columns, already filtered and ordered by the parent. */
  columns: Row[]
  /**
   * `faders` is the nine strips on the console and gets wide, labelled cells.
   * `all` is every source the console reports and has to fall back to a dense
   * square grid — thirty of them will not fit any other way.
   */
  scope: 'faders' | 'all'
  pending: string | null
}>()

const emit = defineEmits<{ cycle: [row: Row, cell: Cell] }>()

const { display, rename, reset, count } = useLocalNames()

const inName = (r: Row) => display('input', r.row, r.label)
const outName = (o: Output) => display('output', o.col, o.label)

const wide = computed(() => props.scope === 'faders')

/** Grid metrics, shared by the header and the body so the two stay aligned. */
const GAP = 4
const PAD = 16
const LABEL_W = 220

const scroller = useTemplateRef<HTMLElement>('scroller')
const box = ref({ w: 0, h: 0 })

let watcher: ResizeObserver | null = null
onMounted(() => {
  if (!scroller.value) return
  watcher = new ResizeObserver(([entry]) => {
    const r = entry?.contentRect
    box.value = { w: r?.width ?? 0, h: r?.height ?? 0 }
  })
  watcher.observe(scroller.value)
})
onBeforeUnmount(() => watcher?.disconnect())

const clamp = (lo: number, n: number, hi: number) => Math.max(lo, Math.min(hi, n))

/**
 * Wide columns carry the name the right way up and a fader number under it, so
 * the head is a fixed two lines. Square columns turn the name on its side, so
 * their head is as tall as the longest one: Rajdhani is narrow, and ~7px per
 * character plus room for the colour dot lands within a few pixels.
 */
const HEAD_H = computed(() => {
  if (wide.value) return 66
  const longest = props.columns.reduce((n, c) => Math.max(n, inName(c).length), 0)
  return clamp(72, Math.round(longest * 7 + 30), 140)
})

/** What one column and one row could have if they divided the panel evenly. */
const share = computed(() => {
  const cols = props.columns.length
  const rows = props.matrix.outputs.length
  if (!cols || !rows || !box.value.w) return { w: 0, h: 0 }
  return {
    w: Math.floor((box.value.w - PAD * 2 - LABEL_W - GAP) / cols) - GAP,
    h: Math.floor((box.value.h - HEAD_H.value - PAD) / rows) - GAP,
  }
})

/**
 * Cells grow into whatever room there is, on both axes, within limits.
 *
 * Nine faders leave most of a wide window empty at a fixed size, and thirteen
 * outputs at a generous one push most of the rows off the bottom of a short
 * window. Square cells take whichever of the two fits; wide ones size their
 * axes independently, since a wide cell has no reason to stay square.
 */
const SQUARE = computed(() => clamp(32, Math.min(share.value.w, share.value.h), 56))
const COL_W = computed(() => (wide.value ? clamp(88, share.value.w, 140) : SQUARE.value))
const ROW_H = computed(() => (wide.value ? clamp(40, share.value.h, 56) : SQUARE.value))

/** Row and column under the pointer, for the crosshair. */
const hotRow = ref(-1)
const hotCol = ref(-1)

function clearHot() {
  hotRow.value = -1
  hotCol.value = -1
}

function hoverCell(outputCol: number, inputRow: number) {
  hotRow.value = outputCol
  hotCol.value = inputRow
}

const editing = ref<string | null>(null)
const draft = ref('')
const editInput = useTemplateRef<HTMLInputElement>('editInput')

function startRename(kind: 'input' | 'output', key: number, current: string) {
  editing.value = `${kind}:${key}`
  draft.value = current
}

function commit() {
  const open = editing.value
  if (!open) return
  const [kind, key] = open.split(':') as ['input' | 'output', string]
  const id = Number(key)
  const reported =
    kind === 'input'
      ? (props.columns.find(r => r.row === id)?.label ?? '')
      : (props.matrix.outputs[id]?.label ?? '')
  rename(kind, id, draft.value, reported)
  editing.value = null
}

function cancel() {
  editing.value = null
}

watch(editing, async open => {
  if (!open) return
  await nextTick()
  editInput.value?.select()
})

/**
 * A square cell has room for a colour and nothing else; a wide one can say the
 * word as well, which is worth having when the difference between linked and
 * unlinked decides whether a fader move is heard.
 */
const CELL_STYLE: Record<Cell['state'], string> = {
  linked: 'bg-link/16 shadow-[inset_0_0_0_1px_rgb(55_214_122/0.3)]',
  unlinked: 'bg-unlink/14 shadow-[inset_0_0_0_1px_rgb(76_141_255/0.28)]',
  muted: 'bg-mute/15 shadow-[inset_0_0_0_1px_rgb(229_72_77/0.28)]',
}
const DOT_STYLE: Record<Cell['state'], string> = {
  linked: 'bg-link',
  unlinked: 'bg-unlink',
  muted: 'bg-mute',
}
const WORD_STYLE: Record<Cell['state'], string> = {
  linked: 'text-[#6fe0a4]',
  unlinked: 'text-[#8fb6ff]',
  muted: 'text-[#f08a8d]',
}
const WORD: Record<Cell['state'], string> = {
  linked: 'Linked',
  unlinked: 'Unlinked',
  muted: 'Muted',
}

/**
 * Inert cells, and every cell while a write is in flight, take no clicks.
 *
 * This is a guard rather than the `disabled` attribute because a disabled
 * button fires no pointer events, so its tooltip would never open — and the
 * tooltip is the only place an inert cell's stored state is shown.
 */
function onCell(input: Row, output: Output) {
  if (!output.custom || props.pending !== null) return
  emit('cycle', input, input.cells[output.col]!)
}

/**
 * A cell on an output that is not on Custom.
 *
 * Main Mix and Mix Minus carry every channel whatever the cell underneath says,
 * so showing that cell's colour would be reporting a state that is not in
 * effect. The whole row goes to hatched grey instead and stops taking clicks —
 * the stored state is still there, one hover away in the tooltip, for when the
 * output is switched to Custom.
 */
const INERT_CELL = 'hatched cursor-not-allowed bg-white/[0.022]'
const INERT_DOT = 'size-[3px] rounded-full bg-[#3a4045]'

const STATE_WORD: Record<Cell['state'], string> = {
  linked: 'Linked and will follows the fader',
  unlinked: 'Unlinked and have its own independent level',
  muted: 'Muted',
}

/**
 * One labelled line per fact. A cell tooltip carries four unrelated things —
 * which route it is, what state it holds, its level and its address — and as
 * loose sentences they ran together. The labels let the eye jump to the one it
 * is after.
 */
function cellTitle(input: Row, output: Output, cell: Cell) {
  const lines = [`Route: ${inName(input)} → ${outName(output)}`]

  if (output.custom) {
    lines.push(`State: ${STATE_WORD[cell.state]}`)
  } else {
    // The stored state is real but has no effect, so it is labelled as stored
    // rather than presented as what the output is doing.
    lines.push(`State: ${STATE_WORD[cell.state]} (stored, not in effect)`)
    lines.push(
      `Output: ${outName(output)} is on ${output.modeLabel} and will carries every channel, so this cell does nothing and cannot be changed here.`,
    )
  }

  if (cell.state === 'unlinked' && cell.levelSteps !== null) {
    lines.push(`Level: ${levelLabel(cell.levelSteps)}`)
  }
  if (input.masterMute) {
    lines.push(`Strip: ${inName(input)} is master muted and will silent on every output regardless.`)
  }

  lines.push(`Cell ID: ${cell.id}`)
  return lines.join('\n')
}

function inputTitle(input: Row) {
  const renamed = input.label !== inName(input) ? `\nConsole reports "${input.label}".` : ''
  const muted = input.masterMute
    ? '\nMaster Muted: silent on every output, though the cells below are unchanged.'
    : input.fader !== null
      ? `\nFader ${input.fader + 1}.`
      : '\nNo fader on the console.'
  return `${inName(input)}${muted}${renamed}\nDouble-click to rename locally.`
}

function outputTitle(output: Output) {
  const renamed = output.label !== outName(output) ? `\nConsole reports "${output.label}".` : ''
  const mode = output.custom
    ? 'Custom mix: the cells in this row are live.'
    : `${output.modeLabel}: this output carries every channel, so its cells are stored but inert.`
  return `${outName(output)}\n${mode}${renamed}\nDouble-click to rename locally.`
}

const EDIT_FIELD =
  'z-9 h-6.5 rounded-lg bg-well px-2.5 text-base font-semibold text-ink shadow-[inset_0_0_0_1px_var(--color-accent),0_8px_20px_rgb(0_0_0/0.55)] outline-none'
</script>

<template>
  <section class="panel flex min-h-0 flex-1 flex-col">
    <div class="flex flex-none items-center gap-3.5 border-b border-white/5 px-4.5 py-3.5">
      <h2 class="flex-none text-[17px] font-semibold tracking-[0.01em] text-ink">Routing Matrix</h2>
      <p class="min-w-0 flex-1 truncate text-sm text-ink-5">
        Click a cell to change how that input reaches that output. Double-click any name to rename
        it locally.
      </p>
      <UTooltip
        v-if="count"
        text="Clears every name typed in here and goes back to the names the console and the app report. Nothing on the device changes."
      >
        <button
          type="button"
          class="flex-none rounded-lg bg-accent/15 px-2.5 py-1 text-sm font-medium text-accent transition-colors hover:bg-accent/25"
          @click="reset"
        >
          Reset {{ count }} local {{ count === 1 ? 'name' : 'names' }}
        </button>
      </UTooltip>
    </div>

    <div
      class="flex flex-none flex-wrap items-center gap-x-4 gap-y-2 border-b border-white/5 px-4.5 py-2.5"
    >
      <span class="flex items-center gap-1.5">
        <span class="size-2 rounded-full bg-link" />
        <span class="text-sm font-medium text-ink-4">Custom Mix</span>
      </span>
      <span class="flex items-center gap-1.5">
        <span class="size-2 rounded-full bg-[#4a5257]" />
        <span class="text-sm font-medium text-ink-4">Locked Mix</span>
      </span>
      <span class="ml-auto flex flex-wrap items-center gap-x-4 gap-y-2">
        <span class="flex items-center gap-1.5">
          <span class="size-2 rounded-[3px] bg-link" />
          <span class="text-sm font-medium text-ink-4">Linked</span>
        </span>
        <span class="flex items-center gap-1.5">
          <span class="size-2 rounded-[3px] bg-unlink" />
          <span class="text-sm font-medium text-ink-4">Unlinked</span>
        </span>
        <span class="flex items-center gap-1.5">
          <span class="size-2 rounded-[3px] bg-mute" />
          <span class="text-sm font-medium text-ink-4">Muted</span>
        </span>
        <span class="flex items-center gap-1.5">
          <span class="hatched size-2 rounded-[3px] bg-white/8" />
          <span class="text-sm font-medium text-ink-4">Inert</span>
        </span>
      </span>
    </div>

    <div
      v-if="!columns.length"
      class="flex flex-1 items-center justify-center p-8 text-center text-ink-5"
    >
      No input carries a fader. Switch to “All inputs” to see every source the console reports.
    </div>

    <!-- `safe center` keeps the grid centred while it fits and pins it left
         once it overflows, so scrolling never cuts the first column off. -->
    <div
      v-else
      ref="scroller"
      class="flex min-h-0 flex-1 items-start overflow-auto"
      style="justify-content: safe center"
      @mouseleave="clearHot"
    >
      <div class="flex-none px-4 pb-4">
        <div class="sticky top-0 z-5 flex items-end bg-panel">
          <div
            class="sticky left-0 z-6 flex flex-none items-end bg-panel px-3 pb-3"
            :style="{ width: `${LABEL_W}px`, height: `${HEAD_H}px`, marginRight: `${GAP}px` }"
          >
            <span class="text-[13px] font-medium text-ink-5">Outputs ↓ &nbsp;·&nbsp; Inputs →</span>
          </div>

          <UTooltip v-for="input in columns" :key="input.row" :text="inputTitle(input)">
            <div
              class="relative flex flex-none flex-col items-center justify-end rounded-t-[9px] pb-2 transition-colors"
              :class="[
                input.masterMute ? 'bg-mute/10' : '',
                hotCol === input.row ? 'bg-white/6' : '',
              ]"
              :style="{ width: `${COL_W}px`, height: `${HEAD_H}px`, marginRight: `${GAP}px` }"
              @mouseenter="hoverCell(-1, input.row)"
              @dblclick="startRename('input', input.row, inName(input))"
            >
              <template v-if="wide">
                <span class="flex max-w-full items-center gap-1.5 px-1">
                  <!-- Read-only here: the colour is changed on the channel
                       strip, where the source is named once rather than once
                       per column. -->
                  <span
                    class="size-2 flex-none rounded-full"
                    :style="{ background: swatch(input.colour) }"
                  />
                  <span
                    class="truncate text-[15px] font-semibold tracking-[0.01em] transition-colors"
                    :class="hotCol === input.row ? 'text-ink' : 'text-ink-2'"
                  >
                    {{ inName(input) }}
                  </span>
                </span>
                <!-- Master mute belongs on the strip, never in the cells: the
                     console keeps reporting those as linked. -->
                <span v-if="input.masterMute" class="caps mt-px text-[10px] text-mute">
                  Master Muted
                </span>
                <span v-else-if="input.fader !== null" class="mt-px tabular-nums text-[10px] text-ink-5">
                  Fader {{ input.fader + 1 }}
                </span>
                <span v-else class="mt-px text-[10px] text-ink-5">No Fader</span>
              </template>

              <template v-else>
                <span
                  class="rotate-180 text-[15px] font-semibold whitespace-nowrap transition-colors [writing-mode:vertical-rl]"
                  :class="hotCol === input.row ? 'text-ink' : 'text-ink-3'"
                >
                  {{ inName(input) }}
                </span>
                <span
                  v-if="input.masterMute"
                  class="mt-1.5 rounded bg-mute/25 px-1 text-[10px] font-bold tracking-wider text-mute"
                >
                  M
                </span>
              </template>

              <span
                v-if="!wide && !input.masterMute"
                class="mt-1.5 size-1.5 rounded-full"
                :style="{ background: swatch(input.colour) }"
              />

              <input
                v-if="editing === `input:${input.row}`"
                ref="editInput"
                v-model="draft"
                :class="EDIT_FIELD"
                :style="
                  wide
                    ? { position: 'absolute', left: '4px', right: '4px', bottom: '8px' }
                    : { position: 'absolute', left: '0', bottom: '24px', width: '158px' }
                "
                @dblclick.stop
                @keydown.enter="commit"
                @keydown.esc="cancel"
                @blur="commit"
              >
            </div>
          </UTooltip>
        </div>

        <!-- One row per output. -->
        <div
          v-for="output in matrix.outputs"
          :key="output.col"
          class="flex"
          :style="{ marginBottom: `${GAP}px` }"
        >
          <UTooltip :text="outputTitle(output)">
            <div
              class="sticky left-0 z-2 flex flex-none items-center gap-2.5 rounded-[9px] pr-3.5 pl-3 transition-colors"
              :class="hotRow === output.col ? 'bg-white/7' : 'bg-panel'"
              :style="{ width: `${LABEL_W}px`, height: `${ROW_H}px`, marginRight: `${GAP}px` }"
              @mouseenter="hoverCell(output.col, -1)"
              @dblclick="startRename('output', output.col, outName(output))"
            >
              <span
                class="size-[7px] flex-none rounded-full"
                :class="output.custom ? 'bg-link' : 'bg-[#4a5257]'"
              />
              <span
                class="min-w-0 flex-1 truncate text-base font-semibold tracking-[0.01em] transition-colors"
                :class="
                  !output.custom ? 'text-ink-5' : hotRow === output.col ? 'text-ink' : 'text-ink-2'
                "
              >
                {{ outName(output) }}
              </span>
              <span v-if="!output.custom" class="flex-none text-[13px] font-medium text-ink-5">
                {{ output.modeLabel }}
              </span>

              <input
                v-if="editing === `output:${output.col}`"
                ref="editInput"
                v-model="draft"
                class="absolute inset-x-2 top-1/2 -translate-y-1/2"
                :class="EDIT_FIELD"
                @dblclick.stop
                @keydown.enter="commit"
                @keydown.esc="cancel"
                @blur="commit"
              >
            </div>
          </UTooltip>

          <UTooltip
            v-for="input in columns"
            :key="input.row"
            :text="cellTitle(input, output, input.cells[output.col]!)"
          >
            <button
              type="button"
              class="relative flex flex-none items-center justify-center gap-2 transition-colors"
              :class="[
                wide ? 'rounded-[10px]' : 'rounded-lg',
                output.custom
                  ? `cursor-pointer ${CELL_STYLE[input.cells[output.col]!.state]}`
                  : INERT_CELL,
                hotRow === output.col || hotCol === input.row
                  ? 'ring-1 ring-white/10 ring-inset'
                  : '',
              ]"
              :style="{ width: `${COL_W}px`, height: `${ROW_H}px`, marginRight: `${GAP}px` }"
              :aria-disabled="!output.custom || pending !== null"
              @mouseenter="hoverCell(output.col, input.row)"
              @click="onCell(input, output)"
            >
              <template v-if="output.custom">
                <span
                  class="size-2.5 flex-none rounded-[4px]"
                  :class="[
                    DOT_STYLE[input.cells[output.col]!.state],
                    pending === `${input.row}:${output.col}` ? 'animate-pulse' : '',
                  ]"
                />
                <span
                  v-if="wide"
                  class="text-sm font-semibold tracking-[0.01em]"
                  :class="WORD_STYLE[input.cells[output.col]!.state]"
                >
                  {{ WORD[input.cells[output.col]!.state] }}
                </span>

                <!-- Only unlinked cells have a level of their own. A linked cell
                     just mirrors the fader, so showing it here would repeat the
                     channel strip on every column.

                     The wrapper does the positioning because LevelBar's own root
                     is `relative` for its unity tick: putting `absolute` on the
                     component itself loses the cascade, and the bar then sits in
                     the flex row as a zero-width item, shoving the dot and the
                     word off centre. -->
                <span
                  v-if="
                    input.cells[output.col]!.state === 'unlinked' &&
                    input.cells[output.col]!.level !== null
                  "
                  class="pointer-events-none absolute bottom-1"
                  :class="wide ? 'inset-x-3' : 'inset-x-1.5'"
                >
                  <LevelBar thin :level="input.cells[output.col]!.level" />
                </span>
              </template>
              <span v-else :class="INERT_DOT" />
            </button>
          </UTooltip>
        </div>
      </div>
    </div>
  </section>
</template>
