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

        let mut best_by_test: HashMap<String, Value> = HashMap::new();
        for run in &runs {
            let test_name = match run.get("testName").and_then(|v| v.as_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            let results_str = run.get("results").and_then(|v| v.as_str()).unwrap_or("{}");
            let results: Value = serde_json::from_str(results_str).unwrap_or(json!({}));
            let throughput = results.get("throughput").and_then(|v| v.as_f64()).unwrap_or(0.0);

            // Skip runs with no results or >1% error rate
            let total = results.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            if total == 0 { continue; }
            let errors = results.get("errors").and_then(|v| v.as_u64()).unwrap_or(0);
            if (errors as f64 / total as f64) > 0.01 { continue; }

            let is_better = match best_by_test.get(&test_name) {
                Some(existing) => throughput > existing.get("throughput").and_then(|v| v.as_f64()).unwrap_or(0.0),
                None => true,
            };
            if is_better {
                best_by_test.insert(test_name, results);
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
