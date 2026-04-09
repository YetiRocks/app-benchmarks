//! load-graphql benchmark — extracted from bin/load_graphql.rs for embedding in yeti CLI.

use crate::{
    common::LoadTestConfig, common::ReportContext, common::clear_tables,
    cli::{BenchArgs, write_phase},
    client, common::fetch_book_ids, reporter, runner, common::validate_error_rate,
};
use std::sync::Arc;
use std::time::Duration;
use rand;
use uuid::Uuid;

/// Build a graphql endpoint URL: {base_url}/app-benchmarks/{route}/graphql
fn graphql_url(base_url: &str, route: &str) -> String {
    if route.is_empty() {
        format!("{}/app-benchmarks/graphql", base_url)
    } else {
        format!("{}/app-benchmarks/{}/graphql", base_url, route)
    }
}

/// Ensure a benchmark Author record exists for join tests.
async fn seed_author(table: &str, client: &reqwest::Client, base_url: &str, route: &str) {
    let body = serde_json::json!({
        "id": "bench-author-1",
        "name": "Benchmark Author",
    });
    let url = if route.is_empty() {
        format!("{}/app-benchmarks/{}/", base_url, table)
    } else {
        format!("{}/app-benchmarks/{}/{}/", base_url, route, table)
    };
    let _ = client.post(url).json(&body).send().await;
}

/// Seed N Book records via GraphQL mutations for read tests.
async fn seed_books_graphql(
    book_type: &str,
    client: &reqwest::Client,
    base_url: &str,
    route: &str,
    count: usize,
) {
    let url = graphql_url(base_url, route);
    let create_fn = format!("create{}", book_type);
    for i in 0..count {
        let id = Uuid::new_v4().to_string();
        let mutation = format!(
            r#"mutation {{ {}(input: {{ id: "{}", title: "Seed Book {}", price: 9.99, authorId: "bench-author-1" }}) {{ id }} }}"#,
            create_fn, id, i
        );
        let query = serde_json::json!({ "query": mutation });
        let _ = client.post(&url).json(&query).send().await;
    }
    tracing::info!("Seeded {} {} records via GraphQL", count, book_type);
}

pub async fn run(args: BenchArgs) {
    crate::common::init_tracing();
    let client = client::build_client();
    let duration = Duration::from_secs(args.duration);
    let warmup = Duration::from_secs(args.warmup);

    tracing::info!(
        "load-graphql: test={}, duration={}s, warmup={}s, vus={}, mode={}, base={}",
        args.test,
        args.duration,
        args.warmup,
        args.vus,
        args.mode,
        args.primary_url()
    );

    match args.test.as_str() {
        "graphql-read" | "graphql-read-ramp" | "graphql-read-sustained" => {
            let book_table = "GqlReadBook";
            let author_table = "GqlReadAuthor";
            write_phase(&args, "seeding");
            for url in args.all_urls() {
                clear_tables(&client, url, "app-benchmarks", &args.route, &[book_table, author_table]).await;
                seed_author(author_table, &client, url, &args.route).await;
                seed_books_graphql(book_table, &client, url, &args.route, 1000).await;
            }
            let ids = fetch_book_ids(book_table, &client, args.primary_url(), &args.route, 1000).await;
            if ids.is_empty() {
                tracing::error!("Failed to seed {} records.", book_table);
                std::process::exit(1);
            }
            tracing::info!("Setup: {} {} IDs ready for read test", ids.len(), book_table);
            let ids = Arc::new(ids);

            let book_table_owned = book_table.to_string();
            let route = args.route.clone();
            let scenario = move |ctx: Arc<runner::ScenarioContext>| {
                let ids = ids.clone();
                let book_table = book_table_owned.clone();
                let route = route.clone();
                async move {
                    let idx = ctx.next_request_idx() as usize % ids.len();
                    let id = &ids[idx];
                    let query = serde_json::json!({
                        "query": format!("{{ {}(id: \"{}\") {{ id title price }} }}", book_table, id)
                    });
                    let url = graphql_url(&ctx.base_url, &route);
                    let start = std::time::Instant::now();
                    let result = ctx.client.post(&url).json(&query).send().await;
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
                route: &args.route,
                run_group: args.run_group.as_deref(),
            };
            reporter::report_results_with_snapshots(
                &rctx, &args.test, elapsed, &summary, &snapshots, args.vus,
            )
            .await;

            write_phase(&args, "cleaning");
            clear_tables(
                &client,
                args.primary_url(),
                "app-benchmarks",
                &args.route,
                &[book_table, author_table],
            )
            .await;
        },
        "graphql-mutation" => {
            let book_table = "GqlWriteBook";
            let author_table = "GqlWriteAuthor";
            write_phase(&args, "seeding");
            tracing::info!("Clearing {} + {} tables...", book_table, author_table);
            clear_tables(
                &client,
                args.primary_url(),
                "app-benchmarks",
                &args.route,
                &[book_table, author_table],
            )
            .await;

            write_phase(&args, "warming");
            let book_table_owned = book_table.to_string();
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
                    let book_table = book_table_owned.clone();
                    let route = route.clone();
                    async move {
                        let id = Uuid::new_v4().to_string();
                        let create_fn = format!("create{}", book_table);
                        let mutation = format!(
                            r#"mutation {{ {}(input: {{ id: "{}", title: "GQL Bench {}", price: 9.99, authorId: "bench-author-1" }}) {{ id }} }}"#,
                            create_fn, id, &id[..8]
                        );
                        let query = serde_json::json!({ "query": mutation });
                        let url = graphql_url(&ctx.base_url, &route);
                        let start = std::time::Instant::now();
                        let result = ctx.client.post(&url).json(&query).send().await;
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
                run_group: args.run_group.as_deref(),
            };
            reporter::report_results_with_snapshots(
                &rctx,
                "graphql-mutation",
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
                &[book_table, author_table],
            )
            .await;
        },
        "graphql-batch-write" => {
            let book_table = "GqlBatchWriteBook";
            write_phase(&args, "seeding");
            clear_tables(&client, args.primary_url(), "app-benchmarks", &args.route, &[book_table]).await;

            write_phase(&args, "warming");
            let batch_size = 100usize;
            let book_table_owned = book_table.to_string();
            let route = args.route.clone();
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus, duration, warmup,
                LoadTestConfig { client: client.clone(), base_url: args.base_url.clone() },
                move |ctx| {
                    let book_table = book_table_owned.clone();
                    let route = route.clone();
                    async move {
                        // Build N aliased create mutations in one GraphQL request
                        let create_fn = format!("create{}", book_table);
                        let mutations: Vec<String> = (0..batch_size)
                            .map(|i| {
                                let id = Uuid::new_v4().to_string();
                                format!(
                                    r#"m{i}: {create_fn}(input: {{ id: "{id}", title: "Batch {short}", price: 9.99, authorId: "bench-author-1" }}) {{ id }}"#,
                                    i = i, create_fn = create_fn, id = id, short = &id[..8]
                                )
                            })
                            .collect();
                        let query = format!("mutation {{ {} }}", mutations.join(" "));
                        let body = serde_json::json!({ "query": query });
                        let url = graphql_url(&ctx.base_url, &route);
                        let start = std::time::Instant::now();
                        let result = ctx.client.post(&url).json(&body).send().await;
                        ctx.record_batch_response(start, result, batch_size as u64).await;
                    }
                },
            ).await;

            let summary = metrics.summary(elapsed);
            validate_error_rate(&summary);
            let rctx = ReportContext { client: &client, base_url: &args.report_url, route: &args.route, run_group: args.run_group.as_deref() };
            reporter::report_results_with_snapshots(&rctx, "graphql-batch-write", elapsed, &summary, &snapshots, args.vus).await;

            write_phase(&args, "cleaning");
            clear_tables(&client, args.primary_url(), "app-benchmarks", &args.route, &[book_table]).await;
        },
        "graphql-update" => {
            let book_table = "GqlUpdateBook";
            write_phase(&args, "seeding");
            clear_tables(&client, args.primary_url(), "app-benchmarks", &args.route, &[book_table]).await;

            // Seed records to update
            let record_count = 1000;
            let record_ids: Vec<String> = (0..record_count).map(|_| Uuid::new_v4().to_string()).collect();
            tracing::info!("Seeding {} records for update test...", record_count);
            let create_fn = format!("create{}", book_table);
            let gql_url = graphql_url(args.primary_url(), &args.route);
            for id in &record_ids {
                let mutation = format!(
                    r#"mutation {{ {}(input: {{ id: "{}", title: "Update Bench {}", price: 10.0, authorId: "bench-author-1" }}) {{ id }} }}"#,
                    create_fn, id, &id[..8]
                );
                let _ = client.post(&gql_url).json(&serde_json::json!({ "query": mutation }))
                    .send().await;
            }
            tracing::info!("Seeded {} records", record_count);
            let ids = Arc::new(record_ids);

            write_phase(&args, "warming");
            let book_table_owned = book_table.to_string();
            let route = args.route.clone();
            let (metrics, elapsed, snapshots): (_, _, Vec<_>) = runner::run_load_test(
                args.vus, duration, warmup,
                LoadTestConfig { client: client.clone(), base_url: args.base_url.clone() },
                move |ctx| {
                    let ids = ids.clone();
                    let book_table = book_table_owned.clone();
                    let route = route.clone();
                    async move {
                        let idx = ctx.next_request_idx() as usize % ids.len();
                        let id = &ids[idx];
                        let update_fn = format!("update{}", book_table);
                        let price: f64 = rand::random::<f64>() * 100.0;
                        let mutation = format!(
                            r#"mutation {{ {}(id: "{}", input: {{ price: {:.2} }}) {{ id }} }}"#,
                            update_fn, id, price
                        );
                        let query = serde_json::json!({ "query": mutation });
                        let url = graphql_url(&ctx.base_url, &route);
                        let start = std::time::Instant::now();
                        let result = ctx.client.post(&url).json(&query).send().await;
                        ctx.record_response(start, result).await;
                    }
                },
            ).await;
            let summary = metrics.summary(elapsed);
            crate::common::validate_error_rate(&summary);
            reporter::report_results_with_snapshots(
                &ReportContext { client: &client, base_url: &args.report_url, route: &args.route, run_group: args.run_group.as_deref() },
                "graphql-update", elapsed, &summary, &snapshots, args.vus,
            ).await;

            write_phase(&args, "cleaning");
            clear_tables(&client, args.primary_url(), "app-benchmarks", &args.route, &[book_table]).await;
        },
        "graphql-join" => {
            let book_table = "GqlJoinBook";
            let author_table = "GqlJoinAuthor";
            write_phase(&args, "seeding");
            for url in args.all_urls() {
                clear_tables(&client, url, "app-benchmarks", &args.route, &[book_table, author_table]).await;
                seed_author(author_table, &client, url, &args.route).await;
                seed_books_graphql(book_table, &client, url, &args.route, 1000).await;
            }
            let ids = fetch_book_ids(book_table, &client, args.primary_url(), &args.route, 1000).await;
            if ids.is_empty() {
                tracing::error!("Failed to seed {} records.", book_table);
                std::process::exit(1);
            }
            tracing::info!("Setup: {} {} IDs ready for join test", ids.len(), book_table);
            let ids = Arc::new(ids);

            write_phase(&args, "warming");
            let book_table_owned = book_table.to_string();
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
                    let ids = ids.clone();
                    let book_table = book_table_owned.clone();
                    let route = route.clone();
                    async move {
                        let idx = ctx.next_request_idx() as usize % ids.len();
                        let id = &ids[idx];
                        let query_str = format!(
                            r#"{{ {}(id: "{}") {{ id title author {{ name }} }} }}"#,
                            book_table, id
                        );
                        let query = serde_json::json!({ "query": query_str });
                        let url = graphql_url(&ctx.base_url, &route);
                        let start = std::time::Instant::now();
                        let result = ctx.client.post(&url).json(&query).send().await;
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
                run_group: args.run_group.as_deref(),
            };
            reporter::report_results_with_snapshots(
                &rctx,
                "graphql-join",
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
                &[book_table, author_table],
            )
            .await;
        },
        other => {
            tracing::error!("Unknown test for load-graphql: {}", other);
            std::process::exit(1);
        },
    }
}
