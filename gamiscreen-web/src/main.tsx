import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'
import { ensurePwaFreshness } from './pwaVersioning'
import packageInfo from '../package.json'
import '@picocss/pico/css/pico.min.css'
import './styles.css'

ensurePwaFreshness(packageInfo.version)

const container = document.getElementById('root')
if (!container) throw new Error('Root element not found')
createRoot(container).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)

// Register service worker in production builds
if ('serviceWorker' in navigator && import.meta.env.MODE === 'production') {
  window.addEventListener('load', () => {
    const base = (import.meta as any).env?.BASE_URL || '/'
    const url = (base.endsWith('/') ? base : base + '/') + 'sw.js'
    navigator.serviceWorker.register(url).catch((err) => {
      console.warn('Service worker registration failed:', err)
    })
  })
}
