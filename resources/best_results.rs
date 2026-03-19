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
    ("rest-read", "REST Reads", "load-rest", 30, 100, "throughput"),
    ("rest-write", "REST Writes", "load-rest", 30, 100, "throughput"),
    ("rest-update", "REST Update", "load-rest", 30, 100, "throughput"),
    ("rest-join", "REST Join", "load-rest", 30, 50, "throughput"),
    ("graphql-read", "GraphQL Reads", "load-graphql", 30, 100, "throughput"),
    ("graphql-mutation", "GraphQL Mutations", "load-graphql", 30, 100, "throughput"),
    ("graphql-join", "GraphQL Join", "load-graphql", 30, 50, "throughput"),
    ("vector-embed", "Vector Embed", "load-vector", 30, 10, "throughput"),
    ("vector-search", "Vector Search", "load-vector", 30, 100, "throughput"),
    ("blob-retrieval", "150k Blob Retrieval", "load-blob", 30, 100, "throughput"),
    ("ws", "WebSocket", "load-realtime", 30, 1000, "throughput"),
    ("sse", "SSE Streaming", "load-realtime", 30, 1000, "throughput"),
    ("rest-read-sustained", "REST Reads", "load-rest", 300, 100, "sustained"),
    ("rest-write-sustained", "REST Writes", "load-rest", 300, 100, "sustained"),
    ("rest-update-sustained", "REST Update", "load-rest", 300, 100, "sustained"),
    ("rest-join-sustained", "REST Join", "load-rest", 300, 50, "sustained"),
    ("graphql-read-sustained", "GraphQL Reads", "load-graphql", 300, 100, "sustained"),
    ("graphql-mutation-sustained", "GraphQL Mutations", "load-graphql", 300, 100, "sustained"),
    ("graphql-join-sustained", "GraphQL Join", "load-graphql", 300, 50, "sustained"),
    ("vector-embed-sustained", "Vector Embed", "load-vector", 300, 10, "sustained"),
    ("vector-search-sustained", "Vector Search", "load-vector", 300, 100, "sustained"),
    ("blob-retrieval-sustained", "150k Blob Retrieval", "load-blob", 300, 100, "sustained"),
    ("ws-sustained", "WebSocket", "load-realtime", 300, 1000, "sustained"),
    ("sse-sustained", "SSE Streaming", "load-realtime", 300, 1000, "sustained"),
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
                { "category": "sustained", "label": "Sustained - 5m" },
            ],
        }))
    },
    delete(_request, ctx) => {
        let table = ctx.get_table("TestRun")?;
        let count = table.delete_all().await?;
        reply().json(json!({ "deleted": count }))
    }
});
