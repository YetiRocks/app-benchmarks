//! load-rest benchmark — extracted from bin/load_rest.rs for embedding in yeti CLI.

use crate::{
    common::LoadTestConfig, common::ReportContext, common::clear_tables,
    cli::{BenchArgs, write_phase},
    client, common::fetch_book_ids, reporter, runner, common::validate_error_rate, common::verify_write_results,
};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Ensure a benchmark Author record exists for join tests.
async fn seed_author(table: &str, client: &reqwest::Client, base_url: &str, auth_user: &str, auth_pass: &str) {
    let body = serde_json::json!({
        "id": "bench-author-1",
        "name": "Benchmark Author",
    });
    let _ = client
        .post(format!("{}/app-benchmarks/{}/", base_url, table))
        .basic_auth(auth_user, Some(auth_pass))
        .json(&body)
        .send()
        .await;
}

/// Seed N Book records for read tests.
async fn seed_books(book_table: &str,
    client: &reqwest::Client,
    base_url: &str,
    auth_user: &str,
    auth_pass: &str,
    count: usize,
) {
    for i in 0..count {
        let id = Uuid::new_v4().to_string();
        let body = serde_json::json!({
            "id": id,
            "title": format!("Seed Book {}", i),
            "price": 9.99,
            "authorId": "bench-author-1",
        });
        let _ = client
            .post(format!("{}/app-benchmarks/{}/", base_url, book_table))
            .basic_auth(auth_user, Some(auth_pass))
            .json(&body)
            .send()
            .await;
    }
    tracing::info!("Seeded {} {} records", count, book_table);
}

pub async fn run(args: BenchArgs) {
    crate::common::init_tracing();
    let (auth_user, auth_pass) = args.auth_parts();
    let auth_user = auth_user.to_string();
    let auth_pass = auth_pass.to_string();
    let client = client::build_client();
    let duration = Duration::from_secs(args.duration);
    let warmup = Duration::from_secs(args.warmup);

    tracing::info!(
        "load-rest: test={}, duration={}s, warmup={}s, vus={}, mode={}, base={}",
        args.test,
        args.duration,
        args.warmup,
        args.vus,
        args.mode,
        args.primary_url()
    );

    match args.test.as_str() {
        "rest-read" | "rest-read-ramp" | "rest-read-sustained" => {
            let book_table = "ReadBook";
            let author_table = "ReadAuthor";
            write_phase(&args, "seeding");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
            seed_author(author_table, &client, args.primary_url(), &auth_user, &auth_pass).await;
            seed_books(book_table, &client, args.primary_url(), &auth_user, &auth_pass, 1000).await;
            let ids = fetch_book_ids(book_table, &client, args.primary_url(), &auth_user, &auth_pass, 10_000).await;
            if ids.is_empty() {
                tracing::error!("Failed to seed {} records.", book_table);
                std::process::exit(1);
            }
            tracing::info!("Setup: {} {} IDs ready for read test", ids.len(), book_table);
            let ids = Arc::new(ids);

            let book_table_owned = book_table.to_string();
            let scenario = move |ctx: Arc<runner::ScenarioContext>| {
                let ids = ids.clone();
                let book_table = book_table_owned.clone();
                async move {
                    let idx = ctx.next_request_idx() as usize % ids.len();
                    let id = &ids[idx];
                    let url = format!(
                        "{}/app-benchmarks/{}/{}?select=id,title",
                        ctx.base_url, book_table, id
                    );
                    let start = std::time::Instant::now();
                    let result = ctx
                        .client
                        .get(&url)
                        .basic_auth(&ctx.auth_user, Some(&ctx.auth_pass))
                        .send()
                        .await;
                    ctx.record_response(start, result).await;
                }
            };

            write_phase(&args, "warming");
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = if args.is_ramp() {
                runner::run_ramp_test(
                    duration,
                    warmup,
                    runner::RampConfig {
                        start_vus: args.start_vus,
                        step_vus: args.step_vus,
                        step_interval: Duration::from_secs(args.step_interval),
                        max_vus: args.max_vus,
                    },
                    LoadTestConfig {
                        client: client.clone(),
                        base_url: args.base_url.clone(),
                        auth_user: auth_user.clone(),
                        auth_pass: auth_pass.clone(),
                    },
                    scenario,
                )
                .await
            } else {
                runner::run_load_test(
                    args.vus,
                    duration,
                    warmup,
                    LoadTestConfig {
                        client: client.clone(),
                        base_url: args.base_url.clone(),
                        auth_user: auth_user.clone(),
                        auth_pass: auth_pass.clone(),
                    },
                    scenario,
                )
                .await
            };

            let summary = metrics.summary(elapsed);
            validate_error_rate(&summary);
            let rctx = ReportContext {
                client: &client,
                base_url: &args.report_url,
                auth_user: &auth_user,
                auth_pass: &auth_pass,
            };
            reporter::report_results_with_snapshots(
                &rctx, &args.test, elapsed, &summary, &snapshots, args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
        },
        "rest-write" | "rest-write-ramp" | "rest-write-sustained" => {
            let book_table = "WriteBook";
            let author_table = "WriteAuthor";
            write_phase(&args, "seeding");
            tracing::info!("Clearing {} + {} tables...", book_table, author_table);
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
            seed_author(author_table, &client, args.primary_url(), &auth_user, &auth_pass).await;
            write_phase(&args, "warming");
            let book_table_owned = book_table.to_string();
            let scenario = move |ctx: Arc<runner::ScenarioContext>| {
                let book_table = book_table_owned.clone();
                async move {
                    let id = Uuid::new_v4().to_string();
                    let body = serde_json::json!({
                        "id": id,
                        "title": format!("Bench Book {}", &id[..8]),
                        "price": 9.99,
                        "authorId": "bench-author-1",
                    });
                    let url = format!("{}/app-benchmarks/{}/", ctx.base_url, book_table);
                    let start = std::time::Instant::now();
                    let result = ctx
                        .client
                        .post(&url)
                        .basic_auth(&ctx.auth_user, Some(&ctx.auth_pass))
                        .json(&body)
                        .send()
                        .await;
                    ctx.record_response(start, result).await;
                }
            };

            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = if args.is_ramp() {
                runner::run_ramp_test(
                    duration,
                    warmup,
                    runner::RampConfig {
                        start_vus: args.start_vus,
                        step_vus: args.step_vus,
                        step_interval: Duration::from_secs(args.step_interval),
                        max_vus: args.max_vus,
                    },
                    LoadTestConfig {
                        client: client.clone(),
                        base_url: args.base_url.clone(),
                        auth_user: auth_user.clone(),
                        auth_pass: auth_pass.clone(),
                    },
                    scenario,
                )
                .await
            } else {
                runner::run_load_test(
                    args.vus,
                    duration,
                    warmup,
                    LoadTestConfig {
                        client: client.clone(),
                        base_url: args.base_url.clone(),
                        auth_user: auth_user.clone(),
                        auth_pass: auth_pass.clone(),
                    },
                    scenario,
                )
                .await
            };

            let summary = metrics.summary(elapsed);
            validate_error_rate(&summary);

            let successful_writes = summary.total - summary.errors;
            let extra = verify_write_results(
                book_table,
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                successful_writes,
            )
            .await;

            let rctx = ReportContext {
                client: &client,
                base_url: &args.report_url,
                auth_user: &auth_user,
                auth_pass: &auth_pass,
            };
            reporter::report_results_full(
                &rctx, &args.test, elapsed, &summary, extra, &snapshots, args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
        },
        "rest-batch-write" => {
            let book_table = "BatchWriteBook";
            write_phase(&args, "seeding");
            clear_tables(&client, args.primary_url(), &auth_user, &auth_pass, "app-benchmarks", &[book_table]).await;

            write_phase(&args, "warming");
            let batch_size = 100usize;
            let book_table_owned = book_table.to_string();
            let scenario = move |ctx: Arc<runner::ScenarioContext>| {
                let book_table = book_table_owned.clone();
                async move {
                    // Build a batch of 100 records as a JSON array
                    let records: Vec<serde_json::Value> = (0..batch_size)
                        .map(|_| {
                            let id = Uuid::new_v4().to_string();
                            serde_json::json!({
                                "id": id,
                                "title": format!("Batch {}", &id[..8]),
                                "price": 9.99,
                                "authorId": "bench-author-1",
                            })
                        })
                        .collect();
                    let body = serde_json::Value::Array(records);
                    let url = format!("{}/app-benchmarks/{}/", ctx.base_url, book_table);
                    let start = std::time::Instant::now();
                    let result = ctx
                        .client
                        .post(&url)
                        .basic_auth(&ctx.auth_user, Some(&ctx.auth_pass))
                        .json(&body)
                        .send()
                        .await;
                    ctx.record_batch_response(start, result, batch_size as u64).await;
                }
            };

            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus, duration, warmup,
                LoadTestConfig { client: client.clone(), base_url: args.base_url.clone(), auth_user: auth_user.clone(), auth_pass: auth_pass.clone() },
                scenario,
            ).await;

            let summary = metrics.summary(elapsed);
            validate_error_rate(&summary);
            let rctx = ReportContext { client: &client, base_url: &args.report_url, auth_user: &auth_user, auth_pass: &auth_pass };
            reporter::report_results_with_snapshots(&rctx, "rest-batch-write", elapsed, &summary, &snapshots, args.vus).await;

            write_phase(&args, "cleaning");
            clear_tables(&client, args.primary_url(), &auth_user, &auth_pass, "app-benchmarks", &[book_table]).await;
        },
        "rest-update" => {
            let book_table = "UpdateBook";
            let author_table = "UpdateAuthor";
            write_phase(&args, "seeding");
            tracing::info!("Clearing {} + {} tables...", book_table, author_table);
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
            seed_author(author_table, &client, args.primary_url(), &auth_user, &auth_pass).await;
            let record_count = args.vus * 100;
            let record_ids: Vec<String> = (0..record_count)
                .map(|_| Uuid::new_v4().to_string())
                .collect();
            tracing::info!("Setup: creating {} records...", record_count);

            for id in &record_ids {
                let body = serde_json::json!({
                    "id": id,
                    "title": format!("Update Bench {}", &id[..8]),
                    
                    "price": 10.0,
                    "authorId": "bench-author-1",
                });
                let url = format!("{}/app-benchmarks/{}/", args.primary_url(), book_table);
                let _ = client
                    .post(&url)
                    .basic_auth(&auth_user, Some(&auth_pass))
                    .json(&body)
                    .send()
                    .await;
            }
            tracing::info!("Setup complete. Starting load test...");

            write_phase(&args, "warming");
            let ids = Arc::new(record_ids);
            let book_table_owned = book_table.to_string();
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
                    let ids = ids.clone();
                    let book_table = book_table_owned.clone();
                    async move {
                        let idx = ctx.next_request_idx() as usize % ids.len();
                        let id = &ids[idx];
                        let price: f64 = rand::random::<f64>() * 100.0;
                        let body = serde_json::json!({ "price": price });
                        let url = format!("{}/app-benchmarks/{}/{}", ctx.base_url, book_table, id);
                        let start = std::time::Instant::now();
                        let result = ctx
                            .client
                            .patch(&url)
                            .basic_auth(&ctx.auth_user, Some(&ctx.auth_pass))
                            .json(&body)
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
                "rest-update",
                elapsed,
                &summary,
                &snapshots,
                args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
        },
        "rest-join" => {
            let book_table = "JoinBook";
            let author_table = "JoinAuthor";
            write_phase(&args, "seeding");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
            seed_author(author_table, &client, args.primary_url(), &auth_user, &auth_pass).await;
            seed_books(book_table, &client, args.primary_url(), &auth_user, &auth_pass, 1000).await;
            let ids = fetch_book_ids(book_table, &client, args.primary_url(), &auth_user, &auth_pass, 10_000).await;
            if ids.is_empty() {
                tracing::error!("Failed to seed {} records.", book_table);
                std::process::exit(1);
            }
            tracing::info!("Setup: {} {} IDs ready for join test", ids.len(), book_table);
            let ids = Arc::new(ids);

            write_phase(&args, "warming");
            let book_table_owned = book_table.to_string();
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
                    let ids = ids.clone();
                    let book_table = book_table_owned.clone();
                    async move {
                        let idx = ctx.next_request_idx() as usize % ids.len();
                        let id = &ids[idx];
                        let url = format!(
                            "{}/app-benchmarks/{}/{}?select=id,title,author%7Bname%7D",
                            ctx.base_url, book_table, id
                        );
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
                "rest-join",
                elapsed,
                &summary,
                &snapshots,
                args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                args.primary_url(),
                &auth_user,
                &auth_pass,
                "app-benchmarks",
                &[book_table, author_table],
            )
            .await;
        },
        other => {
            tracing::error!("Unknown test for load-rest: {}", other);
            std::process::exit(1);
        },
    }
}
