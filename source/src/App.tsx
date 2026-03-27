import { useState, useEffect, useCallback, useRef } from 'react'
import { api } from './api'
import { TestDef, CategoryDef, LatestResult, RunnerState, HistoryRun, Snapshot } from './types'
import { BenchmarkChart } from './components/BenchmarkChart'

// ── Helpers ──

function formatNumber(n: number): string {
  if (n >= 1000000) { const v = n / 1000000; return (v % 1 === 0 ? v.toFixed(0) : v.toFixed(1)) + 'M' }
  if (n >= 10000) return Math.round(n / 1000) + 'k'
  if (n >= 1000) { const v = n / 1000; const s = v.toFixed(1); return (s.endsWith('.0') ? s.slice(0, -2) : s) + 'k' }
  return n.toFixed(0)
}


function formatMs(n: number): string {
  if (n === 0) return '-'
  if (n >= 100) return n.toFixed(0)
  if (n >= 10) return n.toFixed(1)
  return n.toFixed(2)
}

function formatDate(timestamp: string): string {
  const d = new Date(timestamp)
  const month = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  const hours = String(d.getHours()).padStart(2, '0')
  const minutes = String(d.getMinutes()).padStart(2, '0')
  return `${month}.${day} ${hours}:${minutes}`
}

function ListIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
      <line x1="5" y1="3" x2="14" y2="3" /><line x1="5" y1="8" x2="14" y2="8" /><line x1="5" y1="13" x2="14" y2="13" />
      <circle cx="2" cy="3" r="1" fill="currentColor" stroke="none" />
      <circle cx="2" cy="8" r="1" fill="currentColor" stroke="none" />
      <circle cx="2" cy="13" r="1" fill="currentColor" stroke="none" />
    </svg>
  )
}

// ── App ──

function useAuth() {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null)
  useEffect(() => {
    fetch('/yeti-auth/oauth_user', { credentials: 'same-origin' })
      .then(r => r.ok ? r.json() : null)
      .then(data => setAuthenticated(!!(data?.user)))
      .catch(() => setAuthenticated(false))
  }, [])
  return authenticated
}

function LoginPage() {
  const handleLogin = () => {
    window.location.href = `/yeti-auth/oauth_login?provider=google&redirect_uri=/app-benchmarks/&app_id=app-benchmarks`
  }
  return (
    <div className="login-page">
      <div className="login-card">
        <img src={`${import.meta.env.BASE_URL}logo_white.svg`} alt="Yeti" className="login-logo" />
        <button className="btn btn-oauth btn-google" onClick={handleLogin}>
          <svg viewBox="0 0 24 24" style={{ width: 20, height: 20 }}>
            <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"/>
            <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"/>
            <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"/>
            <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"/>
          </svg>
          Sign in with Google
        </button>
      </div>
    </div>
  )
}

function BenchmarkApp() {
  // tests keyed by id, with best metrics if available
  const [testMap, setTestMap] = useState<Record<string, TestDef & { best?: Record<string, unknown> }>>({})
  const [categories, setCategories] = useState<CategoryDef[]>([])
  const [runner, setRunner] = useState<RunnerState>({ status: 'idle' })
  const [historyModal, setHistoryModal] = useState<{ testId: string; testName: string; isRealtimeTest: boolean } | null>(null)
  const [history, setHistory] = useState<HistoryRun[]>([])
  const [error, setError] = useState<string | null>(null)
  const [showDeleteAll, setShowDeleteAll] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const isValidUrl = (url: string) => { try { const u = new URL(url); return u.protocol === 'https:' || u.protocol === 'http:' } catch { return false } }
  const commitUrl = () => { if (isValidUrl(targetUrl)) { setSavedUrl(targetUrl); localStorage.setItem('bench-target-url', targetUrl) } }

  const [savedUrl, setSavedUrl] = useState(() => {
    const stored = localStorage.getItem('bench-target-url')
    return stored && stored.length > 0 ? stored : `${window.location.protocol}//${window.location.host}`
  })
  const [targetUrl, setTargetUrl] = useState(savedUrl)
  const [historyView, setHistoryView] = useState<'table' | 'chart'>('table')
  const [chartSnapshots, setChartSnapshots] = useState<Snapshot[]>([])
  const pollRef = useRef<number | null>(null)

  const fetchBestResults = useCallback(async () => {
    try {
      const resp = await api('/bestresults')
      if (resp.ok) {
        const data = await resp.json()
        setTestMap(data.tests || {})
        if (data.categories?.length) setCategories(data.categories)
      }
    } catch { /* Server may not be ready */ }
  }, [])

  const fetchRunnerState = useCallback(async () => {
    try {
      const resp = await api('/runner')
      if (resp.ok) {
        const data = await resp.json()
        const phase = data.phase || data.status || 'idle'
        const state: RunnerState = {
          status: data.status || 'idle', phase,
          testName: data.testName, warmupSecs: data.warmupSecs,
          elapsedSecs: data.elapsedSecs, configuredDuration: data.configuredDuration,
          lastError: data.lastError,
        }

        const isOverdue = state.phase === 'running'
          && (state.configuredDuration ?? 0) > 0
          && (state.elapsedSecs ?? 0) > (state.configuredDuration ?? 0) + 10
        if (isOverdue) { state.status = 'idle'; state.phase = 'idle' }

        setRunner(state)

        if (state.status === 'idle') {
          if (pollRef.current) {
            clearInterval(pollRef.current); pollRef.current = null
            fetchBestResults()
            if (state.lastError) setError(state.lastError)
          }
        } else if (!pollRef.current) {
          pollRef.current = window.setInterval(fetchRunnerState, 1000)
        }
      }
    } catch { /* Server may not be ready */ }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fetchBestResults])

  const fetchHistory = useCallback(async (testId: string) => {
    try {
      const resp = await api(`/history/${testId}`)
      if (resp.ok) {
        const data = await resp.json()
        setHistory(data.runs || [])
      }
    } catch { setHistory([]) }
  }, [])

  useEffect(() => {
    fetchBestResults(); fetchRunnerState()
    return () => { if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null } }
  }, [fetchBestResults, fetchRunnerState])

  const startTest = async (testId: string) => {
    setError(null)
    // Optimistic UI — show seeding immediately, don't wait for server
    setRunner({ status: 'seeding', phase: 'seeding', testName: testId, startedAt: Date.now() / 1000 })
    if (pollRef.current) clearInterval(pollRef.current)
    pollRef.current = window.setInterval(fetchRunnerState, 1000)
    try {
      const resp = await api('/runner', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ test: testId, targetUrl: savedUrl, vus: testMap[testId]?.vus }),
      })
      if (!resp.ok) {
        const data = await resp.json().catch(() => null)
        setError(data?.error || 'Failed to start test')
        setRunner({ status: 'idle' })
        if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null }
      }
    } catch (e) {
      setError(`Connection error: ${e}`)
      setRunner({ status: 'idle' })
      if (pollRef.current) { clearInterval(pollRef.current); pollRef.current = null }
    }
  }

  const openHistory = (testId: string, testName: string, isRealtimeTest: boolean) => {
    setHistoryModal({ testId, testName, isRealtimeTest }); setHistoryView('table'); fetchHistory(testId)
  }

  const deleteAllResults = async () => {
    setDeleting(true)
    try {
      const resp = await api('/bestresults', { method: 'DELETE' })
      if (resp.ok) { fetchBestResults(); setShowDeleteAll(false) }
    } catch { /* silent */ }
    finally { setDeleting(false) }
  }

  const effectivePhase = runner.phase ?? runner.status
  const isBusy = effectivePhase !== 'idle'
  const runningTest = isBusy ? runner.testName : null
  const tests = Object.values(testMap).sort((a, b) => ((a as any).order ?? 0) - ((b as any).order ?? 0))
  const testsWithResults = tests.filter(t => t.best).length

  return (
    <div className="app">
      <nav className="nav">
        <div className="nav-left">
          <a href="/"><img src={`${import.meta.env.BASE_URL}logo_white.svg`} alt="Yeti" className="nav-logo" /></a>
        </div>
        <span className="nav-title">Benchmarks</span>
        <div className="nav-right" />
      </nav>
      <main className="page benchmarks-panel">
      <div className="panel-header">
        <span className="panel-header-label">{testsWithResults} of {tests.length} tests with results</span>
        <div className="target-form-group">
          <input
            className="search-input target-input" type="text" value={targetUrl}
            onChange={e => setTargetUrl(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter' && isValidUrl(targetUrl)) commitUrl() }}
            placeholder="Benchmark target URL" title="Target URL for benchmark load tests" disabled={isBusy}
          />
          <button
            className={`target-btn ${targetUrl === savedUrl ? 'target-unchanged' : isValidUrl(targetUrl) ? 'target-valid' : 'target-invalid'}`}
            disabled={isBusy || targetUrl === savedUrl || !isValidUrl(targetUrl)}
            onClick={commitUrl}
            title={targetUrl === savedUrl ? 'Target URL' : isValidUrl(targetUrl) ? 'Apply target URL' : 'Invalid URL'}
          >
            {targetUrl === savedUrl ? (
              <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M11.05 3H12.95C13.53 3 14 3.47 14 4.05V12.95C14 13.53 13.53 14 12.95 14H3.05C2.47 14 2 13.53 2 12.95V4.05C2 3.47 2.47 3 3.05 3H4.95L5.45 2H10.55L11.05 3ZM8 12C9.66 12 11 10.66 11 9C11 7.34 9.66 6 8 6C6.34 6 5 7.34 5 9C5 10.66 6.34 12 8 12Z"/></svg>
            ) : isValidUrl(targetUrl) ? (
              <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z"/></svg>
            ) : (
              <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor"><path d="M3.72 3.72a.75.75 0 011.06 0L8 6.94l3.22-3.22a.75.75 0 111.06 1.06L9.06 8l3.22 3.22a.75.75 0 11-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 01-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 010-1.06z"/></svg>
            )}
          </button>
        </div>
        <button className="btn btn-sm" disabled={isBusy || testsWithResults === 0}
          onClick={() => setShowDeleteAll(true)}>Delete All</button>
      </div>

      <div className="benchmarks-content">
        {error && <div className="bench-error">{error}</div>}

        {categories.map(section => (
          <div key={section.category} className="bench-section">
            <div className="bench-section-header">
              <div className="bench-section-label">{section.label}</div>
            </div>
            <div className="metrics-grid bench-grid">
              {tests.filter(t => t.category === section.category).map(test => {
                const isThisTest = runningTest === test.id
                const best = test.best as LatestResult['results'] | undefined
                return (
                  <TestCard key={test.id} test={test} latest={best ? { name: test.id, throughput: 0, run: {}, results: best } : undefined}
                    phase={isThisTest ? (runner.phase ?? runner.status) : 'idle'}
                    isDisabled={isBusy && !isThisTest}
                    warmupSecs={isThisTest ? (runner.warmupSecs ?? 0) : 0}
                    elapsedSecs={isThisTest ? (runner.elapsedSecs ?? 0) : 0}
                    configuredDuration={isThisTest ? (runner.configuredDuration ?? 0) : 0}
                    onRun={() => startTest(test.id)}
                    onOpenHistory={() => openHistory(test.id, test.name, test.binary === 'load-realtime' && test.id !== 'ws-publish')} />
                )
              })}
            </div>
          </div>
        ))}
      </div>

      {/* History Modal */}
      {historyModal && (
        <div className="bench-modal-overlay" onClick={() => { setHistoryModal(null); setHistory([]); setChartSnapshots([]) }}>
          <div className="bench-modal" onClick={e => e.stopPropagation()} style={{ maxWidth: historyView === 'chart' ? '750px' : undefined }}>
            <div className="bench-modal-header">
              <span className="bench-modal-title">{historyModal.testName} — {historyView === 'chart' ? 'Chart' : `Run History (${history.length})`}</span>
              <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                {historyView === 'chart' && <button className="btn btn-sm" onClick={() => setHistoryView('table')}>Back</button>}
                <button className="bench-modal-close" onClick={() => { setHistoryModal(null); setHistory([]); setChartSnapshots([]) }}>&times;</button>
              </div>
            </div>
            <div className="bench-modal-body">
              {historyView === 'chart' ? (
                <BenchmarkChart snapshots={chartSnapshots} width={680} height={300} />
              ) : history.length === 0 ? (
                <div className="empty-state">No runs recorded yet</div>
              ) : (
                <table className="data-table">
                  <thead><tr>
                    <th>Date</th>
                    <th>Clients</th>
                    <th>{historyModal.isRealtimeTest ? 'msg/s' : 'RPS'}</th>
                    <th>{historyModal.isRealtimeTest ? 'total' : 'p95'}</th>
                    <th>{historyModal.isRealtimeTest ? 'loss' : 'CV %'}</th>
                    <th>Chart</th>
                  </tr></thead>
                  <tbody>
                    {history.map(run => {
                      let parsed: Record<string, number> = {}; try { parsed = JSON.parse(run.results || '{}') } catch { /* ignore */ }
                      const isRT = historyModal.isRealtimeTest
                      return (
                        <tr key={run.id}>
                          <td>{formatDate(run.timestamp)}</td>
                          <td>{parsed.peakConnections != null ? formatNumber(parsed.peakConnections) : run.clients ? formatNumber(run.clients) : '-'}</td>
                          <td>{formatNumber(parsed.throughput ?? 0)}</td>
                          <td>{isRT ? formatNumber(parsed.total ?? 0) : (parsed.p95 != null ? formatMs(parsed.p95) : '-')}</td>
                          <td>{isRT
                            ? (parsed.total && parsed.peakConnections && parsed.published
                              ? `${(100 - (parsed.total / (parsed.peakConnections * parsed.published)) * 100).toFixed(1)}%`
                              : '-')
                            : (parsed.cv != null ? parsed.cv.toFixed(1) : '-')}</td>
                          <td>{run.snapshots ? <button className="btn btn-sm" onClick={() => { try { setChartSnapshots(JSON.parse(run.snapshots!)); setHistoryView('chart') } catch { /* ignore */ } }}>View</button> : '-'}</td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Delete All Confirm */}
      {showDeleteAll && (
        <div className="modal-overlay" onClick={() => !deleting && setShowDeleteAll(false)}>
          <div className="modal-content" onClick={e => e.stopPropagation()}>
            <h2 className="modal-title">Delete All Results?</h2>
            <p className="modal-message">This will permanently delete all benchmark results across every test. This cannot be undone.</p>
            <div className="modal-actions">
              <button className="btn" disabled={deleting} onClick={() => setShowDeleteAll(false)}>Cancel</button>
              <button className="btn btn-primary" disabled={deleting} onClick={deleteAllResults}>{deleting ? 'Deleting...' : 'Delete All'}</button>
            </div>
          </div>
        </div>
      )}
    </main>
    </div>
  )
}

// ── TestCard ──

interface TestCardProps {
  test: TestDef; latest?: LatestResult; phase: string; isDisabled: boolean
  warmupSecs: number; elapsedSecs: number; configuredDuration: number
  onRun: () => void; onOpenHistory: () => void
}

function TestCard({ test, latest, phase, isDisabled, warmupSecs, elapsedSecs, configuredDuration, onRun, onOpenHistory }: TestCardProps) {
  const results = latest?.results
  const hasData = !!(results && results.throughput)
  const isRealtimeTest = test.binary === 'load-realtime' && test.id !== 'ws-publish'

  const cardClass = ['metric-card bench-card',
    phase === 'seeding' ? 'bench-seeding' : '', phase === 'warming' ? 'bench-warming' : '',
    phase === 'running' ? 'bench-running' : '', phase === 'cleaning' ? 'bench-cleaning' : '',
  ].filter(Boolean).join(' ')

  return (
    <div className={cardClass}>
      <div className="bench-card-header">
        <div className="metric-name">{test.name}</div>
        <div className="bench-card-actions">
          {hasData && <button className="bench-history-btn" onClick={e => { e.stopPropagation(); onOpenHistory() }} title="View run history"><ListIcon /></button>}
          {phase === 'seeding' ? <span className="bench-timer bench-timer-seeding"><span className="bench-spinner" />Seeding...</span>
          : phase === 'warming' ? <span className="bench-timer bench-timer-warming"><span className="bench-spinner" />Warming {warmupSecs.toFixed(0)}s</span>
          : phase === 'running' ? <span className="bench-timer"><span className="bench-spinner" />{elapsedSecs.toFixed(0)}s / {configuredDuration}s</span>
          : phase === 'cleaning' ? <span className="bench-timer bench-timer-cleaning"><span className="bench-spinner" />Cleaning...</span>
          : <button className="btn btn-sm btn-primary" disabled={isDisabled} onClick={e => { e.stopPropagation(); onRun() }}>Run</button>}
        </div>
      </div>
      <div className="bench-card-stats">
        <div className="bench-stat">
          <span className="bench-stat-value">
            {hasData && results!.peakConnections != null ? formatNumber(results!.peakConnections!) : formatNumber(test.vus)}
          </span>
          <span className="bench-stat-label">VUS</span>
        </div>
        <div className="bench-stat">
          <span className={`bench-stat-value${hasData ? '' : ' bench-stat-empty'}`}>
            {hasData ? formatNumber(results!.throughput ?? 0) : '-'}
          </span>
          <span className="bench-stat-label">{isRealtimeTest ? 'M/S' : 'RPS'}</span>
        </div>
        <div className="bench-stat">
          <span className={`bench-stat-value${hasData ? '' : ' bench-stat-empty'}`}>
            {hasData ? (isRealtimeTest ? formatNumber(results!.total ?? 0) : formatMs(results!.p95 ?? 0)) : '-'}
          </span>
          <span className="bench-stat-label">{isRealtimeTest ? 'TOTAL' : 'P95'}</span>
        </div>
      </div>
    </div>
  )
}

export default function App() {
  const authenticated = useAuth()
  if (authenticated === null) return <div className="empty-state">Loading...</div>
  if (!authenticated) return <LoginPage />
  return <BenchmarkApp />
}
