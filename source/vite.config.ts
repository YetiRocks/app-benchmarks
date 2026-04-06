import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  base: './',
  define: {
    RESOURCE_ROUTE: JSON.stringify(process.env.RESOURCE_ROUTE || '/api'),
    STATIC_ROUTE: JSON.stringify(process.env.STATIC_ROUTE || '/'),
  },
  plugins: [react()],
  build: { outDir: '../web', emptyOutDir: true },
})
