import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    // The Rust binary serves this directory.
    outDir: 'dist',
    sourcemap: false,
    rollupOptions: {
      output: {
        // The libraries change far less often than the app, so they go in
        // chunks of their own that browsers keep cached across releases.
        manualChunks(id) {
          if (!id.includes('/node_modules/')) return undefined
          if (/\/node_modules\/(react|react-dom|scheduler|react-router|react-router-dom|@remix-run)\//.test(id)) return 'react'
          if (id.includes('/node_modules/@tanstack/')) return 'query'
          if (id.includes('/node_modules/lucide-react/')) return 'icons'
          return 'vendor'
        },
      },
    },
  },
  server: {
    port: 5173,
    // In development the UI runs on Vite and the API on axum; proxying keeps
    // requests same-origin so the app needs no CORS configuration at all.
    proxy: {
      '/api': {
        target: process.env.CAIREZEKI_API ?? 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
})
