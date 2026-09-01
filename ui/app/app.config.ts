export default defineAppConfig({
  ui: {
    tooltip: {
      slots: {
        // Nuxt UI's own surface tokens are not this app's, so the tooltip is
        // painted from the same palette as every other panel. Sizing lives in
        // `.rc-tip` in app.css: the default theme pins the content to `h-6`
        // and truncates the text, which clips anything past one short line.
        content:
          'rc-tip bg-panel text-ink-2 rounded-[9px] px-2.5 py-1.5 text-[13px] font-medium ' +
          'shadow-[0_6px_20px_rgb(0_0_0/0.45)] ring ring-white/10 z-50',
      },
    },
  },
})
