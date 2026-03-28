//! load-blob benchmark — extracted from bin/load_blob.rs for embedding in yeti CLI.

use crate::{
    common::LoadTestConfig, common::ReportContext, common::clear_tables,
    cli::{BenchArgs, write_phase},
    client, reporter, runner, common::validate_error_rate,
};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub async fn run(args: BenchArgs) {
    crate::common::init_tracing();
    let (auth_user, auth_pass) = args.auth_parts();
    let auth_user = auth_user.to_string();
    let auth_pass = auth_pass.to_string();
    let client = client::build_client();
    let duration = Duration::from_secs(args.duration);
    let warmup = Duration::from_secs(args.warmup);

    tracing::info!(
        "load-blob: test={}, duration={}s, warmup={}s, vus={}, base={}",
        args.test,
        args.duration,
        args.warmup,
        args.vus,
        args.base_url
    );

    match args.test.as_str() {
        "blob-retrieval" => {
            write_phase(&args, "seeding");
            tracing::info!("Clearing BlobData table...");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["BlobData"],
            )
            .await;

            let blob_id = Uuid::new_v4().to_string();
            let large_content =
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(2700);
            tracing::info!("Setup: creating 150KB article (id={})...", &blob_id[..8]);

            let body = serde_json::json!({
                "id": blob_id,
                "title": "Blob Benchmark Article",
                "author": "Benchmark",
                "category": "benchmark",
                "content": large_content,
            });
            let url = format!("{}/app-benchmarks/BlobData/", args.base_url);
            match client
                .post(&url)
                .basic_auth(&auth_user, Some(&auth_pass))
                .json(&body)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("Setup complete.");
                },
                Ok(resp) => {
                    tracing::error!(
                        "Setup: POST returned {} — cannot run blob test without seed data",
                        resp.status()
                    );
                    std::process::exit(1);
                },
                Err(e) => {
                    tracing::error!("Setup error: {}", e);
                    std::process::exit(1);
                },
            }

            write_phase(&args, "warming");
            let blob_id = Arc::new(blob_id);
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus,
                duration,
                warmup,
                LoadTestConfig {
                    client: client.clone(),
                    base_url: args.base_url.clone(),
                    auth_user: auth_user.clone(),
                    auth_pass: auth_pass.clone(),
                },
                move |ctx| {
                    let blob_id = blob_id.clone();
                    async move {
                        let url = format!("{}/app-benchmarks/BlobData/{}", ctx.base_url, blob_id);
                        let start = std::time::Instant::now();
                        let result = ctx
                            .client
                            .get(&url)
                            .basic_auth(&ctx.auth_user, Some(&ctx.auth_pass))
                            .send()
                            .await;
                        ctx.record_response(start, result).await;
                    }
                },
            )
            .await;

            let summary = metrics.summary(elapsed);
            validate_error_rate(&summary);
            let rctx = ReportContext {
                client: &client,
                base_url: &args.report_url,
                auth_user: &auth_user,
                auth_pass: &auth_pass,
            };
            reporter::report_results_with_snapshots(
                &rctx,
                "blob-retrieval",
                elapsed,
                &summary,
                &snapshots,
                args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                &args.base_url,
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &["BlobData"],
            )
            .await;
        },
        other => {
            tracing::error!("Unknown test for load-blob: {}", other);
            std::process::exit(1);
        },
    }
}
