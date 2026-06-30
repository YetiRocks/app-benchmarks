import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Mount-agnostic bundle (founder-locked architecture):
//
// The compiled web assets ride embedded in a SIGNED, content-addressed wasm
// component, so the artifact must be BYTE-IDENTICAL wherever it runs — its
// digest + cosign signature are preserved. The deployment mount is therefore
// applied at SERVE time by the Yeti web server (the router injects
// `<base href="{mount}/">` + `window.__YETI_BASE_PATH` into the served
// index.html), never baked at build time.
//
//   - `base: './'` emits RELATIVE asset URLs and makes Vite's lazy chunks
//     self-resolve against their own URL at any mount/depth.
//   - `__STATIC_ROOT__` (the routing/API base, no trailing slash) becomes a
//     RUNTIME read of `window.__YETI_BASE_PATH` (fallback `/`). With no
//     injector (`npm run dev`) the global is undefined → `/`, so assets resolve
//     at the dev root and HMR is intact.
//
// There is no build-time base env: the same bundle serves at a prod subdomain
// root AND locally under a nested path, with no per-mount rebuild.

// `__STATIC_ROOT__` is mapped to a runtime global (a dotted entity name — the
// only non-literal form esbuild `define` accepts). The served index.html sets
// `window.__YETI_STATIC_ROOT__` in <head> (computed from the host-injected
// `window.__YETI_BASE_PATH`, no trailing slash) before any module evaluates, so
// every existing `__STATIC_ROOT__` call site reads the mount at runtime with no
// source change. Absent (`npm run dev`) → `/`.
const RUNTIME_STATIC_ROOT = 'window.__YETI_STATIC_ROOT__'
const RESOURCES_ROOT = 'api'

// Read yeti server port from ~/yeti/yeti-config.yaml (3 levels up from this file)
const __dir = dirname(fileURLToPath(import.meta.url))
const yetiYaml = readFileSync(resolve(__dir, '../../../yeti-config.yaml'), 'utf-8')
const YETI_PORT = parseInt(yetiYaml.match(/^port:\s*(\d+)/m)?.[1] ?? '9996', 10)

export default defineConfig({
  // Relative base: assets resolve against the served <base href>, byte-identical
  // at every mount.
  base: './',
  define: {
    // Runtime base reads (NOT build-time constants): each expands to a JS
    // expression evaluated in the browser against window.__YETI_BASE_PATH.
    __STATIC_ROOT__: RUNTIME_STATIC_ROOT,
    __RESOURCES_ROOT__: JSON.stringify(RESOURCES_ROOT),
  },
  server: {
    proxy: {
      // Dev proxy: at the dev root the API lives under `/api`.
      [`/${RESOURCES_ROOT}`]: {
        target: `https://localhost:${YETI_PORT}`,
        changeOrigin: true,
        secure: false,
      },
    },
  },
  plugins: [react()],
  build: { outDir: '../web', emptyOutDir: true },
})
