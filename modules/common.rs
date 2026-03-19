//! Yeti Benchmarks Extension
//!
//! Load test runner, HDR histogram metrics, and result aggregation.
//! Provides benchmark resources (runner control, best results) and
//! standalone load test binaries.


// Benchmark entry points (library versions of former standalone binaries)

/// Maximum allowed error rate before a benchmark is considered invalid.
pub const MAX_ERROR_RATE: f64 = 0.01; // 1%

/// Initialize tracing subscriber for benchmark binaries.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .compact()
        .init();
}

/// Validate error rate after a benchmark run. Exits with code 2 if error rate exceeds threshold.
pub fn validate_error_rate(summary: &crate::metrics::MetricsSummary) {
    if summary.total == 0 {
        tracing::error!("No requests completed — results invalid");
        std::process::exit(2);
    }
    let error_rate = summary.errors as f64 / summary.total as f64;
    if error_rate > MAX_ERROR_RATE {
        tracing::error!(
            error_rate = format!("{:.1}%", error_rate * 100.0),
            errors = summary.errors,
            total = summary.total,
            "Error rate exceeds {:.0}% threshold — results invalid",
            MAX_ERROR_RATE * 100.0
        );
        std::process::exit(2);
    }
}

/// Truncate the specified tables via `DELETE /{app}/{Table}` (collection-level delete).
/// Uses RocksDB's `delete_range_cf` under the hood — near-instant even for millions of keys.
/// Call before write tests to ensure a clean slate and avoid RocksDB compaction stalls.
pub async fn clear_tables(
    client: &reqwest::Client,
    base_url: &str,
    auth_user: &str,
    auth_pass: &str,
    app: &str,
    tables: &[&str],
) {
    for table in tables {
        let url = format!("{}/{}/{}", base_url, app, table);
        match client
            .delete(&url)
            .basic_auth(auth_user, Some(auth_pass))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("Truncated {}/{}", app, table);
            },
            Ok(resp) => {
                tracing::warn!("Truncate {}/{} returned {}", app, table, resp.status());
            },
            Err(e) => {
                tracing::warn!("Failed to truncate {}/{}: {}", app, table, e);
            },
        }
    }
}

/// Fetch real Book IDs from the server via REST API.
pub async fn fetch_book_ids(
    client: &reqwest::Client,
    base_url: &str,
    auth_user: &str,
    auth_pass: &str,
    limit: usize,
) -> Vec<String> {
    let url = format!(
        "{}/app-benchmarks/Book?limit={}&select=id",
        base_url, limit
    );
    match client
        .get(&url)
        .basic_auth(auth_user, Some(auth_pass))
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                let arr = if data.is_array() {
                    data.as_array().cloned().unwrap_or_default()
                } else {
                    data.get("data")
                        .and_then(|d| d.as_array())
                        .cloned()
                        .unwrap_or_default()
                };
                arr.iter()
                    .filter_map(|v| v.get("id").and_then(|id| id.as_str()).map(String::from))
                    .collect()
            } else {
                Vec::new()
            }
        },
        Err(_) => Vec::new(),
    }
}

/// Owned HTTP session config for load test runners (needs owned data for `Send + 'static`).
pub struct LoadTestConfig {
    pub client: reqwest::Client,
    pub base_url: String,
    pub auth_user: String,
    pub auth_pass: String,
}

/// Borrowed HTTP session config for reporters (runs after the test, no ownership transfer).
pub struct ReportContext<'a> {
    pub client: &'a reqwest::Client,
    pub base_url: &'a str,
    pub auth_user: &'a str,
    pub auth_pass: &'a str,
}

/// Post-write verification: fetches table metadata, compares record count to expected,
/// prints a summary, and returns a JSON value with verification + WAL stats for inclusion
/// in the TestRun results.
pub async fn verify_write_results(
    client: &reqwest::Client,
    base_url: &str,
    auth_user: &str,
    auth_pass: &str,
    expected: u64,
) -> Option<serde_json::Value> {
    // Wait for WAL consumer to drain (large writes need more time)
    tracing::info!("Waiting for WAL consumer drain...");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let url = format!("{}/app-benchmarks/Book?_metadata=true", base_url);
    let resp = match client
        .get(&url)
        .basic_auth(auth_user, Some(auth_pass))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("metadata fetch failed: {}", e);
            return None;
        },
    };

    let metadata: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("metadata parse failed: {}", e);
            return None;
        },
    };

    let record_count = metadata
        .get("auditSize")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let matched = record_count >= expected;

    tracing::info!("--- Write Verification ---");
    tracing::info!("  Expected (successful writes): {}", expected);
    tracing::info!("  Actual records in DB:         {}", record_count);
    tracing::info!(
        "  Match: {}",
        if matched {
            "YES"
        } else {
            "NO (WAL may still be draining)"
        }
    );

    let mut extra = serde_json::json!({
        "verification": {
            "recordCount": record_count,
            "expected": expected,
            "match": matched,
        },
    });

    // Include WAL consumer stats if available
    if let Some(wal_stats) = metadata.get("storageStats").and_then(|s| s.get("wal")) {
        tracing::info!("--- WAL Consumer Stats ---");
        if let Some(batches) = wal_stats.get("batches").and_then(|v| v.as_u64()) {
            tracing::info!("  Batches committed:    {}", batches);
        }
        if let Some(avg) = wal_stats.get("avgBatchSize").and_then(|v| v.as_f64()) {
            tracing::info!("  Avg batch size:       {:.1}", avg);
        }
        if let Some(avg) = wal_stats.get("avgCommitTimeUs").and_then(|v| v.as_f64()) {
            tracing::info!("  Avg commit time:      {:.1} us", avg);
        }
        if let Some(ops) = wal_stats.get("opsCommitted").and_then(|v| v.as_u64()) {
            tracing::info!("  Total ops committed:  {}", ops);
        }
        if let Some(bytes) = wal_stats.get("bytesRead").and_then(|v| v.as_u64()) {
            let mb = bytes as f64 / (1024.0 * 1024.0);
            tracing::info!("  WAL bytes read:       {:.1} MB", mb);
        }
        extra["wal"] = wal_stats.clone();
    }

    // Include server config from storageStats if available
    if let Some(storage_stats) = metadata.get("storageStats") {
        // Pass through any top-level storage stats that aren't wal
        let mut config = serde_json::Map::new();
        if let Some(obj) = storage_stats.as_object() {
            for (k, v) in obj {
                if k != "wal" {
                    config.insert(k.clone(), v.clone());
                }
            }
        }
        if !config.is_empty() {
            extra["storageConfig"] = serde_json::Value::Object(config);
        }
    }

    Some(extra)
}
