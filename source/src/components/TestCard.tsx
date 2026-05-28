import { TestDef, LatestResult } from '../types'
import { formatNumber, formatMs } from '../utils'

interface TestCardProps {
  test: TestDef
  /** Fallback when `test.vus` is absent. The bestresults API only
   * sends `vus` for tests that override the platform default. */
  defaultVus: number
  latest?: LatestResult
  phase: string
  isDisabled: boolean
  warmupSecs: number
  elapsedSecs: number
  configuredDuration: number
  onRun: () => void
  onOpenHistory: () => void
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

export function TestCard({ test, defaultVus, latest, phase, isDisabled, warmupSecs, elapsedSecs, configuredDuration, onRun, onOpenHistory }: TestCardProps) {
  const results = latest?.results
  const hasData = !!(results && results.throughput)
  const isRealtimeTest = test.binary === 'load-realtime' && test.id !== 'ws-publish'

  const cardClass = ['metric-card bench-card',
    phase === 'seeding' ? 'bench-seeding' : '',
    phase === 'warming' ? 'bench-warming' : '',
    phase === 'running' ? 'bench-running' : '',
    phase === 'cleaning' ? 'bench-cleaning' : '',
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
            {hasData && results!.peakConnections != null ? formatNumber(results!.peakConnections!) : formatNumber(test.vus ?? defaultVus)}
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
