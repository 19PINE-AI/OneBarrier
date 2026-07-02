import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  // relative base so the site works when served from a subpath
  // (deployed at https://ring0.me/research/OneBarrier/)
  base: './',
  plugins: [react()],
})
