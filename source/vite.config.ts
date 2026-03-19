import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'fs'
import { resolve } from 'path'

// Read base path from yeti config.yaml automatically.
// Uses route_prefix if set, otherwise /{app_id}/.
function readYetiBasePath(): string {
  // Allow env override (for CI or manual builds)
  if (process.env.YETI_BASE_PATH) return process.env.YETI_BASE_PATH

  try {
    const configPath = resolve(__dirname, '../config.yaml')
    const config = readFileSync(configPath, 'utf-8')

    // Check route_prefix
    const prefixMatch = config.match(/^route_prefix:\s*["']?([^"'\n]+)["']?/m)
    if (prefixMatch) {
      const prefix = prefixMatch[1].trim()
      return prefix === '/' ? '/' : `${prefix.replace(/\/+$/, '')}/`
    }

    // Fall back to app_id
    const idMatch = config.match(/^app_id:\s*["']?([^"'\n]+)["']?/m)
    if (idMatch) return `/${idMatch[1].trim()}/`
  } catch { /* config not found - use default */ }

  return './'
}

const basePath = readYetiBasePath()

export default defineConfig({
  base: basePath,
  define: {
    __BASENAME__: JSON.stringify(basePath.replace(/\/$/, '') || '/'),
  },
  plugins: [react()],
  build: {
    outDir: '../web',
    emptyOutDir: true,
  },
  server: {
    fs: { allow: ['..'] },
    port: 5180,
  },
})
