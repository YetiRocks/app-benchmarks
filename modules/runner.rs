use crate::common::LoadTestConfig;
use crate::metrics::{Metrics, Snapshot, SnapshotCollector};
use reqwest::Client;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;

pub struct ScenarioContext {
    pub client: Client,
    pub base_url: String,
    pub auth_user: String,
    pub auth_pass: String,
    pub metrics: Arc<Metrics>,
    pub vu_id: u64,
    /// Per-VU request counter for distributing IDs across the full pool.
    pub request_counter: AtomicU64,
}

/// Run a load test: spawn `vus` tasks, each looping `scenario_fn` for `warmup + duration`.
/// Metrics are discarded during the warmup phase.
/// Returns the shared Metrics, measured elapsed duration, and per-second snapshots.
pub async fn run_load_test<F, Fut>(
    vus: u64,
    duration: Duration,
    warmup: Duration,
    config: LoadTestConfig,
    scenario_fn: F,
) -> (Arc<Metrics>, f64, Vec<Snapshot>)
where
    F: Fn(Arc<ScenarioContext>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    let LoadTestConfig {
        client,
        base_url,
        auth_user,
        auth_pass,
    } = config;
    let metrics = Arc::new(Metrics::new());
    metrics.active_vus.store(vus, Ordering::Relaxed);
    let scenario_fn = Arc::new(scenario_fn);
    let total_duration = warmup + duration;
    let deadline = Instant::now() + total_duration;

    let mut join_set = JoinSet::new();

    for vu_id in 0..vus {
        let ctx = Arc::new(ScenarioContext {
            client: client.clone(),
            base_url: base_url.clone(),
            auth_user: auth_user.clone(),
            auth_pass: auth_pass.clone(),
            metrics: metrics.clone(),
            vu_id,
            request_counter: AtomicU64::new(0),
        });
        let sf = scenario_fn.clone();

        join_set.spawn(async move {
            while Instant::now() < deadline {
                sf(ctx.clone()).await;
            }
        });
    }

    // Warmup phase
    if !warmup.is_zero() {
        tracing::info!("Warmup: {}s (metrics discarded)...", warmup.as_secs());
        tokio::time::sleep(warmup).await;
    }

    // Flip to measuring mode
    metrics.set_warming(false);
    let measure_start = Instant::now();

    // Start snapshot collection
    let collector = SnapshotCollector::new(metrics.clone());
    let collector_handle = collector.start();

    // Wait for all VUs to finish
    while join_set.join_next().await.is_some() {}

    // Collect snapshots
    let snapshots = collector.finish();
    let _ = collector_handle.await;

    let elapsed = measure_start.elapsed().as_secs_f64();
    (metrics, elapsed, snapshots)
}

/// Ramp test parameters.
pub struct RampConfig {
    pub start_vus: u64,
    pub step_vus: u64,
    pub step_interval: Duration,
    pub max_vus: u64,
}

/// Run a ramp test: start with `start_vus`, add `step_vus` every `step_interval` up to `max_vus`.
/// Returns the shared Metrics, measured elapsed duration, and per-second snapshots.
pub async fn run_ramp_test<F, Fut>(
    duration: Duration,
    warmup: Duration,
    ramp: RampConfig,
    config: LoadTestConfig,
    scenario_fn: F,
) -> (Arc<Metrics>, f64, Vec<Snapshot>)
where
    F: Fn(Arc<ScenarioContext>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    let LoadTestConfig {
        client,
        base_url,
        auth_user,
        auth_pass,
    } = config;
    let metrics = Arc::new(Metrics::new());
    let scenario_fn = Arc::new(scenario_fn);
    let total_duration = warmup + duration;
    let deadline = Instant::now() + total_duration;

    let mut join_set = JoinSet::new();
    let mut vu_id_counter = 0u64;

    // Helper to spawn VUs
    let spawn_vus = |join_set: &mut JoinSet<()>,
                     count: u64,
                     start_id: &mut u64,
                     metrics: &Arc<Metrics>,
                     client: &Client,
                     base_url: &str,
                     auth_user: &str,
                     auth_pass: &str,
                     scenario_fn: &Arc<F>,
                     deadline: Instant| {
        for _ in 0..count {
            let ctx = Arc::new(ScenarioContext {
                client: client.clone(),
                base_url: base_url.to_string(),
                auth_user: auth_user.to_string(),
                auth_pass: auth_pass.to_string(),
                metrics: metrics.clone(),
                vu_id: *start_id,
                request_counter: AtomicU64::new(0),
            });
            *start_id += 1;
            let sf = scenario_fn.clone();
            join_set.spawn(async move {
                while Instant::now() < deadline {
                    sf(ctx.clone()).await;
                }
            });
        }
    };

    // Spawn initial VUs
    let initial = ramp.start_vus.min(ramp.max_vus);
    spawn_vus(
        &mut join_set,
        initial,
        &mut vu_id_counter,
        &metrics,
        &client,
        &base_url,
        &auth_user,
        &auth_pass,
        &scenario_fn,
        deadline,
    );
    let mut current_vus = initial;
    metrics.active_vus.store(current_vus, Ordering::Relaxed);
    tracing::info!(
        "Ramp: started with {} VUs (max={})",
        current_vus,
        ramp.max_vus
    );

    // Warmup phase
    if !warmup.is_zero() {
        tracing::info!("Warmup: {}s (metrics discarded)...", warmup.as_secs());
        tokio::time::sleep(warmup).await;
    }

    // Flip to measuring mode
    metrics.set_warming(false);
    let measure_start = Instant::now();

    // Start snapshot collection
    let collector = SnapshotCollector::new(metrics.clone());
    let collector_handle = collector.start();

    // Ramp loop: add VUs at intervals
    let mut ramp_interval = tokio::time::interval(ramp.step_interval);
    ramp_interval.tick().await; // first tick is immediate

    loop {
        tokio::select! {
            _ = ramp_interval.tick() => {
                if current_vus < ramp.max_vus && Instant::now() < deadline {
                    let to_add = ramp.step_vus.min(ramp.max_vus - current_vus);
                    spawn_vus(
                        &mut join_set, to_add, &mut vu_id_counter,
                        &metrics, &client, &base_url, &auth_user, &auth_pass,
                        &scenario_fn, deadline,
                    );
                    current_vus += to_add;
                    metrics.active_vus.store(current_vus, Ordering::Relaxed);
                    tracing::info!("Ramp: {} VUs active", current_vus);
                }
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                break;
            }
        }

        if Instant::now() >= deadline {
            break;
        }
    }

    // Wait for all VUs to finish
    while join_set.join_next().await.is_some() {}

    // Collect snapshots
    let snapshots = collector.finish();
    let _ = collector_handle.await;

    let elapsed = measure_start.elapsed().as_secs_f64();
    (metrics, elapsed, snapshots)
}

impl ScenarioContext {
    /// Get the next request index for this VU, cycling through the full range.
    pub fn next_request_idx(&self) -> u64 {
        self.request_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Record an HTTP response, checking the status code.
    /// Only 2xx responses are counted as successes; all others are errors.
    pub async fn record_response(
        &self,
        start: std::time::Instant,
        result: Result<reqwest::Response, reqwest::Error>,
    ) {
        match result {
            Ok(resp) => {
                let status = resp.status();
                let bytes = resp.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
                let latency = start.elapsed().as_micros() as u64;
                if status.is_success() {
                    self.metrics.record_success(latency, bytes);
                } else {
                    self.metrics.record_error();
                }
            },
            Err(_) => self.metrics.record_error(),
        }
    }

    /// Record a batch HTTP response as N individual records.
    /// Latency is divided by batch_size (amortized per-record cost).
    /// Total/throughput counts each record in the batch.
    pub async fn record_batch_response(
        &self,
        start: std::time::Instant,
        result: Result<reqwest::Response, reqwest::Error>,
        batch_size: u64,
    ) {
        match result {
            Ok(resp) => {
                let status = resp.status();
                let bytes = resp.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
                let latency = start.elapsed().as_micros() as u64;
                if status.is_success() {
                    let per_record_latency = latency / batch_size;
                    let per_record_bytes = bytes / batch_size;
                    for _ in 0..batch_size {
                        self.metrics.record_success(per_record_latency, per_record_bytes);
                    }
                } else {
                    self.metrics.record_error();
                }
            },
            Err(_) => self.metrics.record_error(),
        }
    }
}
