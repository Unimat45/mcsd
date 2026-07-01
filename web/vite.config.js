import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// The app is built to static assets and embedded in the Go binary. In dev,
// `npm run dev` serves the UI and proxies API/SSE calls to the running daemon.
export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    // Output directly into the Go package that embeds the SPA via go:embed
    // (go:embed cannot reach outside its own package directory).
    outDir: '../internal/webui/dist',
    emptyOutDir: true,
  },
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8765',
        changeOrigin: true,
      },
    },
  },
})
