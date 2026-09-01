<script setup lang="ts">
// Imported statically, like the core and event APIs in app.vue. A dynamic
// `import()` here put the window API in its own chunk, and that chunk does not
// reliably load over Tauri's asset protocol — the buttons then did nothing at
// all, because a failed chunk load only rejects a promise nobody was reading.
import { getCurrentWindow } from '@tauri-apps/api/window'
import logoMark from '~/assets/logo-mark.png'

/**
 * The window's own title bar, because the native one cannot carry the mark and
 * sits at a different height to everything below it. `data-tauri-drag-region`
 * gives back dragging and double-click-to-maximise; resizing is still the OS's.
 *
 * Tauri's drag script ignores clicks whose target is a button, so the three
 * controls below work despite sitting inside the drag region.
 */
const emit = defineEmits<{ error: [string] }>()

const inTauri = import.meta.client && '__TAURI_INTERNALS__' in window
const maximised = ref(false)

/** Every one of these can be refused by the ACL; none of them may fail quietly. */
async function act(what: string, run: (w: ReturnType<typeof getCurrentWindow>) => Promise<unknown>) {
  try {
    await run(getCurrentWindow())
  } catch (e) {
    emit('error', `Could not ${what} the window: ${e}`)
  }
}

const minimise = () => act('minimise', w => w.minimize())
const close = () => act('close', w => w.close())
const maximise = () =>
  act('maximise', async w => {
    await w.toggleMaximize()
    maximised.value = await w.isMaximized()
  })

onMounted(async () => {
  if (!inTauri) return
  const w = getCurrentWindow()
  try {
    maximised.value = await w.isMaximized()
    // Snapping and the OS's own shortcuts resize the window too, so the icon
    // follows the window rather than only our own button.
    await w.onResized(async () => {
      maximised.value = await w.isMaximized()
    })
  } catch {
    // A wrong icon is not worth a banner; the button still toggles.
  }
})

const BTN =
  'flex w-[46px] items-center justify-center text-ink-4 transition-colors hover:text-white'
</script>

<template>
  <header
    data-tauri-drag-region
    class="flex h-10 flex-none items-center justify-between border-b border-white/5 bg-chrome pl-4"
  >
    <div class="pointer-events-none flex items-center gap-2.5">
      <!-- The mark is a single-colour drawing, so it is worn as a mask and
           takes the accent rather than shipping a second, tinted copy. -->
      <span
        class="block h-4 w-[29px] bg-accent"
        :style="{
          maskImage: `url(${logoMark})`,
          maskSize: 'contain',
          maskRepeat: 'no-repeat',
          maskPosition: 'center',
        }"
      />
      <span class="text-[15px] font-semibold tracking-[0.02em] text-ink">RØDECaster</span>
      <span class="text-[15px] tracking-[0.02em] text-ink-4">Routing</span>
    </div>

    <div v-if="inTauri" class="flex h-full items-stretch">
      <UTooltip text="Minimise">
        <button type="button" :class="[BTN, 'hover:bg-white/6']" @click="minimise">
          <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden="true">
            <path d="M1 5.5h9" stroke="currentColor" stroke-width="1.1" />
          </svg>
        </button>
      </UTooltip>
      <UTooltip :text="maximised ? 'Restore' : 'Maximise'">
        <button type="button" :class="[BTN, 'hover:bg-white/6']" @click="maximise">
          <svg
            width="11"
            height="11"
            viewBox="0 0 11 11"
            aria-hidden="true"
            fill="none"
            stroke="currentColor"
            stroke-width="1.1"
          >
            <template v-if="maximised">
              <rect x="1.2" y="3.2" width="6.6" height="6.6" />
              <path d="M3.4 3.2V1.2h6.4v6.4H7.8" />
            </template>
            <rect v-else x="1.4" y="1.4" width="8.2" height="8.2" />
          </svg>
        </button>
      </UTooltip>
      <UTooltip text="Close">
        <button type="button" :class="[BTN, 'hover:bg-mute']" @click="close">
          <svg width="11" height="11" viewBox="0 0 11 11" aria-hidden="true">
            <path d="M1.4 1.4l8.2 8.2M9.6 1.4l-8.2 8.2" stroke="currentColor" stroke-width="1.1" />
          </svg>
        </button>
      </UTooltip>
    </div>
  </header>
</template>
