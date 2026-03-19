//! load-realtime benchmark — extracted from bin/load_realtime.rs for embedding in yeti CLI.

use crate::{
    common::ReportContext, common::clear_tables,
    cli::{BenchArgs, write_phase},
    client,
    metrics::{Metrics, SnapshotCollector},
    reporter,
};
use futures::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

// ── Connection lifecycle tracker ──

struct ConnectionTracker {
    connected: AtomicU64,
    peak: AtomicU64,
    failed: AtomicU64,
    disconnected: AtomicU64,
    published: AtomicU64,
}

impl ConnectionTracker {
    fn new() -> Self {
        Self {
            connected: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            disconnected: AtomicU64::new(0),
            published: AtomicU64::new(0),
        }
    }

    fn on_connect(&self) {
        let current = self.connected.fetch_add(1, Ordering::Relaxed) + 1;
        let mut peak = self.peak.load(Ordering::Relaxed);
        while current > peak {
            match self.peak.compare_exchange_weak(
                peak,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    fn on_disconnect(&self) {
        self.connected.fetch_sub(1, Ordering::Relaxed);
        self.disconnected.fetch_add(1, Ordering::Relaxed);
    }

    fn on_fail(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    fn on_publish(&self) {
        self.published.fetch_add(1, Ordering::Relaxed);
    }
}

const BATCH_SIZE: u64 = 1000;
const BATCH_DELAY_MS: u64 = 100;

pub async fn run(args: BenchArgs) {
    crate::common::init_tracing();
    let (auth_user, auth_pass) = args.auth_parts();
    let auth_user = auth_user.to_string();
    let auth_pass = auth_pass.to_string();
    let client = client::build_client();
    let duration = Duration::from_secs(args.duration);
    let warmup = Duration::from_secs(args.warmup);

    tracing::info!(
        "load-realtime: test={}, duration={}s, warmup={}s, vus={} (subscribers), mode={}, base={}",
        args.test,
        args.duration,
        args.warmup,
        args.vus,
        args.mode,
        args.base_url
    );

    // Clear Message table before all realtime tests
    write_phase(&args, "seeding");
    tracing::info!("Clearing Message table...");
    clear_tables(
        &client,
        &args.base_url,
        &auth_user,
        &auth_pass,
        "app-benchmarks",
        &["Message"],
    )
    .await;

    write_phase(&args, "warming");

    match args.test.as_str() {
        "ws" => {
            run_ws_test(
                &args, &auth_user, &auth_pass, &client, duration, warmup, false,
            )
            .await;
            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["Message"],
            )
            .await;
        },
        "ws-ramp" => {
            run_ws_test(
                &args, &auth_user, &auth_pass, &client, duration, warmup, true,
            )
            .await;
            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["Message"],
            )
            .await;
        },
        "sse" => {
            run_sse_test(
                &args, &auth_user, &auth_pass, &client, duration, warmup, false,
            )
            .await;
            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["Message"],
            )
            .await;
        },
        "sse-ramp" => {
            run_sse_test(
                &args, &auth_user, &auth_pass, &client, duration, warmup, true,
            )
            .await;
            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["Message"],
            )
            .await;
        },
        other => {
            tracing::error!("Unknown test for load-realtime: {}", other);
            std::process::exit(1);
        },
    }
}

// ── WebSocket max-connection test ──

async fn run_ws_test(
    args: &BenchArgs,
    auth_user: &str,
    auth_pass: &str,
    client: &reqwest::Client,
    duration: Duration,
    warmup: Duration,
    is_ramp: bool,
) {
    let metrics = Arc::new(Metrics::new());
    let tracker = Arc::new(ConnectionTracker::new());
    let stop = Arc::new(AtomicBool::new(false));

    let connector = client::build_ws_connector();

    let total_vus = if is_ramp { args.max_vus } else { args.vus };

    let batch_size = if is_ramp { args.step_vus } else { BATCH_SIZE };
    let initial_vus = if is_ramp { args.start_vus } else { total_vus };
    let batch_delay = if is_ramp {
        Duration::from_secs(args.step_interval)
    } else {
        Duration::from_millis(BATCH_DELAY_MS)
    };

    tracing::info!(
        "Ramping up {} WS subscribers (batch={}, delay={:?})...",
        initial_vus,
        batch_size,
        batch_delay
    );
    let ramp_start = Instant::now();
    let mut handles = Vec::with_capacity(total_vus as usize);

    let initial = if is_ramp { initial_vus } else { total_vus };
    for batch_start in (0..initial).step_by(BATCH_SIZE as usize) {
        let batch_end = (batch_start + BATCH_SIZE).min(initial);

        for _vu in batch_start..batch_end {
            spawn_ws_subscriber(
                &args.base_url,
                &connector,
                &metrics,
                &tracker,
                &stop,
                &mut handles,
            );
        }

        if batch_end % 5000 == 0 || batch_end == initial {
            let connected = tracker.connected.load(Ordering::Relaxed);
            let failed = tracker.failed.load(Ordering::Relaxed);
            tracing::info!(
                "  spawned {}/{} (connected={}, failed={})",
                batch_end,
                total_vus,
                connected,
                failed
            );
        }

        if batch_end < initial {
            tokio::time::sleep(Duration::from_millis(BATCH_DELAY_MS)).await;
        }
    }

    metrics
        .active_vus
        .store(tracker.connected.load(Ordering::Relaxed), Ordering::Relaxed);

    let ramp_elapsed = ramp_start.elapsed();
    tracing::info!(
        "Initial ramp complete in {:.1}s: {} connected, {} failed",
        ramp_elapsed.as_secs_f64(),
        tracker.connected.load(Ordering::Relaxed),
        tracker.failed.load(Ordering::Relaxed),
    );

    let num_publishers = 4;
    let pub_url = format!("{}/app-benchmarks/Message", args.base_url);
    let mut pub_handles = Vec::new();
    for _ in 0..num_publishers {
        let pub_client = client.clone();
        let pub_user = auth_user.to_string();
        let pub_pass = auth_pass.to_string();
        let pub_stop = stop.clone();
        let pub_tracker = tracker.clone();
        let pub_url = pub_url.clone();
        pub_handles.push(tokio::spawn(async move {
            while !pub_stop.load(Ordering::Relaxed) {
                let body = serde_json::json!({
                    "id": Uuid::new_v4().to_string(),
                    "title": "bench",
                    "content": "benchmark message",
                });
                let _ = pub_client
                    .post(&pub_url)
                    .basic_auth(&pub_user, Some(&pub_pass))
                    .json(&body)
                    .send()
                    .await;
                pub_tracker.on_publish();
            }
        }));
    }

    if !warmup.is_zero() {
        tracing::info!("Warmup: {}s (metrics discarded)...", warmup.as_secs());
        tokio::time::sleep(warmup).await;
    }
    metrics.set_warming(false);
    let measure_start = std::time::Instant::now();

    let collector = SnapshotCollector::new(metrics.clone());
    let collector_handle = collector.start();

    if is_ramp {
        tracing::info!(
            "Measuring for {}s with ramp (step={} every {:?})...",
            duration.as_secs(),
            args.step_vus,
            batch_delay
        );
        let mut ramp_timer = tokio::time::interval(batch_delay);
        ramp_timer.tick().await;

        let measure_deadline = tokio::time::Instant::now() + duration;
        let mut current_spawned = initial;

        loop {
            tokio::select! {
                _ = ramp_timer.tick() => {
                    if current_spawned < total_vus {
                        let to_add = args.step_vus.min(total_vus - current_spawned);
                        for _ in 0..to_add {
                            spawn_ws_subscriber(
                                &args.base_url, &connector, &metrics, &tracker, &stop, &mut handles,
                            );
                        }
                        current_spawned += to_add;
                        metrics.active_vus.store(
                            tracker.connected.load(Ordering::Relaxed),
                            Ordering::Relaxed,
                        );
                        tracing::info!("  ramp: {} subscribers spawned ({} connected)",
                            current_spawned, tracker.connected.load(Ordering::Relaxed));
                    }
                }
                _ = tokio::time::sleep_until(measure_deadline) => {
                    break;
                }
            }
        }
    } else {
        tracing::info!("Measuring for {}s...", duration.as_secs());
        tokio::time::sleep(duration).await;
    }

    stop.store(true, Ordering::Release);

    let snapshots = collector.finish();
    let _ = collector_handle.await;

    for h in pub_handles {
        h.await.ok();
    }
    for h in handles {
        h.await.ok();
    }

    let elapsed = measure_start.elapsed().as_secs_f64();
    let test_name = if is_ramp { "ws-ramp" } else { "ws" };
    print_connection_report("WebSocket", &tracker, &metrics, elapsed);
    let extra = connection_extra(&tracker);

    let summary = metrics.summary(elapsed);
    let summary_text = summary.format_summary(elapsed);

    let mut results_json = serde_json::json!({
        "throughput": (summary.throughput * 10.0).round() / 10.0,
        "p50": (summary.p50_ms * 100.0).round() / 100.0,
        "p95": (summary.p95_ms * 100.0).round() / 100.0,
        "p99": (summary.p99_ms * 100.0).round() / 100.0,
        "p999": (summary.p999_ms * 100.0).round() / 100.0,
        "total": summary.total,
        "errors": summary.errors,
    });
    if let Some(extra_obj) = extra.as_object() {
        for (k, v) in extra_obj {
            results_json
                .as_object_mut()
                .unwrap()
                .insert(k.clone(), v.clone());
        }
    }

    let snapshots_str = if !snapshots.is_empty() {
        serde_json::to_string(&snapshots).ok()
    } else {
        None
    };

    let mut payload = serde_json::json!({
        "testName": test_name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "durationSecs": (elapsed * 10.0).round() / 10.0,
        "results": results_json.to_string(),
        "summary": summary_text,
        "extrapolatedThroughput": format!("{:.1}", summary.throughput),
    });
    if let Some(snaps) = snapshots_str {
        payload
            .as_object_mut()
            .unwrap()
            .insert("snapshots".to_string(), serde_json::json!(snaps));
    }

    let url = format!("{}/app-benchmarks/TestRun", args.base_url);
    match client
        .post(&url)
        .basic_auth(auth_user, Some(auth_pass))
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) if !resp.status().is_success() => {
            tracing::warn!("POST {} returned {}", url, resp.status());
        },
        Err(e) => tracing::warn!("Failed to POST results: {}", e),
        _ => {},
    }
}

fn spawn_ws_subscriber(
    base_url: &str,
    connector: &tokio_tungstenite::Connector,
    metrics: &Arc<Metrics>,
    tracker: &Arc<ConnectionTracker>,
    stop: &Arc<AtomicBool>,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    let ws_url = format!(
        "{}/app-benchmarks/Message?stream=ws",
        base_url
            .replace("https://", "wss://")
            .replace("http://", "ws://")
    );
    let m = metrics.clone();
    let t = tracker.clone();
    let s = stop.clone();
    let conn = connector.clone();

    handles.push(tokio::spawn(async move {
        let ws_result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_tls_with_config(&ws_url, None, false, Some(conn)),
        )
        .await;

        let mut ws = match ws_result {
            Ok(Ok((ws, _))) => {
                t.on_connect();
                ws
            },
            _ => {
                t.on_fail();
                return;
            },
        };

        while !s.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
                Ok(Some(Ok(msg))) => {
                    let bytes = msg.into_data().len() as u64;
                    m.record_success(0, bytes);
                },
                Ok(Some(Err(_))) => {
                    m.record_error();
                    break;
                },
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        let _ = ws.close(None).await;
        t.on_disconnect();
    }));
}

// ── SSE max-connection test ──

struct SseSubscriberCtx<'a> {
    base_url: &'a str,
    auth_user: &'a str,
    auth_pass: &'a str,
    sse_client: &'a reqwest::Client,
    metrics: &'a Arc<Metrics>,
    tracker: &'a Arc<ConnectionTracker>,
    stop: &'a Arc<AtomicBool>,
}

async fn run_sse_test(
    args: &BenchArgs,
    auth_user: &str,
    auth_pass: &str,
    client: &reqwest::Client,
    duration: Duration,
    warmup: Duration,
    is_ramp: bool,
) {
    let metrics = Arc::new(Metrics::new());
    let tracker = Arc::new(ConnectionTracker::new());
    let stop = Arc::new(AtomicBool::new(false));

    let sse_client = client::build_streaming_client();

    let total_vus = if is_ramp { args.max_vus } else { args.vus };

    let batch_size = if is_ramp { args.step_vus } else { BATCH_SIZE };
    let initial_vus = if is_ramp { args.start_vus } else { total_vus };
    let batch_delay = if is_ramp {
        Duration::from_secs(args.step_interval)
    } else {
        Duration::from_millis(BATCH_DELAY_MS)
    };

    tracing::info!(
        "Ramping up {} SSE subscribers (batch={}, delay={:?})...",
        initial_vus,
        batch_size,
        batch_delay
    );
    let ramp_start = Instant::now();
    let mut handles = Vec::with_capacity(total_vus as usize);

    let initial = if is_ramp { initial_vus } else { total_vus };
    let sse_ctx = SseSubscriberCtx {
        base_url: &args.base_url,
        auth_user,
        auth_pass,
        sse_client: &sse_client,
        metrics: &metrics,
        tracker: &tracker,
        stop: &stop,
    };
    for batch_start in (0..initial).step_by(BATCH_SIZE as usize) {
        let batch_end = (batch_start + BATCH_SIZE).min(initial);

        for _vu in batch_start..batch_end {
            spawn_sse_subscriber(&sse_ctx, &mut handles);
        }

        if batch_end % 5000 == 0 || batch_end == initial {
            let connected = tracker.connected.load(Ordering::Relaxed);
            let failed = tracker.failed.load(Ordering::Relaxed);
            tracing::info!(
                "  spawned {}/{} (connected={}, failed={})",
                batch_end,
                total_vus,
                connected,
                failed
            );
        }

        if batch_end < initial {
            tokio::time::sleep(Duration::from_millis(BATCH_DELAY_MS)).await;
        }
    }

    metrics
        .active_vus
        .store(tracker.connected.load(Ordering::Relaxed), Ordering::Relaxed);

    let ramp_elapsed = ramp_start.elapsed();
    tracing::info!(
        "Initial ramp complete in {:.1}s: {} connected, {} failed",
        ramp_elapsed.as_secs_f64(),
        tracker.connected.load(Ordering::Relaxed),
        tracker.failed.load(Ordering::Relaxed),
    );

    let num_publishers = 4;
    let pub_url = format!("{}/app-benchmarks/Message", args.base_url);
    let mut pub_handles = Vec::new();
    for _ in 0..num_publishers {
        let pub_client = client.clone();
        let pub_user = auth_user.to_string();
        let pub_pass = auth_pass.to_string();
        let pub_stop = stop.clone();
        let pub_tracker = tracker.clone();
        let pub_url = pub_url.clone();
        pub_handles.push(tokio::spawn(async move {
            while !pub_stop.load(Ordering::Relaxed) {
                let body = serde_json::json!({
                    "id": Uuid::new_v4().to_string(),
                    "title": "bench",
                    "content": "benchmark sse message",
                });
                let _ = pub_client
                    .post(&pub_url)
                    .basic_auth(&pub_user, Some(&pub_pass))
                    .json(&body)
                    .send()
                    .await;
                pub_tracker.on_publish();
            }
        }));
    }

    if !warmup.is_zero() {
        tracing::info!("Warmup: {}s (metrics discarded)...", warmup.as_secs());
        tokio::time::sleep(warmup).await;
    }
    metrics.set_warming(false);
    let measure_start = std::time::Instant::now();

    let collector = SnapshotCollector::new(metrics.clone());
    let collector_handle = collector.start();

    if is_ramp {
        tracing::info!(
            "Measuring for {}s with ramp (step={} every {:?})...",
            duration.as_secs(),
            args.step_vus,
            batch_delay
        );
        let mut ramp_timer = tokio::time::interval(batch_delay);
        ramp_timer.tick().await;

        let measure_deadline = tokio::time::Instant::now() + duration;
        let mut current_spawned = initial;

        loop {
            tokio::select! {
                _ = ramp_timer.tick() => {
                    if current_spawned < total_vus {
                        let to_add = args.step_vus.min(total_vus - current_spawned);
                        for _ in 0..to_add {
                            spawn_sse_subscriber(&sse_ctx, &mut handles);
                        }
                        current_spawned += to_add;
                        metrics.active_vus.store(
                            tracker.connected.load(Ordering::Relaxed),
                            Ordering::Relaxed,
                        );
                        tracing::info!("  ramp: {} subscribers spawned ({} connected)",
                            current_spawned, tracker.connected.load(Ordering::Relaxed));
                    }
                }
                _ = tokio::time::sleep_until(measure_deadline) => {
                    break;
                }
            }
        }
    } else {
        tracing::info!("Measuring for {}s...", duration.as_secs());
        tokio::time::sleep(duration).await;
    }

    stop.store(true, Ordering::Release);

    let snapshots = collector.finish();
    let _ = collector_handle.await;

    for h in pub_handles {
        h.await.ok();
    }
    for h in handles {
        h.await.ok();
    }

    let elapsed = measure_start.elapsed().as_secs_f64();
    let test_name = if is_ramp { "sse-ramp" } else { "sse" };
    print_connection_report("SSE", &tracker, &metrics, elapsed);
    let extra = connection_extra(&tracker);
    let rctx = ReportContext {
        client,
        base_url: &args.base_url,
        auth_user,
        auth_pass,
    };
    reporter::report_results_full(
        &rctx,
        test_name,
        elapsed,
        &metrics.summary(elapsed),
        Some(extra),
        &snapshots,
        total_vus,
    )
    .await;
}

fn spawn_sse_subscriber(
    ctx: &SseSubscriberCtx<'_>,
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    let sse_url = format!("{}/app-benchmarks/Message?stream=sse", ctx.base_url);
    let m = ctx.metrics.clone();
    let t = ctx.tracker.clone();
    let s = ctx.stop.clone();
    let c = ctx.sse_client.clone();
    let sse_user = ctx.auth_user.to_string();
    let sse_pass = ctx.auth_pass.to_string();

    handles.push(tokio::spawn(async move {
        let resp = tokio::time::timeout(
            Duration::from_secs(10),
            c.get(&sse_url)
                .basic_auth(&sse_user, Some(&sse_pass))
                .send(),
        )
        .await;

        let stream = match resp {
            Ok(Ok(r)) if r.status().is_success() => {
                t.on_connect();
                r.bytes_stream()
            },
            _ => {
                t.on_fail();
                return;
            },
        };
        let mut stream = stream;

        while !s.load(Ordering::Relaxed) {
            match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    let text = String::from_utf8_lossy(&chunk);
                    let msg_count = text.lines().filter(|l| l.starts_with("data:")).count();
                    for _ in 0..msg_count.max(1) {
                        m.record_success(0, chunk.len() as u64);
                    }
                },
                Ok(Some(Err(_))) => {
                    m.record_error();
                    break;
                },
                Ok(None) => break,
                Err(_) => continue,
            }
        }

        t.on_disconnect();
    }));
}

// ── Shared reporting ──

fn connection_extra(tracker: &ConnectionTracker) -> serde_json::Value {
    serde_json::json!({
        "peakConnections": tracker.peak.load(Ordering::Relaxed),
        "connectionFailures": tracker.failed.load(Ordering::Relaxed),
        "published": tracker.published.load(Ordering::Relaxed),
    })
}

fn print_connection_report(
    protocol: &str,
    tracker: &ConnectionTracker,
    metrics: &Metrics,
    elapsed: f64,
) {
    let peak = tracker.peak.load(Ordering::Relaxed);
    let failed = tracker.failed.load(Ordering::Relaxed);
    let disconnected = tracker.disconnected.load(Ordering::Relaxed);
    let published = tracker.published.load(Ordering::Relaxed);
    let summary = metrics.summary(elapsed);

    tracing::info!("=== {} Max-Connection Results ===", protocol);
    tracing::info!("Peak concurrent connections: {}", peak);
    tracing::info!("Connection failures: {}", failed);
    tracing::info!("Disconnects during test: {}", disconnected);
    tracing::info!("Messages published: {}", published);
    tracing::info!(
        "Messages received: {} ({:.1} msgs/s fan-out)",
        summary.total,
        summary.throughput
    );
    if peak > 0 && published > 0 {
        let expected = peak * published;
        let delivery_pct = if expected > 0 {
            summary.total as f64 / expected as f64 * 100.0
        } else {
            0.0
        };
        tracing::info!(
            "Delivery rate: {:.1}% ({} received / {} expected)",
            delivery_pct,
            summary.total,
            expected
        );
    }
    if summary.total_bytes > 0 {
        let mb = summary.total_bytes as f64 / (1024.0 * 1024.0);
        tracing::info!("Total bytes: {:.1} MB ({:.1} MB/s)", mb, mb / elapsed);
    }
}
