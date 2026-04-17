import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const STATIC_ROOT = '/app-benchmarks'
const RESOURCES_ROOT = 'api'

const __dir = dirname(fileURLToPath(import.meta.url))
const yetiYaml = readFileSync(resolve(__dir, '../../../yeti-config.yaml'), 'utf-8')
const YETI_PORT = parseInt(yetiYaml.match(/^port:\s*(\d+)/m)?.[1] ?? '9996', 10)

export default defineConfig({
  base: `${STATIC_ROOT}/`,
  define: {
    __STATIC_ROOT__: JSON.stringify(STATIC_ROOT),
    __RESOURCES_ROOT__: JSON.stringify(RESOURCES_ROOT),
  },
  server: {
    proxy: {
      [`${STATIC_ROOT}/${RESOURCES_ROOT}`]: {
        target: `https://localhost:${YETI_PORT}`,
        changeOrigin: true,
        secure: false,
      },
    },
  },
  plugins: [react()],
  build: { outDir: '../web', emptyOutDir: true },
})
