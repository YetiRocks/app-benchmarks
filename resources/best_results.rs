// Best Results Resource
//
// Returns all test definitions with their best results as a keyed object.
// Tests with no results are included (for empty card display).
//
// GET  /app-benchmarks/bestresults → { tests: { "rest-read": { id, name, ... , best: {...} }, ... }, categories: [...] }
// DELETE /app-benchmarks/bestresults → deletes all TestRun records

use yeti_sdk::prelude::*;

// Test definitions — single source of truth (matches benchmark_runner.rs TESTS)
const TESTS: &[(&str, &str, &str, u64, u64, &str)] = &[
    // (id, name, binary, duration, vus, category)
    ("rest-read", "REST Read", "load-rest", 30, 100, "throughput"),
    ("rest-write", "REST Write", "load-rest", 30, 100, "throughput"),
    ("rest-batch-write", "REST Batch Write", "load-rest", 30, 100, "throughput"),
    ("rest-update", "REST Update", "load-rest", 30, 100, "throughput"),
    ("rest-join", "REST Join", "load-rest", 30, 100, "throughput"),
    ("graphql-read", "GraphQL Read", "load-graphql", 30, 100, "throughput"),
    ("graphql-mutation", "GraphQL Write", "load-graphql", 30, 100, "throughput"),
    ("graphql-batch-write", "GraphQL Batch Write", "load-graphql", 30, 100, "throughput"),
    ("graphql-update", "GraphQL Update", "load-graphql", 30, 100, "throughput"),
    ("graphql-join", "GraphQL Join", "load-graphql", 30, 100, "throughput"),
    ("vector-embed", "Vector Embed", "load-vector", 30, 10, "throughput"),
    ("vector-search", "Vector Search", "load-vector", 30, 100, "throughput"),
    ("blob-retrieval", "150k Blob Retrieval", "load-blob", 30, 100, "throughput"),
    ("ws", "WS Fan-Out", "load-realtime", 30, 15_000, "throughput"),
    ("ws-publish", "WS Fan-In", "load-realtime", 30, 100, "throughput"),
    ("sse", "SSE Fan-Out", "load-realtime", 30, 15_000, "throughput"),
    ("mqtt", "MQTT Fan-Out", "load-realtime", 30, 15_000, "throughput"),
];

resource!(BestResults {
    name = "bestresults",
    get(_request, ctx) => {
        // Fetch all TestRun records and find best throughput per test
        let runs = match ctx.get_table("TestRun") {
            Ok(table) => table.get_all().await.unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        // Group runs by (testName, runGroup) and aggregate parallel processes
        let mut groups: HashMap<(String, String), Vec<Value>> = HashMap::new();
        for run in &runs {
            let test_name = match run.get("testName").and_then(|v| v.as_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            let results_str = run.get("results").and_then(|v| v.as_str()).unwrap_or("{}");
            let results: Value = serde_json::from_str(results_str).unwrap_or(json!({}));
            let total = results.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            if total == 0 { continue; }
            let errors = results.get("errors").and_then(|v| v.as_u64()).unwrap_or(0);
            if (errors as f64 / total as f64) > 0.01 { continue; }

            let group = run.get("runGroup").and_then(|v| v.as_str())
                .unwrap_or("solo").to_string();
            groups.entry((test_name, group)).or_default().push(results);
        }

        // Aggregate each group: sum throughput/total/errors, weighted-avg latencies
        let mut best_by_test: HashMap<String, Value> = HashMap::new();
        for ((test_name, _group), results_vec) in &groups {
            let mut agg_throughput = 0.0;
            let mut agg_total = 0u64;
            let mut agg_errors = 0u64;
            let mut weighted_p50 = 0.0;
            let mut weighted_p95 = 0.0;
            let mut weighted_p99 = 0.0;

            for r in results_vec {
                let t = r.get("throughput").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let n = r.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as f64;
                agg_throughput += t;
                agg_total += r.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                agg_errors += r.get("errors").and_then(|v| v.as_u64()).unwrap_or(0);
                weighted_p50 += r.get("p50").and_then(|v| v.as_f64()).unwrap_or(0.0) * n;
                weighted_p95 += r.get("p95").and_then(|v| v.as_f64()).unwrap_or(0.0) * n;
                weighted_p99 += r.get("p99").and_then(|v| v.as_f64()).unwrap_or(0.0) * n;
            }

            let total_f = agg_total as f64;
            let aggregated = json!({
                "throughput": (agg_throughput * 10.0).round() / 10.0,
                "p50": if total_f > 0.0 { (weighted_p50 / total_f * 100.0).round() / 100.0 } else { 0.0 },
                "p95": if total_f > 0.0 { (weighted_p95 / total_f * 100.0).round() / 100.0 } else { 0.0 },
                "p99": if total_f > 0.0 { (weighted_p99 / total_f * 100.0).round() / 100.0 } else { 0.0 },
                "total": agg_total,
                "errors": agg_errors,
                "nodes": results_vec.len(),
            });

            let is_better = match best_by_test.get(test_name) {
                Some(existing) => agg_throughput > existing.get("throughput").and_then(|v| v.as_f64()).unwrap_or(0.0),
                None => true,
            };
            if is_better {
                best_by_test.insert(test_name.clone(), aggregated);
            }
        }

        // Build keyed object: every test definition included, with best metrics if available
        let mut tests = serde_json::Map::new();
        for (i, &(id, name, binary, duration, vus, category)) in TESTS.iter().enumerate() {
            let mut entry = json!({
                "id": id,
                "name": name,
                "binary": binary,
                "duration": duration,
                "vus": vus,
                "category": category,
                "order": i,
            });
            if let Some(best) = best_by_test.get(id) {
                entry["best"] = best.clone();
            }
            tests.insert(id.to_string(), entry);
        }

        reply().json(json!({
            "tests": tests,
            "categories": [
                { "category": "throughput", "label": "Throughput - 30s" },
            ],
        }))
    },
    delete(_request, ctx) => {
        let table = ctx.get_table("TestRun")?;
        let count = table.delete_all().await?;
        reply().json(json!({ "deleted": count }))
    }
});
