// Types matching the app-benchmarks REST API responses

export interface TestDef {
  id: string
  name: string
  binary: string
  duration: number
  vus: number
  category: string
}

export interface CategoryDef {
  category: string
  label: string
}

export interface LatestResult {
  name: string
  throughput: number
  run: Record<string, unknown>
  results: {
    throughput?: number
    p50?: number
    p95?: number
    p99?: number
    p999?: number
    total?: number
    errors?: number
    cv?: number
    summary?: string
    peakConnections?: number
    connectionFailures?: number
    published?: number
  } | null
}

export interface RunnerState {
  status: 'idle' | 'seeding' | 'warming' | 'running' | 'cleaning'
  phase?: 'idle' | 'seeding' | 'warming' | 'running' | 'cleaning'
  testName?: string
  startedAt?: number
  warmupSecs?: number
  elapsedSecs?: number
  configuredDuration?: number
  lastError?: string
  tests?: TestDef[]
  categories?: CategoryDef[]
  targetUrl?: string
}

export interface HistoryRun {
  id: string
  testName: string
  timestamp: string
  durationSecs: number
  clients?: number
  results: string      // JSON string
  summary: string
  snapshots?: string   // JSON string
}

export interface Snapshot {
  second: number
  rps: number
  p50_ms: number
  p95_ms: number
  p99_ms: number
  p999_ms: number
  errors: number
  active_vus: number
}
