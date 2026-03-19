// Benchmark Runner Resource
//
// Manages benchmark test execution and results.
// Spawns standalone load test binaries (compiled from src/bin/) as child processes.
//
// | Method | Path                          | Description                     |
// |--------|-------------------------------|---------------------------------|
// | GET    | /app-benchmarks/runner        | Get runner state                |
// | POST   | /app-benchmarks/runner        | Start a benchmark test          |

use std::sync::{Mutex, OnceLock};
use yeti_sdk::prelude::*;

// ── Test definitions ──

struct TestDef {
    id: &'static str,
    name: &'static str,
    binary: &'static str,
    duration: u64,
    vus: u64,
    category: &'static str,
}

impl TestDef {
    const fn quick(id: &'static str, name: &'static str, binary: &'static str, vus: u64) -> Self {
        Self { id, name, binary, duration: 30, vus, category: "throughput" }
    }
    const fn sustained(id: &'static str, name: &'static str, binary: &'static str, vus: u64) -> Self {
        Self { id, name, binary, duration: 300, vus, category: "sustained" }
    }
}

const TESTS: &[TestDef] = &[
    TestDef::quick("rest-read", "REST Reads", "load-rest", 100),
    TestDef::quick("rest-write", "REST Writes", "load-rest", 100),
    TestDef::quick("rest-update", "REST Update", "load-rest", 100),
    TestDef::quick("rest-join", "REST Join", "load-rest", 50),
    TestDef::quick("graphql-read", "GraphQL Reads", "load-graphql", 100),
    TestDef::quick("graphql-mutation", "GraphQL Mutations", "load-graphql", 100),
    TestDef::quick("graphql-join", "GraphQL Join", "load-graphql", 50),
    TestDef::quick("vector-embed", "Vector Embed", "load-vector", 10),
    TestDef::quick("vector-search", "Vector Search", "load-vector", 100),
    TestDef::quick("blob-retrieval", "150k Blob Retrieval", "load-blob", 100),
    TestDef::quick("ws", "WebSocket", "load-realtime", 1000),
    TestDef::quick("sse", "SSE Streaming", "load-realtime", 1000),
    TestDef::sustained("rest-read-sustained", "REST Reads", "load-rest", 100),
    TestDef::sustained("rest-write-sustained", "REST Writes", "load-rest", 100),
    TestDef::sustained("rest-update-sustained", "REST Update", "load-rest", 100),
    TestDef::sustained("rest-join-sustained", "REST Join", "load-rest", 50),
    TestDef::sustained("graphql-read-sustained", "GraphQL Reads", "load-graphql", 100),
    TestDef::sustained("graphql-mutation-sustained", "GraphQL Mutations", "load-graphql", 100),
    TestDef::sustained("graphql-join-sustained", "GraphQL Join", "load-graphql", 50),
    TestDef::sustained("vector-embed-sustained", "Vector Embed", "load-vector", 10),
    TestDef::sustained("vector-search-sustained", "Vector Search", "load-vector", 100),
    TestDef::sustained("blob-retrieval-sustained", "150k Blob Retrieval", "load-blob", 100),
    TestDef::sustained("ws-sustained", "WebSocket", "load-realtime", 1000),
    TestDef::sustained("sse-sustained", "SSE Streaming", "load-realtime", 1000),
];

// ── Runner state ──

#[derive(Clone)]
struct RunnerState {
    status: String,
    test_name: Option<String>,
    started_at: Option<f64>,
    warming_started_at: Option<f64>,
    configured_duration: Option<u64>,
    configured_vus: Option<u64>,
    warmup_duration: u64,
    last_error: Option<String>,
    child_pid: Option<u32>,
    status_file: Option<String>,
}

impl Default for RunnerState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            test_name: None,
            started_at: None,
            warming_started_at: None,
            configured_duration: None,
            configured_vus: None,
            warmup_duration: 5,
            last_error: None,
            child_pid: None,
            status_file: None,
        }
    }
}

fn runner_state() -> &'static Mutex<RunnerState> {
    static STATE: OnceLock<Mutex<RunnerState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(RunnerState::default()))
}

fn runner_child() -> &'static Mutex<Option<std::process::Child>> {
    static CHILD: OnceLock<Mutex<Option<std::process::Child>>> = OnceLock::new();
    CHILD.get_or_init(|| Mutex::new(None))
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Find the load test binary — checks the shared build cache first (where the
/// plugin compiler puts binaries), then the app's own target directory.
fn find_binary(binary_name: &str) -> Option<String> {
    let root = get_root_directory();

    // Shared build cache (plugin compiler output — binaries built alongside dylib)
    for profile in &["release", "debug"] {
        let path = root.join("cache/builds/target").join(profile).join(binary_name);
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }

    // App's own target directory (standalone cargo build)
    let app_dir = get_apps_directory().join("app-benchmarks");
    for profile in &["release", "debug"] {  // prefer release
        let path = app_dir.join("target").join(profile).join(binary_name);
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }

    None
}

resource!(BenchmarkRunner {
    name = "runner",
    get(_request, ctx) => {
        let state = runner_state().lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut current = state.clone();

        // Check if child process has finished
        if current.status != "idle" {
            let mut should_idle = false;

            // Check child process status
            if let Ok(mut guard) = runner_child().lock() {
                match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => {
                            if !status.success() {
                                runner_state().lock().unwrap_or_else(|e| e.into_inner())
                                    .last_error = Some(format!("Exited with: {}", status));
                            }
                            *guard = None;
                            should_idle = true;
                        },
                        Ok(None) => {
                            // Process still running — but check for timeout
                            // If started_at + configured_duration + 300s has passed, kill it
                            if let (Some(started), Some(duration)) = (current.started_at, current.configured_duration) {
                                let max_time = (current.warmup_duration as f64) + (duration as f64) + 15.0;
                                if now_secs() - started > max_time {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    *guard = None;
                                    should_idle = true;
                                    runner_state().lock().unwrap_or_else(|e| e.into_inner())
                                        .last_error = Some("Benchmark timed out".to_string());
                                }
                            }
                        },
                        Err(_) => { *guard = None; should_idle = true; },
                    },
                    None => {
                        // No child process stored — state is stale, force idle
                        should_idle = true;
                    },
                }
            }

            if should_idle {
                // Check if the child's last phase was "cleaning" — show it for one cycle
                let show_cleaning = {
                    let s = runner_state().lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(ref path) = s.status_file {
                        std::fs::read_to_string(path).ok()
                            .map(|p| p.trim() == "cleaning" && s.status != "cleaning")
                            .unwrap_or(false)
                    } else { false }
                };

                let mut s = runner_state().lock().unwrap_or_else(|e| e.into_inner());
                if show_cleaning {
                    s.status = "cleaning".to_string();
                    s.child_pid = None;
                } else {
                    s.status = "idle".to_string();
                    s.child_pid = None;
                    s.warming_started_at = None;
                    if let Some(ref path) = s.status_file {
                        let _ = std::fs::remove_file(path);
                    }
                    s.status_file = None;
                }
                current = s.clone();
            }
        }

        // Read phase from status file and update state
        // Note: the binary writes seeding → warming → cleaning (no "running" phase).
        // We infer "running" from elapsed time past warmup duration.
        if let Some(ref sf) = current.status_file {
            if let Ok(phase) = std::fs::read_to_string(sf) {
                let phase = phase.trim();
                let mut s = runner_state().lock().unwrap_or_else(|e| e.into_inner());
                match phase {
                    "seeding" if s.status != "seeding" => {
                        s.status = "seeding".to_string();
                    },
                    "warming" => {
                        if s.warming_started_at.is_none() {
                            s.warming_started_at = Some(now_secs());
                        }
                        // Check if warmup period has elapsed → transition to "running"
                        let warmup_elapsed = s.warming_started_at.map(|ws| now_secs() - ws).unwrap_or(0.0);
                        if warmup_elapsed >= s.warmup_duration as f64 {
                            s.status = "running".to_string();
                        } else {
                            s.status = "warming".to_string();
                        }
                    },
                    "cleaning" if s.status != "cleaning" => {
                        s.status = "cleaning".to_string();
                    },
                    _ => {},
                }
                current = s.clone();
            }
        }

        // Calculate elapsed: time since warmup ended (start of actual measurement)
        let elapsed = if current.status == "running" {
            let measuring_start = current.warming_started_at
                .map(|ws| ws + current.warmup_duration as f64)
                .unwrap_or(now_secs());
            let raw = (now_secs() - measuring_start).max(0.0);
            // Cap at configured duration
            match current.configured_duration {
                Some(d) if d > 0 => raw.min(d as f64),
                _ => raw,
            }
        } else { 0.0 };

        reply().json(json!({
            "status": current.status,
            "phase": current.status,
            "testName": current.test_name,
            "warmupSecs": if current.status == "warming" {
                current.warming_started_at.map(|s| now_secs() - s).unwrap_or(0.0)
            } else { 0.0 },
            "elapsedSecs": elapsed,
            "configuredDuration": current.configured_duration,
            "lastError": current.last_error,
        }))
    },

    post(request, ctx) => {
        let body = request.json_value()?;
        let test_id = body.require_str("test")?;

        let test_def = match TESTS.iter().find(|t| t.id == test_id) {
            Some(t) => t,
            None => return bad_request(&format!("Unknown test: {}", test_id)),
        };

        {
            let state = runner_state().lock().unwrap_or_else(|e| e.into_inner());
            if state.status != "idle" {
                return bad_request("A test is already running");
            }
        }

        // Find the load test binary
        let binary_path = match find_binary(test_def.binary) {
            Some(p) => p,
            None => return bad_request(&format!(
                "Load test binary '{}' not found. Run 'cargo build --release --bins' in the app-benchmarks directory.",
                test_def.binary
            )),
        };

        let duration = test_def.duration;
        let vus = body.get("vus").and_then(|v| v.as_u64()).unwrap_or(test_def.vus);

        let status_file = std::env::temp_dir()
            .join(format!("yeti-bench-{}.status", std::process::id()))
            .to_string_lossy().to_string();
        let _ = std::fs::write(&status_file, "seeding");

        let target_url = body.get("targetUrl").and_then(|v| v.as_str()).map(String::from)
            .or_else(|| std::env::var("YETI_BENCHMARK_TARGET").ok())
            .unwrap_or_else(|| "https://localhost".to_string());

        let mut cmd = std::process::Command::new(&binary_path);
        cmd.arg("--test").arg(&test_id)
            .arg("--base-url").arg(&target_url)
            .arg("--duration").arg(duration.to_string())
            .arg("--vus").arg(vus.to_string())
            .arg("--warmup").arg("5")
            .arg("--status-file").arg(&status_file);

        // Close inherited FDs (prevents RocksDB SST file inheritance)
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    // Close all inherited FDs — RocksDB can open thousands of SST files
                    for fd in 3..65536 { libc::close(fd); }
                    Ok(())
                });
            }
        }

        match cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            Ok(child) => {
                let pid = child.id();
                *runner_child().lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
                let mut state = runner_state().lock().unwrap_or_else(|e| e.into_inner());
                state.status = "seeding".to_string();
                state.test_name = Some(test_id.to_string());
                state.started_at = Some(now_secs());
                state.configured_duration = Some(duration);
                state.configured_vus = Some(vus);
                state.warmup_duration = 5;
                state.last_error = None;
                state.child_pid = Some(pid);
                state.status_file = Some(status_file);

                reply().json(json!({
                    "status": "seeding",
                    "testName": test_id,
                    "pid": pid,
                }))
            },
            Err(e) => {
                let _ = std::fs::remove_file(&status_file);
                let mut state = runner_state().lock().unwrap_or_else(|e| e.into_inner());
                state.status = "idle".to_string();
                state.last_error = Some(format!("Failed to start '{}': {}", binary_path, e));
                bad_request(&format!("Failed to start benchmark: {}", e))
            },
        }
    }
});
