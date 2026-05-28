//! Benchmark runner — POST starts a load-test subprocess on the
//! host via `yeti:process/spawner`; GET reports the current phase.
//!
//! Single-runner contract: at most one load-test runs at a time per
//! app instance. State lives in a wasm-local `OnceLock<Mutex<...>>`
//! — fine for the single-pool-instance model the dashboard exercises.

use std::sync::{Mutex, OnceLock};
use yeti_sdk::prelude::*;
use yeti_sdk::process::{Durability, Phase, PhaseEvent, SpawnConfig, SpawnHandle};

#[derive(Clone, Debug)]
struct RunnerState {
    test_name: String,
    pid: u32,
    phase: String,
    started_at_ms: u64,
}

fn state_slot() -> &'static Mutex<Option<RunnerState>> {
    static SLOT: OnceLock<Mutex<Option<RunnerState>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn phase_label(p: Phase) -> &'static str {
    match p {
        Phase::Pending => "seeding",
        Phase::Seeding => "seeding",
        Phase::Warming => "warming",
        Phase::Running => "running",
        Phase::Cleaning => "cleaning",
        Phase::Exited => "idle",
        Phase::Crashed => "crashed",
    }
}

resource!(Runner {
    name = "runner",

    get(ctx) => {
        // Snapshot the slot under the mutex, then drop the lock
        // before any async work — holding it across `.await` would
        // deadlock concurrent GETs.
        let active = state_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        // Completion detection. The wasm guest can't drain the
        // host's PhaseEvent stream on a background task
        // (tokio::spawn panics inside the wasm sandbox — TLS isn't
        // host TLS), so the state slot would otherwise stay at
        // "seeding" forever after POST. Instead, on every GET we
        // check the TestRun table for a row matching the active
        // test_name with timestamp >= started_at. If one exists,
        // load-rest's reporter wrote its result and exited — the
        // run is done. Clear the slot and report idle.
        if let Some(s) = active.as_ref()
            && let Ok(table) = ctx.table("TestRun")
        {
            let rows: Vec<Value> = TableAccess::get_all(&table).await.unwrap_or_default();
            let started_ms = s.started_at_ms;
            let done = rows.iter().any(|r| {
                r["testName"].as_str() == Some(&s.test_name)
                    && r["timestamp"].as_str()
                        .and_then(|ts| yeti_sdk::__macro_deps::chrono::DateTime::parse_from_rfc3339(ts).ok())
                        .map(|dt| dt.timestamp_millis() as u64)
                        .is_some_and(|ms| ms >= started_ms)
            });
            if done {
                *state_slot().lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
        }

        // Re-snapshot after the possible clear above so the
        // response reflects "idle" without keeping the lock held
        // across `.await`.
        let final_state = state_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        match final_state {
            Some(s) => json!({
                "status": s.phase,
                "phase": s.phase,
                "testName": s.test_name,
                "pid": s.pid,
                "startedAt": s.started_at_ms,
            }),
            None => json!({ "status": "idle", "phase": "idle" }),
        }
    },

    post(ctx) => {
        // Reject concurrent runs.
        {
            let guard = state_slot().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = &*guard {
                if s.phase != "idle" {
                    return Err(YetiError::Validation(format!(
                        "test '{}' already running", s.test_name
                    )));
                }
            }
        }

        let body: Value = serde_json::from_slice(ctx.body())
            .map_err(|e| YetiError::Validation(format!("body is not valid JSON: {e}")))?;
        let test = body.get("test").and_then(Value::as_str)
            .ok_or_else(|| YetiError::Validation("missing 'test'".into()))?
            .to_owned();
        let binary = body.get("binary").and_then(Value::as_str)
            .ok_or_else(|| YetiError::Validation("missing 'binary'".into()))?
            .to_owned();
        let duration = body.get("duration").and_then(Value::as_u64).unwrap_or(30);
        let total_vus = body.get("vus").and_then(Value::as_u64).unwrap_or(100);
        let target_url = body.get("targetUrl").and_then(Value::as_str)
            .unwrap_or("https://localhost").to_owned();

        let cfg = SpawnConfig {
            binary,
            args: vec![
                "--test".into(), test.clone(),
                "--duration".into(), duration.to_string(),
                "--vus".into(), total_vus.to_string(),
                "--base-url".into(), target_url,
                "--report-url".into(), "https://localhost".into(),
                "--warmup".into(), "5".into(),
            ],
            env: Vec::new(),
            status_file: None,
            durability: Durability::OneShot,
            max_restarts: 0,
        };

        let handle = yeti_sdk::process::spawn(cfg)?;
        let pid = handle.pid();

        {
            let mut guard = state_slot().lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(RunnerState {
                test_name: test.clone(),
                pid,
                phase: "seeding".to_owned(),
                started_at_ms: now_ms(),
            });
        }

        // v1: don't try to drain the phase stream on a background
        // task — `tokio::spawn` panics inside the wasm guest
        // ("no reactor running" / TLS isolation, see
        // project_phase_52_host_invoke memory). We let the handle
        // drop here; the host supervisor keeps watching the child
        // and load-rest's own reporter writes the TestRun row on
        // exit. Subsequent GETs reflect the state we set below
        // (stays "seeding" until cleared manually or replaced by a
        // new POST). Push the lazy-phase update path to v2 once we
        // have a wasm-safe stream poll (could be a host-injected
        // futures task or per-tick GET-side fetch).
        drop(handle);

        json!({
            "pid": pid,
            "status": "seeding",
            "testName": test,
            "totalVus": total_vus,
        })
    },
});
