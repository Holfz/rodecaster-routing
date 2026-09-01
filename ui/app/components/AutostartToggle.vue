<script setup lang="ts">
// Statically imported for the same reason as the window API: a lazily-loaded
// chunk does not reliably resolve over Tauri's asset protocol.
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart'

/**
 * Whether the app registers itself to start with the desktop.
 *
 * The backend turns this on once, the first time the app ever runs, because the
 * console is always plugged in and this is only useful while it is following
 * events. From then on this toggle is the only thing that changes it, and the
 * choice sticks across launches.
 *
 * The switch shows what the OS actually reports, never an optimistic guess: it
 * is written, then read back.
 */
const emit = defineEmits<{ error: [string] }>()

const on = ref(false)
const ready = ref(false)

async function sync() {
  try {
    on.value = await isEnabled()
    ready.value = true
  } catch (e) {
    emit('error', `Could not read the autostart setting: ${e}`)
  }
}

onMounted(sync)

async function toggle(next: boolean) {
  try {
    await (next ? enable() : disable())
  } catch (e) {
    emit('error', `Could not ${next ? 'enable' : 'disable'} autostart: ${e}`)
  }
  await sync()
}

const proxy = computed({
  get: () => on.value,
  set: v => {
    void toggle(v)
  },
})
</script>

<template>
  <section v-if="ready" class="panel flex-none px-4 py-3">
    <AppSwitch
      v-model="proxy"
      label="Start With Windows"
      title="Registers the app under the current user's Run key, so it launches when you sign in. Nothing is written to the console."
    />
  </section>
</template>
