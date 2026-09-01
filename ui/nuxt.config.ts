export default defineNuxtConfig({
  // Tauri serves the built files from disk, so no server rendering.
  ssr: false,
  modules: ['@nuxt/ui'],
  devtools: { enabled: false },
  css: ['~/assets/app.css'],
  // The palette is dark and only dark; nothing here follows the OS.
  colorMode: { preference: 'dark', fallback: 'dark' },
  // Relative asset URLs so the bundle works from Tauri's asset protocol.
  app: { baseURL: './', head: { title: 'RØDECaster Routing' } },
  vite: {
    // Tauri needs a fixed dev port and shows real errors rather than swallowing them.
    clearScreen: false,
    server: { strictPort: true },
  },
})
