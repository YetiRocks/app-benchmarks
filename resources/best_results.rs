//! Dashboard catalog endpoint. GET returns the test definition map
//! the React UI consumes — each entry's `vus` is None unless the
//! test deliberately overrides the platform default (only the
//! realtime fan-out tests do; the rest pick `defaultVus` from the
//! top-level response).
//!
//! Best-per-test merging from the TestRun table is deferred to v2;
//! the dashboard falls back to "no best yet" for empty cells.

use yeti_sdk::prelude::*;

const DEFAULT_VUS: u64 = 100;

// (id, name, binary, duration, vus_override, category)
const TESTS: &[(&str, &str, &str, u64, Option<u64>, &str)] = &[
    ("rest-read", "REST Read", "load-rest", 30, None, "throughput"),
    ("rest-write", "REST Write", "load-rest", 30, None, "throughput"),
    ("rest-batch-write", "REST Batch Write", "load-rest", 30, None, "throughput"),
    ("rest-update", "REST Update", "load-rest", 30, None, "throughput"),
    ("rest-join", "REST Join", "load-rest", 30, None, "throughput"),
    ("graphql-read", "GraphQL Read", "load-graphql", 30, None, "throughput"),
    ("graphql-mutation", "GraphQL Write", "load-graphql", 30, None, "throughput"),
    ("graphql-batch-write", "GraphQL Batch Write", "load-graphql", 30, None, "throughput"),
    ("graphql-update", "GraphQL Update", "load-graphql", 30, None, "throughput"),
    ("graphql-join", "GraphQL Join", "load-graphql", 30, None, "throughput"),
    ("vector-embed", "Vector Embed", "load-vector", 30, None, "throughput"),
    ("vector-search", "Vector Search", "load-vector", 30, None, "throughput"),
    ("blob-retrieval", "150k Blob Retrieval", "load-blob", 30, None, "throughput"),
    ("ws", "WS Fan-Out", "load-realtime", 30, Some(15_000), "throughput"),
    ("ws-publish", "WS Fan-In", "load-realtime", 30, None, "throughput"),
    ("sse", "SSE Fan-Out", "load-realtime", 30, Some(15_000), "throughput"),
    ("mqtt", "MQTT Fan-Out", "load-realtime", 30, Some(15_000), "throughput"),
];

resource!(BestResults {
    name = "bestresults",

    get(_ctx) => {
        let mut tests = serde_json::Map::new();
        for (i, &(id, name, binary, duration, vus_override, category)) in TESTS.iter().enumerate() {
            let mut entry = json!({
                "id": id,
                "name": name,
                "binary": binary,
                "duration": duration,
                "category": category,
                "order": i,
            });
            if let Some(v) = vus_override {
                entry["vus"] = json!(v);
            }
            tests.insert(id.to_owned(), entry);
        }
        json!({
            "tests": tests,
            "defaultVus": DEFAULT_VUS,
            "categories": [
                { "category": "throughput", "label": "Throughput - 30s" },
            ],
        })
    },
});
