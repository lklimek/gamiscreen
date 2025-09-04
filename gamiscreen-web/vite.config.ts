import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // Option A: use CORS on server; Option B: proxy API to avoid CORS in dev
    proxy: process.env.VITE_DEV_PROXY === '1' ? {
      '/api': {
        target: process.env.VITE_API_PROXY_TARGET || 'http://localhost:3000',
        changeOrigin: true,
      },
    } : undefined,
  },
  build: {
    outDir: 'dist'
  }
})

