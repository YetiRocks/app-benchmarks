/** Compact numeric formatting: 1.2M / 12k / 999. */
export function formatNumber(n: number): string {
  if (n >= 1000000) {
    const v = n / 1000000
    return (v % 1 === 0 ? v.toFixed(0) : v.toFixed(1)) + 'M'
  }
  if (n >= 10000) return Math.round(n / 1000) + 'k'
  if (n >= 1000) {
    const v = n / 1000
    const s = v.toFixed(1)
    return (s.endsWith('.0') ? s.slice(0, -2) : s) + 'k'
  }
  return n.toFixed(0)
}

/** Latency formatting in ms. Returns '-' for zero/missing. */
export function formatMs(n: number): string {
  if (n === 0) return '-'
  if (n >= 100) return n.toFixed(0)
  if (n >= 10) return n.toFixed(1)
  return n.toFixed(2)
}

/** Compact timestamp: MM.DD HH:MM in local time. */
export function formatDate(timestamp: string): string {
  const d = new Date(timestamp)
  const month = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  const hours = String(d.getHours()).padStart(2, '0')
  const minutes = String(d.getMinutes()).padStart(2, '0')
  return `${month}.${day} ${hours}:${minutes}`
}
