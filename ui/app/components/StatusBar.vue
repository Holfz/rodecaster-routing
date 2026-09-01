<script setup lang="ts">
import type { Matrix } from '~/types'

const props = defineProps<{ matrix: Matrix | null }>()

/**
 * Cells the operator has actively silenced, across the outputs where a cell
 * means anything. Nothing here is sampled or timed: the console pushes its own
 * changes, so there is no poll rate or round-trip latency to report.
 */
const muted = computed(() => {
  const m = props.matrix
  if (!m) return 0
  const custom = m.outputs.filter(o => o.custom).map(o => o.col)
  return m.rows.reduce(
    (n, row) => n + custom.filter(col => row.cells[col]?.state === 'muted').length,
    0,
  )
})
</script>

<template>
  <footer
    class="flex h-[30px] flex-none items-center justify-between border-t border-white/5 bg-chrome px-4.5 tabular-nums text-[11px] text-ink-5"
  >
    <span class="flex items-center gap-1.5">
      <span
        class="size-1.5 rounded-full"
        :class="matrix ? 'bg-link' : 'animate-pulse bg-warn'"
      />
      <span class="text-sm font-medium text-ink-3">
        <template v-if="matrix">RØDECaster Pro II · USB</template>
        <template v-else>Waiting for the console</template>
      </span>
    </span>

    <span v-if="matrix" class="flex items-center gap-4.5">
      <UTooltip text="Muted cells on outputs set to Custom">
        <span>{{ muted }} muted</span>
      </UTooltip>
      <UTooltip text="Cell addressing base, derived from this dump rather than hardcoded">
        <span>base {{ matrix.mixBase }}</span>
      </UTooltip>
      <UTooltip text="How long the startup state dump took to read and scan">
        <span>dump {{ matrix.readMs }} ms</span>
      </UTooltip>
    </span>
  </footer>
</template>
