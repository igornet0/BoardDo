import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// SmartDo address — `make dev LISTEN_ADDR=...` forwards it here.
const backend = process.env.BOARDDO_BACKEND ?? '127.0.0.1:8080'

const proxy = {
  '/api': `http://${backend}`,
  '/ws': {
    target: `ws://${backend}`,
    ws: true,
  },
}

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, proxy },
  preview: { port: 4173, proxy },
})
