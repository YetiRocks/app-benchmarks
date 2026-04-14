//! load-vector benchmark — extracted from bin/load_vector.rs for embedding in yeti CLI.

use crate::{
    common::LoadTestConfig, common::ReportContext, common::clear_tables,
    cli::{BenchArgs, write_phase},
    client, reporter, runner, common::validate_error_rate,
};
use std::time::Duration;
use uuid::Uuid;

const SAMPLE_TOPICS: &[&str] = &[
    "technology innovation artificial intelligence",
    "climate change renewable energy sustainability",
    "space exploration mars colonization",
    "quantum computing breakthroughs",
    "biotechnology gene editing crispr",
    "ocean conservation marine biology",
    "autonomous vehicles self driving cars",
    "blockchain decentralized finance",
    "neuroscience brain computer interfaces",
    "cybersecurity threat detection",
];

/// Simple percent-encoding for query params.
fn urlencoding(s: String) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            },
            _ => {
                out.push_str(&format!("%{:02X}", b));
            },
        }
    }
    out
}

pub async fn run(args: BenchArgs) {
    crate::common::init_tracing();
    let client = client::build_client();
    let duration = Duration::from_secs(args.duration);
    let warmup = Duration::from_secs(args.warmup);

    tracing::info!(
        "load-vector: test={}, duration={}s, warmup={}s, vus={}, base={}",
        args.test,
        args.duration,
        args.warmup,
        args.vus,
        args.primary_url()
    );

    match args.test.as_str() {
        "vector-embed" => {
            let article_table = "EmbedArticle";
            write_phase(&args, "seeding");
            tracing::info!("Clearing {} table...", article_table);
            clear_tables(
                &client,
                args.primary_url(),
                "app-benchmarks",
                &args.route,
                &[article_table],
            )
            .await;

            write_phase(&args, "warming");
            let article_table_owned = article_table.to_string();
            let route = args.route.clone();
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus,
                duration,
                warmup,
                LoadTestConfig {
                    client: client.clone(),
                    base_url: args.base_url.clone(),
                },
                move |ctx| {
                    let article_table = article_table_owned.clone();
                    let route = route.clone();
                    async move {
                        let id = Uuid::new_v4().to_string();
                        let topic_idx = ctx.next_request_idx() as usize % SAMPLE_TOPICS.len();
                        let body = serde_json::json!({
                            "id": id,
                            "title": format!("Vector Article {}", &id[..8]),
                            "author": "Benchmark",
                            "category": "benchmark",
                            "content": format!(
                                "This article explores {}. Generated for benchmark testing with unique content to trigger embedding computation. ID: {}",
                                SAMPLE_TOPICS[topic_idx], id
                            ),
                        });
                        let url = if route.is_empty() {
                            format!("{}/app-benchmarks/{}/", ctx.base_url, article_table)
                        } else {
                            format!("{}/app-benchmarks/{}/{}/", ctx.base_url, route, article_table)
                        };
                        let start = std::time::Instant::now();
                        let result = ctx.client.post(&url).json(&body).send().await;
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
                route: &args.route,
            };
            reporter::report_results_with_snapshots(
                &rctx,
                "vector-embed",
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
                "app-benchmarks",
                &args.route,
                &[article_table],
            )
            .await;
        },
        "vector-search" => {
            let article_table = "SearchArticle";
            write_phase(&args, "seeding");
            tracing::info!("Seeding 50 {} records for vector search...", article_table);
            clear_tables(
                &client,
                args.primary_url(),
                "app-benchmarks",
                &args.route,
                &[article_table],
            )
            .await;
            for i in 0..50 {
                let id = Uuid::new_v4().to_string();
                let topic_idx = i % SAMPLE_TOPICS.len();
                let body = serde_json::json!({
                    "id": id,
                    "title": format!("Seed Article {}", i),
                    "author": "Benchmark",
                    "category": "benchmark",
                    "content": format!(
                        "This article explores {}. Seed content for vector search benchmarking. ID: {}",
                        SAMPLE_TOPICS[topic_idx], id
                    ),
                });
                let url = args.table_url(args.primary_url(), article_table);
                let _ = client.post(format!("{}/", url)).json(&body).send().await;
            }
            tracing::info!("Waiting for embeddings to process...");
            tokio::time::sleep(Duration::from_secs(5)).await;

            write_phase(&args, "warming");
            let article_table_owned = article_table.to_string();
            let route = args.route.clone();
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus,
                duration,
                warmup,
                LoadTestConfig {
                    client: client.clone(),
                    base_url: args.base_url.clone(),
                },
                move |ctx| {
                    let article_table = article_table_owned.clone();
                    let route = route.clone();
                    async move {
                        let topic_idx = ctx.next_request_idx() as usize % SAMPLE_TOPICS.len();
                        let query = serde_json::json!({
                            "conditions": [{
                                "field": "embedding",
                                "op": "vector",
                                "value": SAMPLE_TOPICS[topic_idx]
                            }],
                            "limit": 10
                        });
                        let url = if route.is_empty() {
                            format!(
                                "{}/app-benchmarks/{}/?query={}",
                                ctx.base_url,
                                article_table,
                                urlencoding(query.to_string())
                            )
                        } else {
                            format!(
                                "{}/app-benchmarks/{}/{}/?query={}",
                                ctx.base_url,
                                route,
                                article_table,
                                urlencoding(query.to_string())
                            )
                        };
                        let start = std::time::Instant::now();
                        let result = ctx.client.get(&url).send().await;
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
                route: &args.route,
            };
            reporter::report_results_with_snapshots(
                &rctx,
                "vector-search",
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
                "app-benchmarks",
                &args.route,
                &[article_table],
            )
            .await;
        },
        other => {
            tracing::error!("Unknown test for load-vector: {}", other);
            std::process::exit(1);
        },
    }
}
