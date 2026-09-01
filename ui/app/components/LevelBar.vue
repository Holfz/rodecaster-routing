<script setup lang="ts">
/**
 * A fader level, drawn on the console's own 0-1 travel.
 *
 * Everything past unity is boost, so that part runs yellow through orange to
 * red while the part below stays neutral.
 *
 * Mechanically this is an ordinary progress bar: one fill element whose `width`
 * changes and nothing else. The gradient is painted at the *track's* width via
 * a fixed `background-size`, so it stays anchored to unity-to-full travel while
 * the fill crops it — no stretched inner element, no clip-path repaint, both of
 * which stuttered during a fader slide.
 */
const props = withDefaults(
  defineProps<{
    /** 0..1, or null when unknown. */
    level: number | null
    /** Unity as a fraction of full travel. 89/127 on this console. */
    unity?: number
    /** Muted strips keep their level but read dimmed. */
    muted?: boolean
    thin?: boolean
  }>(),
  { unity: 89 / 127, muted: false, thin: false },
)

const track = ref<HTMLElement | null>(null)
const trackWidth = ref(0)

let observer: ResizeObserver | null = null
onMounted(() => {
  if (!track.value) return
  observer = new ResizeObserver(([entry]) => {
    trackWidth.value = entry?.contentRect.width ?? 0
  })
  observer.observe(track.value)
})
onBeforeUnmount(() => observer?.disconnect())

const pct = computed(() => Math.max(0, Math.min(1, props.level ?? 0)) * 100)
const unityPct = computed(() => props.unity * 100)

/**
 * Neutral up to unity, then hot. Hard stops at unity so boost reads as its own
 * band, and the ramp is anchored to unity-to-full so a given level is always
 * the same colour.
 */
const gradient = computed(() => {
  const u = unityPct.value
  const base = props.muted ? 'var(--color-mute)' : 'var(--color-link)'
  const mid = u + (100 - u) * 0.55
  return `linear-gradient(to right, ${base} 0%, ${base} ${u}%, #f5a524 ${u}%, #f97316 ${mid}%, var(--color-mute) 100%)`
})

// Before the observer reports, fall back to the fill's own width. Slightly
// wrong colours for one frame beats no bar at all.
const backgroundSize = computed(() =>
  trackWidth.value > 0 ? `${trackWidth.value}px 100%` : '100% 100%',
)
</script>

<template>
  <span
    ref="track"
    class="relative block rounded-full"
    :class="[thin ? 'h-0.5' : 'h-[5px]', muted ? 'bg-mute/15' : 'bg-white/8']"
  >
    <span
      class="absolute inset-y-0 left-0 rounded-full"
      :class="muted ? 'opacity-50' : ''"
      :style="{
        width: `${pct}%`,
        backgroundImage: gradient,
        backgroundSize,
        backgroundRepeat: 'no-repeat',
      }"
    />
    <span
      v-if="!thin"
      class="absolute top-1/2 h-2 w-px -translate-y-1/2 bg-white/40"
      :style="{ left: `${unityPct}%` }"
    />
  </span>
</template>
