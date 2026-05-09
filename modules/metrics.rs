use hdrhistogram::Histogram;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Number of histogram shards. Power of 2 so we can mask instead of mod.
/// 16 covers typical CPU counts; with 100+ VUs hitting `record_success`
/// concurrently, contention drops by ~16× vs. a single mutex.
const NUM_SHARDS: usize = 16;
const SHARD_MASK: usize = NUM_SHARDS - 1;

fn new_hist() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 60_000_000, 3).expect("failed to create histogram")
}

fn new_shards() -> Vec<Mutex<Histogram<u64>>> {
    (0..NUM_SHARDS).map(|_| Mutex::new(new_hist())).collect()
}

/// Merge all shard histograms into one. If `drain` is true, each shard is
/// reset after merging — used by per-second snapshots.
fn merge_shards(shards: &[Mutex<Histogram<u64>>], drain: bool) -> Histogram<u64> {
    let mut merged = new_hist();
    for shard in shards {
        if let Ok(mut h) = shard.lock() {
            let _ = merged.add(h.clone());
            if drain {
                h.reset();
            }
        }
    }
    merged
}

pub struct Metrics {
    pub total_requests: AtomicU64,
    /// Total individual records inserted/processed. For non-batch tests this
    /// equals `total_requests`; for batch tests one HTTP request carries N
    /// records and `total_records` is bumped by N. Throughput in `summary()`
    /// uses this counter so records/sec stays comparable across tests.
    pub total_records: AtomicU64,
    pub total_errors: AtomicU64,
    pub total_bytes: AtomicU64,
    /// Round-robin shard index, advanced once per recorded sample. Avoids
    /// thread-id lookups on the hot path while still distributing writes
    /// across shards.
    shard_counter: AtomicUsize,
    latency_shards: Vec<Mutex<Histogram<u64>>>,
    interval_shards: Vec<Mutex<Histogram<u64>>>,
    is_warming: AtomicBool,
    interval_requests: AtomicU64,
    interval_errors: AtomicU64,
    /// Current number of active virtual users (for ramp tests)
    pub active_vus: AtomicU64,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_records: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            shard_counter: AtomicUsize::new(0),
            latency_shards: new_shards(),
            interval_shards: new_shards(),
            is_warming: AtomicBool::new(true),
            interval_requests: AtomicU64::new(0),
            interval_errors: AtomicU64::new(0),
            active_vus: AtomicU64::new(0),
        }
    }

    /// Set the warming state. While warming, record_success/record_error are no-ops.
    pub fn set_warming(&self, warming: bool) {
        self.is_warming.store(warming, Ordering::Release);
    }

    pub fn is_warming(&self) -> bool {
        self.is_warming.load(Ordering::Acquire)
    }

    /// Record `latency_us` into both the cumulative and interval histograms,
    /// using a shard chosen by round-robin so the lock is rarely contended.
    fn record_latency(&self, latency_us: u64) {
        let idx = self.shard_counter.fetch_add(1, Ordering::Relaxed) & SHARD_MASK;
        if let Ok(mut h) = self.latency_shards[idx].lock() {
            let _ = h.record(latency_us);
        }
        if let Ok(mut h) = self.interval_shards[idx].lock() {
            let _ = h.record(latency_us);
        }
    }

    pub fn record_success(&self, latency_us: u64, bytes: u64) {
        if self.is_warming.load(Ordering::Relaxed) {
            return;
        }
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_records.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.interval_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency_us);
    }

    /// Record a successful batch response.
    ///
    /// Unlike `record_success` × N, this records ONE histogram entry (with the
    /// per-record amortized latency) and bumps `interval_requests` by 1. Why:
    /// per-second snapshots count *response arrivals*, not records, so a batch
    /// of 100 records doesn't manufacture a 100-RPS spike that inflates CV.
    /// `total_records` still counts every record so displayed throughput
    /// (records/sec) stays comparable across batch and non-batch tests.
    pub fn record_batch_success(
        &self,
        per_record_latency_us: u64,
        total_bytes: u64,
        batch_size: u64,
    ) {
        if self.is_warming.load(Ordering::Relaxed) {
            return;
        }
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_records.fetch_add(batch_size, Ordering::Relaxed);
        self.total_bytes.fetch_add(total_bytes, Ordering::Relaxed);
        self.interval_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(per_record_latency_us);
    }

    pub fn record_error(&self) {
        if self.is_warming.load(Ordering::Relaxed) {
            return;
        }
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_errors.fetch_add(1, Ordering::Relaxed);
        self.interval_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Swap and drain the interval histogram, returning a Snapshot for the last interval.
    pub fn take_interval_snapshot(&self, second: u32) -> Snapshot {
        let reqs = self.interval_requests.swap(0, Ordering::Relaxed);
        let errs = self.interval_errors.swap(0, Ordering::Relaxed);
        let vus = self.active_vus.load(Ordering::Relaxed);

        let merged = merge_shards(&self.interval_shards, /* drain */ true);
        let (p50, p95, p99, p999) = (
            merged.value_at_quantile(0.50) as f64 / 1000.0,
            merged.value_at_quantile(0.95) as f64 / 1000.0,
            merged.value_at_quantile(0.99) as f64 / 1000.0,
            merged.value_at_quantile(0.999) as f64 / 1000.0,
        );

        Snapshot {
            second,
            rps: reqs as f64,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            p999_ms: p999,
            errors: errs,
            active_vus: vus,
        }
    }

    pub fn summary(&self, duration_secs: f64) -> MetricsSummary {
        let requests = self.total_requests.load(Ordering::Relaxed);
        let records = self.total_records.load(Ordering::Relaxed);
        let errors = self.total_errors.load(Ordering::Relaxed);
        let bytes = self.total_bytes.load(Ordering::Relaxed);
        // Throughput is records/sec for batch tests, requests/sec for others.
        // `total_records` tracks both, so we can use it uniformly.
        let total = if records > 0 { records } else { requests };
        let throughput = if duration_secs > 0.0 {
            total as f64 / duration_secs
        } else {
            0.0
        };

        let merged = merge_shards(&self.latency_shards, /* drain */ false);
        let (p50_ms, p95_ms, p99_ms, p999_ms) = (
            merged.value_at_quantile(0.50) as f64 / 1000.0,
            merged.value_at_quantile(0.95) as f64 / 1000.0,
            merged.value_at_quantile(0.99) as f64 / 1000.0,
            merged.value_at_quantile(0.999) as f64 / 1000.0,
        );

        MetricsSummary {
            throughput,
            p50_ms,
            p95_ms,
            p99_ms,
            p999_ms,
            total,
            errors,
            total_bytes: bytes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub throughput: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub total: u64,
    pub errors: u64,
    pub total_bytes: u64,
}

impl MetricsSummary {
    pub fn format_summary(&self, duration_secs: f64) -> String {
        format!(
            "{} requests in {:.0}s ({:.1} req/s), p50={:.2}ms p95={:.2}ms p99={:.2}ms p99.9={:.2}ms, {} errors",
            format_count(self.total),
            duration_secs,
            self.throughput,
            self.p50_ms,
            self.p95_ms,
            self.p99_ms,
            self.p999_ms,
            self.errors,
        )
    }
}

#[derive(Clone, serde::Serialize)]
pub struct Snapshot {
    pub second: u32,
    pub rps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub errors: u64,
    pub active_vus: u64,
}

/// Collect per-second snapshots from a Metrics instance.
/// Call `start()` to begin background collection, then `finish()` to stop and retrieve snapshots.
pub struct SnapshotCollector {
    metrics: Arc<Metrics>,
    snapshots: Arc<Mutex<Vec<Snapshot>>>,
    stop: Arc<AtomicBool>,
}

impl SnapshotCollector {
    pub fn new(metrics: Arc<Metrics>) -> Self {
        Self {
            metrics,
            snapshots: Arc::new(Mutex::new(Vec::new())),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawn a background tokio task that collects a snapshot every second.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let metrics = self.metrics.clone();
        let snapshots = self.snapshots.clone();
        let stop = self.stop.clone();

        tokio::spawn(async move {
            let mut second = 0u32;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.tick().await; // first tick is immediate

            loop {
                interval.tick().await;
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                second += 1;
                let snap = metrics.take_interval_snapshot(second);
                if let Ok(mut v) = snapshots.lock() {
                    v.push(snap);
                }
            }
        })
    }

    /// Stop collection and return all collected snapshots.
    pub fn finish(&self) -> Vec<Snapshot> {
        self.stop.store(true, Ordering::Release);
        self.snapshots.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
