<script setup lang="ts">
import type { Channel, Matrix } from '~/types'
import { levelLabel, panLabel, swatch, UNITY_STEPS } from '~/types'

const props = defineProps<{ matrix: Matrix }>()

const { display } = useLocalNames()

/**
 * The console has nine faders, so the list is always laid out for nine even
 * when fewer are assigned. Otherwise the Device panel below it would jump up
 * and down the sidebar as strips come and go.
 */
const MAX_STRIPS = 9
const ROW_H = 46
const ROW_GAP = 6

/**
 * How many custom-mix outputs still carry this strip.
 *
 * Only Custom outputs are counted: the rest are on Main Mix or Mix Minus and
 * carry every channel whatever their cells say, so folding them in would make
 * the number meaningless.
 */
function reach(ch: Channel) {
  const row = props.matrix.rows[ch.source]
  const custom = props.matrix.outputs.filter(o => o.custom)
  if (!row) return null
  return {
    n: custom.filter(o => row.cells[o.col]?.state !== 'muted').length,
    of: custom.length,
  }
}

/**
 * One chip for the strip's processing, not two.
 *
 * `bypassProcessing` takes the whole chain out of circuit, so whatever FX
 * preset is selected underneath cannot be heard — saying "bypassed" and "no
 * FX" side by side told the same thing twice. Bypass therefore wins, and the
 * preset is only worth naming when it is actually in circuit.
 */
function processing(ch: Channel) {
  if (ch.bypassProcessing) {
    return {
      label: 'Bypassed',
      hot: false,
      title:
        'Processing bypassed: the chain is out of circuit, so any FX preset on this strip is not applied.',
    }
  }
  if (ch.fxPreset >= 0) {
    return {
      label: `FX ${ch.fxPreset}`,
      hot: true,
      title: `FX preset ${ch.fxPreset} is in circuit.`,
    }
  }
  return {
    label: 'No FX',
    hot: false,
    title: 'Processing is in circuit, but no FX preset is selected.',
  }
}

const CHIP = 'caps flex-none rounded-md px-1.5 py-px text-[11px] leading-[1.35]'

/**
 * Source colour lives here rather than on the routing matrix: a source is named
 * once on its strip, but appears as a column in every row of the grid.
 *
 * The console accepts only its own sixteen colours — an arbitrary one is
 * dropped and the old colour stays — so the palette is offered as it is, not as
 * a free picker. This writes the console's show state and shows on the
 * hardware.
 */
const emit = defineEmits<{ recolour: [row: number, argb: string] }>()
const picking = ref<number | null>(null)

function pick(source: number, argb: string) {
  picking.value = null
  emit('recolour', source, argb)
}
</script>

<template>
  <section class="panel flex flex-none flex-col">
    <div class="flex flex-none items-center justify-between border-b border-white/5 px-4 py-3">
      <h2 class="text-base font-semibold tracking-[0.01em] text-ink">Channel Strips</h2>
      <UTooltip :text="`${matrix.channels.length} of the console's ${MAX_STRIPS} faders are assigned`">
        <span class="tabular-nums text-xs text-ink-5">
          {{ String(matrix.channels.length).padStart(2, '0') }} / {{ MAX_STRIPS }}
        </span>
      </UTooltip>
    </div>

    <div
      class="p-2.5"
      :style="{ minHeight: `${MAX_STRIPS * (ROW_H + ROW_GAP) - ROW_GAP + 20}px` }"
    >
      <div
        v-for="ch in matrix.channels"
        :key="ch.index"
        class="rounded-[9px] px-2.5 py-2 transition-colors"
        :class="ch.mute ? 'bg-mute/8 hover:bg-mute/12' : 'bg-white/3 hover:bg-white/6'"
        :style="{ marginBottom: `${ROW_GAP}px` }"
      >
        <div class="flex items-center gap-1.5">
          <UPopover
            :open="picking === ch.source"
            @update:open="picking = $event ? ch.source : null"
          >
            <UTooltip text="This source's colour on the console. Click to change it — only the console's own sixteen are accepted, and it shows on the hardware.">
              <button
                type="button"
                class="size-[9px] flex-none rounded-full transition-transform hover:scale-125"
                :style="{
                  background: swatch(ch.colour),
                  boxShadow: `0 0 5px ${swatch(ch.colour)}99`,
                }"
              />
            </UTooltip>

            <template #content>
              <div class="grid grid-cols-4 gap-1.5 rounded-[10px] bg-panel p-2.5 ring ring-white/10">
                <button
                  v-for="argb in matrix.palette"
                  :key="argb"
                  type="button"
                  class="size-6 rounded-md transition-transform hover:scale-110"
                  :class="argb === ch.colour ? 'ring-2 ring-white/80' : 'ring-1 ring-white/15'"
                  :style="{ background: swatch(argb) }"
                  @click="pick(ch.source, argb)"
                />
              </div>
            </template>
          </UPopover>
          <UTooltip class="min-w-0 flex-1" :text="display('input', ch.source, ch.label)">
            <span
              class="block truncate text-[15px] font-semibold tracking-[0.01em]"
              :class="ch.mute ? 'text-mute' : 'text-ink'"
            >
              {{ display('input', ch.source, ch.label) }}
            </span>
          </UTooltip>

          <!-- channelOutputMute is the strip's master mute: it silences the
               channel while every cell stays linked, which is why the routing
               table shows no change for it. -->
          <UTooltip v-if="ch.mute" text="Master mute: silent on every output, cells unchanged">
            <span :class="[CHIP, 'bg-mute/20 text-mute']">Mute</span>
          </UTooltip>
          <UTooltip v-if="ch.cue" text="Cued to headphones">
            <span :class="[CHIP, 'bg-unlink/15 text-unlink']">Cue</span>
          </UTooltip>
          <UTooltip v-if="ch.talkback" text="Talkback active">
            <span :class="[CHIP, 'bg-warn/15 text-warn']">TB</span>
          </UTooltip>
          <UTooltip :text="processing(ch).title">
            <span
              :class="[
                CHIP,
                processing(ch).hot ? 'bg-accent/15 text-accent' : 'bg-white/5 text-ink-5',
              ]"
            >
              {{ processing(ch).label }}
            </span>
          </UTooltip>
          <UTooltip :text="`Fader ${ch.index + 1}`">
            <span class="flex-none tabular-nums text-[11px] text-ink-5">F{{ ch.index + 1 }}</span>
          </UTooltip>
        </div>

        <div class="mt-1.5 flex items-center gap-2 text-[12px] text-ink-5">
          <LevelBar class="min-w-8 flex-1" :level="ch.level" :muted="ch.mute" />
          <!-- A muted strip keeps its level: it is dimmed red, not blanked,
               because the fader has not moved. -->
          <UTooltip
            text="Fader position on the console's own travel. RØDE puts no dB scale on a fader, so neither does this."
          >
            <span
              class="flex-none tabular-nums"
              :class="
                ch.mute ? 'text-mute/70' : ch.levelSteps === UNITY_STEPS ? 'text-ink' : 'text-ink-3'
              "
            >
              {{ levelLabel(ch.levelSteps) }}
            </span>
          </UTooltip>
          <UTooltip text="Pan position">
            <span class="flex-none tracking-[0.06em]">
              PAN <span class="tabular-nums">{{ ch.pan === null ? '—' : panLabel(ch.pan) }}</span>
            </span>
          </UTooltip>
          <UTooltip
            v-if="reach(ch)"
            :text="`Not muted on ${reach(ch)!.n} of the ${reach(ch)!.of} outputs set to Custom. Outputs on Main Mix or Mix Minus carry every channel, so they are not counted.`"
          >
            <span class="flex-none tracking-[0.06em]">
              <span class="tabular-nums">{{ reach(ch)!.n }}/{{ reach(ch)!.of }}</span> OUT
            </span>
          </UTooltip>
        </div>
      </div>
    </div>
  </section>
</template>
