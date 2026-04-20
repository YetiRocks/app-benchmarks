// History Resource
//
// Returns historical benchmark runs for a specific test.
//
// GET /app-benchmarks/history/{testName} → array of TestRun records sorted by timestamp desc

use yeti_sdk::prelude::*;

resource!(History {
    name = "history",
    get(ctx) => {
        let test_name = ctx.require_id()?;

        let runs = match ctx.table("TestRun") {
            Ok(table) => table.get_all().await.unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let mut filtered: Vec<&Value> = runs.iter()
            .filter(|r| r.get("testName").and_then(|v| v.as_str()) == Some(&test_name))
            .collect();

        filtered.sort_by(|a, b| {
            let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            tb.cmp(ta)
        });

        ok(json!({ "runs": filtered }))
    }
});
