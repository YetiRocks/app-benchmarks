//! Streamlock varied-token ingress load harness.
//!
//! Generates N synthetic JWT-shaped tokens, selects one per request (uniform
//! or approximate-Zipf), POSTs to `/streamlock/api/ingress`, and emits JSON
//! metrics (RPS, P50/95/99, per-shard client-side distribution).
//!
//! The other `load_*` harnesses in this crate use the shared `BenchArgs` /
//! runner / reporter surface tuned for CRUD tables. Streamlock has its own
//! semantics (200 vs 429 meanings, request-level token header), so this
//! module ships its own `Args` but still reuses `runner::run_load_test` +
//! `metrics::Metrics` for the heavy lifting.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use serde::Serialize;

use crate::common::LoadTestConfig;
use crate::runner;

#[derive(Parser, Debug, Clone)]
#[command(about = "Streamlock varied-token ingress load harness")]
pub struct Args {
    /// Yeti base URL (single value — no comma-separated targets for this harness)
    #[arg(long, default_value = "https://localhost")]
    pub base_url: String,

    /// Number of distinct synthetic tokens to cycle through
    #[arg(long, default_value = "1000")]
    pub tokens: usize,

    /// Token distribution: `uniform` | `zipf`
    #[arg(long, default_value = "uniform")]
    pub distribution: String,

    /// Power-law exponent for `zipf` distribution (1.0 ≈ classic Zipf)
    #[arg(long, default_value = "1.0")]
    pub zipf_s: f64,

    /// Virtual users (concurrency)
    #[arg(long, default_value = "500")]
    pub vus: u64,

    /// Measurement duration in seconds
    #[arg(long, default_value = "30")]
    pub duration: u64,

    /// Warmup duration in seconds (metrics discarded)
    #[arg(long, default_value = "5")]
    pub warmup: u64,

    /// Customer API key sent in the `X-StreamLock-Key` header
    #[arg(long, default_value = "itest-key-1")]
    pub api_key: String,

    /// Content id included in every ingress body
    #[arg(long, default_value = "live-42")]
    pub content_id: String,

    /// Informational RPS target; actual rate is driven by VU count
    #[arg(long)]
    pub rps: Option<u64>,

    /// Optional path to write the final JSON report to (stdout always prints it)
    #[arg(long)]
    pub output: Option<String>,

    /// Client-side shard count for distribution tracking (mirrors streamlock's 128 shards)
    #[arg(long, default_value = "128")]
    pub shards: usize,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub tokens: usize,
    pub distribution: String,
    pub vus: u64,
    pub duration_s: f64,
    pub rps: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub latency_p999_ms: f64,
    pub total_requests: u64,
    pub status_2xx: u64,
    pub status_429: u64,
    pub status_other: u64,
    pub network_errors: u64,
    pub shard_counts: Vec<u64>,
    pub shard_mean: f64,
    pub shard_stddev: f64,
    /// stddev / mean — SL-18 acceptance asserts < 0.2 under a balanced workload.
    pub shard_cv: f64,
}

pub async fn run(args: Args) {
    crate::common::init_tracing();
    let client = crate::client::build_client();

    let tokens: Arc<Vec<String>> = Arc::new((0..args.tokens).map(synth_token).collect());

    let shard_counts: Arc<Vec<AtomicU64>> =
        Arc::new((0..args.shards).map(|_| AtomicU64::new(0)).collect());
    let status_2xx = Arc::new(AtomicU64::new(0));
    let status_429 = Arc::new(AtomicU64::new(0));
    let status_other = Arc::new(AtomicU64::new(0));
    let network_errors = Arc::new(AtomicU64::new(0));

    tracing::info!(
        "load-streamlock: tokens={}, distribution={}, vus={}, duration={}s, base={}",
        args.tokens,
        args.distribution,
        args.vus,
        args.duration,
        args.base_url,
    );

    let content_id = args.content_id.clone();
    let api_key = args.api_key.clone();
    let distribution = args.distribution.clone();
    let zipf_s = args.zipf_s;
    let n_tokens = args.tokens;
    let n_shards = args.shards;

    let tokens_outer = tokens.clone();
    let shards_outer = shard_counts.clone();
    let s2xx = status_2xx.clone();
    let s429 = status_429.clone();
    let sother = status_other.clone();
    let snet = network_errors.clone();

    let scenario = move |ctx: Arc<runner::ScenarioContext>| {
        let tokens = tokens_outer.clone();
        let shards = shards_outer.clone();
        let s2xx = s2xx.clone();
        let s429 = s429.clone();
        let sother = sother.clone();
        let snet = snet.clone();
        let content_id = content_id.clone();
        let api_key = api_key.clone();
        let distribution = distribution.clone();
        async move {
            let idx = match distribution.as_str() {
                "zipf" => zipf_sample(n_tokens, zipf_s),
                _ => uniform_sample(n_tokens),
            };
            let token = &tokens[idx];
            shards[token_shard(token, n_shards)].fetch_add(1, Ordering::Relaxed);

            let body = serde_json::json!({
                "token": token,
                "content_id": content_id,
            });
            let url = format!("{}/streamlock/api/ingress", ctx.base_url);
            let start = Instant::now();
            let result = ctx
                .client
                .post(&url)
                .header("X-StreamLock-Key", &api_key)
                .json(&body)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    let bytes = resp.bytes().await.map(|b| b.len() as u64).unwrap_or(0);
                    let lat_us = start.elapsed().as_micros() as u64;
                    // Any non-network response counts toward latency measurement;
                    // the streamlock hot path is the thing we're characterizing.
                    ctx.metrics.record_success(lat_us, bytes);
                    match code {
                        200..=299 => s2xx.fetch_add(1, Ordering::Relaxed),
                        429 => s429.fetch_add(1, Ordering::Relaxed),
                        _ => sother.fetch_add(1, Ordering::Relaxed),
                    };
                }
                Err(_) => {
                    snet.fetch_add(1, Ordering::Relaxed);
                    ctx.metrics.record_error();
                }
            }
        }
    };

    let (metrics, elapsed, _snaps) = runner::run_load_test(
        args.vus,
        Duration::from_secs(args.duration),
        Duration::from_secs(args.warmup),
        LoadTestConfig {
            client: client.clone(),
            base_url: args.base_url.clone(),
        },
        scenario,
    )
    .await;

    let summary = metrics.summary(elapsed);
    let counts: Vec<u64> = shard_counts
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .collect();
    let (shard_mean, shard_stddev) = mean_stddev(&counts);
    let shard_cv = if shard_mean > 0.0 {
        shard_stddev / shard_mean
    } else {
        0.0
    };

    let report = Report {
        tokens: args.tokens,
        distribution: args.distribution,
        vus: args.vus,
        duration_s: elapsed,
        rps: summary.throughput,
        latency_p50_ms: summary.p50_ms,
        latency_p95_ms: summary.p95_ms,
        latency_p99_ms: summary.p99_ms,
        latency_p999_ms: summary.p999_ms,
        total_requests: summary.total,
        status_2xx: status_2xx.load(Ordering::Relaxed),
        status_429: status_429.load(Ordering::Relaxed),
        status_other: status_other.load(Ordering::Relaxed),
        network_errors: network_errors.load(Ordering::Relaxed),
        shard_counts: counts,
        shard_mean,
        shard_stddev,
        shard_cv,
    };

    let json = serde_json::to_string_pretty(&report).expect("serialize report");
    println!("{json}");
    if let Some(path) = &args.output {
        if let Err(e) = std::fs::write(path, format!("{json}\n")) {
            tracing::error!("failed to write {}: {e}", path);
            std::process::exit(2);
        }
    }

    tracing::info!(
        "rps={:.0} p50={:.2}ms p99={:.2}ms 200={} 429={} other={} neterr={} shard_cv={:.3}",
        report.rps,
        report.latency_p50_ms,
        report.latency_p99_ms,
        report.status_2xx,
        report.status_429,
        report.status_other,
        report.network_errors,
        report.shard_cv,
    );
}

/// Build a plausible JWT-shaped string with a unique jti. Not signed — streamlock
/// decodes without verification, matching the production trust model.
fn synth_token(i: usize) -> String {
    use base64::Engine;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#);
    let payload_json = format!(r#"{{"jti":"bench-tok-{i}","exp":9999999999}}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
    // Signature value is irrelevant (unverified); include a constant suffix.
    format!("{header}.{payload}.sig")
}

fn uniform_sample(n: usize) -> usize {
    // rand::random::<f64>() is version-stable across rand 0.8/0.9/0.10.
    let u: f64 = rand::random();
    ((u * n as f64) as usize).min(n - 1)
}

/// Power-law approximation of Zipf. Not the full harmonic-normalized Zipf,
/// but gives a tunable heavy-head distribution without pulling `rand_distr`.
fn zipf_sample(n: usize, s: f64) -> usize {
    let u: f64 = rand::random();
    // u ∈ [0, 1) → x = u^(-1/s) gives a bounded long-tail index; clamp.
    let x = if u > 0.0 { u.powf(-1.0 / s) } else { 1.0 };
    (x as usize).saturating_sub(1).min(n - 1)
}

/// Simple hash over the token string, bucketed into `shards` lanes. Used for
/// client-side observation of fan-out balance; not the same hash streamlock
/// uses internally, but correlates well when the token space is uniform.
fn token_shard(token: &str, shards: usize) -> usize {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in token.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    (h as usize) % shards
}

fn mean_stddev(counts: &[u64]) -> (f64, f64) {
    if counts.is_empty() {
        return (0.0, 0.0);
    }
    let n = counts.len() as f64;
    let mean = counts.iter().map(|c| *c as f64).sum::<f64>() / n;
    let var =
        counts.iter().map(|c| (*c as f64 - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}
