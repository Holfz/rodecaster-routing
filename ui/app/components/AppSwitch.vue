<script setup lang="ts">
const on = defineModel<boolean>({ required: true })
defineProps<{ label: string; title?: string }>()
</script>

<template>
  <!-- UTooltip renders nothing of its own: its trigger is `as-child`, so the
       button below is still the only element here. With no title it disables
       itself, so a switch without one behaves exactly as before. -->
  <UTooltip :text="title">
    <button
      type="button"
      role="switch"
      :aria-checked="on"
      class="flex items-center gap-2"
      @click="on = !on"
    >
      <span
        class="flex h-5 w-9 flex-none items-center rounded-full p-0.5 transition-colors"
        :class="on ? 'bg-accent justify-end' : 'bg-white/10'"
      >
        <span class="size-4 rounded-full bg-white shadow-[0_1px_3px_rgb(0_0_0/0.35)]" />
      </span>
      <span
        class="text-[15px] font-medium whitespace-nowrap"
        :class="on ? 'text-ink-2' : 'text-ink-3'"
      >
        {{ label }}
      </span>
    </button>
  </UTooltip>
</template>
