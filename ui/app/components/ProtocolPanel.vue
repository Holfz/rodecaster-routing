<script setup lang="ts">
import type { FrameLog, LoggedFrame } from '~/types'

const props = defineProps<{ frames: LoggedFrame[]; limit: number }>()
const emit = defineEmits<{ clear: []; error: [string] }>()

const paused = defineModel<boolean>('paused', { required: true })
const onlyApplied = defineModel<boolean>('onlyApplied', { required: true })

/**
 * One frame as one line, columns in the order they appear on screen and padded
 * so a stack of them still lines up wherever it is pasted. The hex goes last
 * and unpadded because it is the only field with no useful width.
 */
function frameLine(f: FrameLog) {
  const cols: [string, number][] = [
    [`${(f.at / 1000).toFixed(3)}s`, 9],
    [f.dir === 'out' ? 'OUT' : 'IN', 4],
    [f.name, 24],
    [f.idNum ? `${f.id} · ${f.idNum}` : f.id, 18],
    [f.values, 24],
    [f.hex, 0],
  ]
  return cols.map(([text, width]) => text.padEnd(width)).join(' ').trimEnd()
}

/** Which row flashed as copied; `all` for the header button. */
const copied = ref<string | null>(null)
let flash: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(flash))

async function copy(text: string, mark: string) {
  try {
    await toClipboard(text)
    copied.value = mark
    clearTimeout(flash)
    flash = setTimeout(() => (copied.value = null), 1100)
  } catch (e) {
    emit('error', `Could not copy to the clipboard: ${e}`)
  }
}

/**
 * The webview grants `navigator.clipboard` on Tauri's own origin, but only
 * under a user gesture and not on every platform. The textarea fallback covers
 * a refusal rather than letting the click do nothing.
 */
async function toClipboard(text: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      return
    }
  } catch {
    // Fall through to the fallback below.
  }
  const field = document.createElement('textarea')
  field.value = text
  field.style.cssText = 'position:fixed;top:0;left:0;opacity:0'
  document.body.append(field)
  field.select()
  const ok = document.execCommand('copy')
  field.remove()
  if (!ok) throw new Error('the webview refused the clipboard')
}

function copyAll() {
  if (props.frames.length) copy(props.frames.map(frameLine).join('\n'), 'all')
}

const BUTTON =
  'rounded-[9px] px-4 py-1.5 text-[15px] font-medium transition-colors disabled:opacity-40'
const QUIET = 'bg-white/5 text-ink-3 hover:bg-white/9 hover:text-ink-2'
</script>

<template>
  <section class="panel flex min-h-0 flex-1 flex-col">
    <div class="flex flex-none items-center justify-between gap-4 border-b border-white/5 px-4.5 py-3.5">
      <div class="flex items-center gap-3.5">
        <h2 class="text-[17px] font-semibold tracking-[0.01em] text-ink">Protocol Monitor</h2>
        <span class="tabular-nums text-xs text-ink-5">{{ frames.length }} / {{ limit }} frames</span>
      </div>
      <div class="flex items-center gap-2.5">
        <AppSwitch
          v-model="onlyApplied"
          label="Only Frames That Changed State"
          title="Hides console events this app does not model. Those dimmed rows are how new events get found, so this stays off by default."
        />
        <UTooltip text="Copy every frame listed here as plain text, in the order shown.">
          <button
            type="button"
            :class="[BUTTON, copied === 'all' ? 'bg-link/20 text-link' : QUIET]"
            :disabled="!frames.length"
            @click="copyAll"
          >
            {{ copied === 'all' ? 'Copied' : 'Copy All' }}
          </button>
        </UTooltip>
        <button
          type="button"
          :class="[BUTTON, paused ? 'bg-accent text-chrome' : QUIET]"
          @click="paused = !paused"
        >
          {{ paused ? 'Resume' : 'Pause' }}
        </button>
        <button type="button" :class="[BUTTON, QUIET]" @click="emit('clear')">Clear</button>
      </div>
    </div>

    <div v-if="!frames.length" class="flex flex-1 items-center justify-center p-8 text-center text-ink-5">
      Nothing yet. Touch the console, or click a cell, and frames appear here.
    </div>

    <div v-else class="min-h-0 flex-1 overflow-auto tabular-nums">
      <div class="min-w-max">
        <div
          class="caps sticky top-0 z-2 flex h-8 items-center bg-panel px-4.5 text-ink-5 shadow-[0_1px_0_rgb(255_255_255/0.05)]"
        >
          <span class="w-[70px] flex-none">t</span>
          <span class="w-[64px] flex-none">dir</span>
          <span class="w-[200px] flex-none">event</span>
          <span class="w-[110px] flex-none">id</span>
          <span class="w-[220px] flex-none">values</span>
          <span class="min-w-[420px] flex-1">bytes</span>
        </div>

        <!-- A row is a button: one click puts that frame on the clipboard as the
             same line the Copy all button writes. -->
        <UTooltip v-for="(f, n) in frames" :key="f.seq" text="Click to copy this frame">
          <button
            type="button"
            class="flex h-[27px] w-full items-center px-4.5 text-left text-[11px] transition-colors"
            :class="[
              copied === String(f.seq)
                ? 'bg-link/20'
                : [n % 2 ? 'bg-white/2' : '', 'hover:bg-white/7'],
              f.applied || f.dir === 'out' ? '' : 'opacity-55',
            ]"
            @click="copy(frameLine(f), String(f.seq))"
          >
            <span class="w-[70px] flex-none text-ink-5">{{ (f.at / 1000).toFixed(3) }}s</span>
            <span class="w-[64px] flex-none">
              <span
                class="rounded-md px-1.5 py-0.5 text-[12px] font-semibold tracking-[0.08em]"
                :class="f.dir === 'out' ? 'bg-accent/15 text-accent' : 'bg-link/15 text-link'"
              >
                {{ f.dir === 'out' ? 'OUT' : 'IN' }}
              </span>
            </span>
            <span class="w-[200px] flex-none truncate text-[#a8dcc0]">{{ f.name }}</span>
            <span class="w-[110px] flex-none text-ink-3">
              {{ f.id }}<template v-if="f.idNum"> · {{ f.idNum }}</template>
            </span>
            <span class="w-[220px] flex-none truncate text-[#d5b985]">{{ f.values }}</span>
            <span class="min-w-[420px] flex-1 whitespace-nowrap text-ink-5">{{ f.hex }}</span>
          </button>
        </UTooltip>
      </div>
    </div>

    <p class="flex-none border-t border-white/5 px-4.5 py-2.5 text-[13px] text-ink-5">
      Click a row to copy it. Dimmed rows changed no state this app models — those are how new events
      get found. OUT frames are what this app sent; IN frames are the console reporting, whoever
      caused the change.
    </p>
  </section>
</template>
