use crate::common::ReportContext;
use crate::metrics::{MetricsSummary, Snapshot};

/// POST test results with optional extra fields merged into the results JSON.
pub async fn report_results_ext(
    ctx: &ReportContext<'_>,
    test_name: &str,
    duration_secs: f64,
    summary: &MetricsSummary,
    extra: Option<serde_json::Value>,
    clients: u64,
) {
    report_results_full(ctx, test_name, duration_secs, summary, extra, &[], clients).await
}

/// POST test results with snapshots and optional extra fields.
pub async fn report_results_with_snapshots(
    ctx: &ReportContext<'_>,
    test_name: &str,
    duration_secs: f64,
    summary: &MetricsSummary,
    snapshots: &[Snapshot],
    clients: u64,
) {
    report_results_full(
        ctx,
        test_name,
        duration_secs,
        summary,
        None,
        snapshots,
        clients,
    )
    .await
}

/// Full reporter: includes p99.9, snapshots, CV, and extra fields.
pub async fn report_results_full(
    ctx: &ReportContext<'_>,
    test_name: &str,
    duration_secs: f64,
    summary: &MetricsSummary,
    extra: Option<serde_json::Value>,
    snapshots: &[Snapshot],
    clients: u64,
) {
    let client = ctx.client;
    let base_url = ctx.base_url;
    let auth_user = ctx.auth_user;
    let auth_pass = ctx.auth_pass;
    let summary_text = summary.format_summary(duration_secs);
    tracing::info!("=== {} ===", test_name);
    tracing::info!("{}", summary_text);
    if summary.total_bytes > 0 {
        let mb = summary.total_bytes as f64 / (1024.0 * 1024.0);
        tracing::info!("Total bytes: {:.1} MB ({:.1} MB/s)", mb, mb / duration_secs);
    }

    // Warn on significant error rate
    if summary.total > 0 {
        let error_rate = summary.errors as f64 / summary.total as f64;
        if error_rate > 0.001 {
            tracing::warn!(
                error_rate = format!("{:.2}%", error_rate * 100.0),
                errors = summary.errors,
                total = summary.total,
                "Significant error rate — results may be unreliable"
            );
        }
    }

    let mut results_json = serde_json::json!({
        "throughput": (summary.throughput * 10.0).round() / 10.0,
        "p50": (summary.p50_ms * 100.0).round() / 100.0,
        "p95": (summary.p95_ms * 100.0).round() / 100.0,
        "p99": (summary.p99_ms * 100.0).round() / 100.0,
        "p999": (summary.p999_ms * 100.0).round() / 100.0,
        "total": summary.total,
        "errors": summary.errors,
    });

    // Calculate CV (coefficient of variation) from snapshots
    if snapshots.len() >= 2 {
        let rps_values: Vec<f64> = snapshots.iter().map(|s| s.rps).collect();
        let mean = rps_values.iter().sum::<f64>() / rps_values.len() as f64;
        if mean > 0.0 {
            let variance = rps_values.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
                / rps_values.len() as f64;
            let cv = (variance.sqrt() / mean) * 100.0;
            let cv_rounded = (cv * 100.0).round() / 100.0;
            results_json
                .as_object_mut()
                .unwrap()
                .insert("cv".to_string(), serde_json::json!(cv_rounded));
            tracing::info!("CV (throughput stability): {:.2}%", cv_rounded);
        }
    }

    if let Some(extra) = extra
        && let (Some(base), Some(ext)) = (results_json.as_object_mut(), extra.as_object())
    {
        for (k, v) in ext {
            base.insert(k.clone(), v.clone());
        }
    }

    // Serialize snapshots if present
    let snapshots_str = if !snapshots.is_empty() {
        match serde_json::to_string(snapshots) {
            Ok(s) => {
                tracing::info!("Snapshots: {} data points collected", snapshots.len());
                Some(s)
            },
            Err(_) => None,
        }
    } else {
        None
    };

    let mut payload = serde_json::json!({
        "testName": test_name,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "durationSecs": (duration_secs * 10.0).round() / 10.0,
        "clients": clients,
        "results": results_json.to_string(),
        "summary": summary_text,
    });

    if let Some(snaps) = snapshots_str {
        payload.as_object_mut().unwrap()
            .insert("snapshots".to_string(), serde_json::json!(snaps));
    }

    if let Some(group) = ctx.run_group {
        payload.as_object_mut().unwrap()
            .insert("runGroup".to_string(), serde_json::json!(group));
    }

    let url = format!("{}/app-benchmarks/TestRun", base_url);
    match client
        .post(&url)
        .basic_auth(auth_user, Some(auth_pass))
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                tracing::warn!("POST {} returned {}", url, status);
            }
        },
        Err(e) => {
            tracing::warn!("Failed to POST results to {}: {}", url, e);
        },
    }
}
