import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'
import '@picocss/pico/css/pico.min.css'
import './styles.css'

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
