<script setup lang="ts">
import type { Info } from '~/types'

const props = defineProps<{ info: Info }>()

/** Only what the state dump actually reports; a missing field says so. */
const facts = computed(() => {
  const i = props.info
  const rows: { k: string; v: string }[] = [
    { k: 'Firmware', v: i.firmware ?? '—' },
    { k: 'Serial', v: i.serial ?? '—' },
    {
      k: 'Audio',
      v: i.sampleRate ? `${(i.sampleRate / 1000).toFixed(1)} kHz · ${i.bufferSize ?? '?'} smp` : '—',
    },
    { k: 'Recorder', v: i.recordLabel },
    { k: 'Storage', v: i.storage },
    { k: 'USB 1', v: i.usb1Connected === null ? '—' : i.usb1Connected ? 'Connected' : 'Not connected' },
  ]
  // A palette index, not a colour: reported as the number, because which
  // number is which colour has never been captured.
  if (i.encoderColour !== null) rows.push({ k: 'Encoder', v: `Colour ${i.encoderColour}` })
  if (i.network) rows.push({ k: 'Network', v: i.network })
  if (i.ssid) rows.push({ k: 'Wi-Fi', v: i.ssid })
  if (i.show) rows.push({ k: 'Show', v: i.show })
  if (i.mixerBuild) rows.push({ k: 'Mixer Build', v: i.mixerBuild })
  return rows
})
</script>

<template>
  <section class="panel flex-none">
    <h2
      class="border-b border-white/5 px-3.5 py-2.5 text-base font-semibold tracking-[0.01em] text-ink"
    >
      Device
    </h2>
    <dl class="px-4 pt-2 pb-3.5">
      <div
        v-for="f in facts"
        :key="f.k"
        class="flex items-baseline justify-between gap-3 border-b border-white/4 py-2 last:border-0"
      >
        <dt class="flex-none text-[15px] font-medium text-ink-4">{{ f.k }}</dt>
        <UTooltip :text="f.v">
          <dd class="min-w-0 truncate tabular-nums text-xs text-ink">{{ f.v }}</dd>
        </UTooltip>
      </div>
    </dl>
  </section>
</template>
