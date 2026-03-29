<p align="center">
  <img src="https://cdn.prod.website-files.com/68e09cef90d613c94c3671c0/697e805a9246c7e090054706_logo_horizontal_grey.png" alt="Yeti" width="200" />
</p>

---

# app-benchmarks

[![Yeti](https://img.shields.io/badge/Yeti-Application-blue)](https://yetirocks.com)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

> **[Yeti](https://yetirocks.com)** - The Performance Platform for Agent-Driven Development.
> Schema-driven APIs, real-time streaming, and vector search. From prompt to production.

**The automated performance lab for Yeti.** Measure everything. Regress nothing.

app-benchmarks is a self-contained benchmarking suite that measures throughput, latency percentiles, and scalability across every Yeti transport: REST, GraphQL, WebSocket, SSE, MQTT, vector search, and blob retrieval. It compiles native Rust load generators, orchestrates multi-process test runs, captures per-second time-series snapshots, and presents results in a real-time dashboard with historical comparison.

---

## Why app-benchmarks

Performance regressions hide in plain sight. A schema change, a new middleware hook, or a dependency update can silently cut throughput in half. By the time someone notices, the cause is buried under weeks of commits.

app-benchmarks eliminates that blind spot:

- **17 workloads across 6 transports** -- REST CRUD, GraphQL queries and mutations, WebSocket fan-in and fan-out, SSE streaming, MQTT pub/sub, vector embedding and search, and large object retrieval. Every path Yeti serves is measured.
- **Native Rust load generators** -- compiled as standalone binaries via the Yeti plugin system. No external load testing tools (k6, wrk, vegeta). Zero runtime dependencies beyond the compiled binary.
- **HdrHistogram latency tracking** -- microsecond-precision histograms with p50, p95, p99, and p99.9 percentile breakdowns. Per-second interval snapshots for time-series analysis.
- **Multi-process scaling** -- the runner auto-scales process count based on virtual user count (1 process per 5,000 VUs). Each process runs independently against the target, with staggered launches to avoid TLS handshake storms.
- **Multi-target distribution** -- comma-delimited target URLs distribute load round-robin across multiple Yeti instances for cluster benchmarking.
- **Best-result tracking** -- every run is stored with full metrics and per-second snapshots. The dashboard highlights personal bests and regressions at a glance.
- **Telemetry suppression** -- active benchmarks signal the host to suppress telemetry collection via `x-yeti-suppress-telemetry` headers, preventing the observer effect from skewing results.
- **Single binary deployment** -- compiles into native Rust plugins and load test binaries. No Node.js, no npm, no Docker compose for the test harness itself.

---

## Quick Start

### 1. Install

```bash
cd ~/yeti/applications
git clone https://github.com/yetirocks/app-benchmarks.git
```

Restart yeti. The application compiles automatically on first load (~6 minutes for resources and binaries) and is cached for subsequent starts (~10 seconds).

### 2. Open the dashboard

```bash
open https://localhost:9996/app-benchmarks
```

The React dashboard shows all 17 test cards organized by category, with best results and run history.

### 3. Start a benchmark

```bash
curl -X POST https://localhost:9996/app-benchmarks/runner \
  -H "Content-Type: application/json" \
  -d '{ "test": "rest-read" }'
```

Response:
```json
{
  "status": "seeding",
  "testName": "rest-read",
  "processes": 1,
  "vusPerProcess": 100,
  "totalVus": 100,
  "pid": 54321
}
```

The runner spawns the `load-rest` binary, which seeds test data, warms up for 5 seconds, then measures for 30 seconds. Results are automatically stored in the TestRun table.

### 4. Check runner status

```bash
curl https://localhost:9996/app-benchmarks/runner
```

Response (while running):
```json
{
  "status": "running",
  "phase": "running",
  "testName": "rest-read",
  "warmupSecs": 0.0,
  "elapsedSecs": 12.4,
  "configuredDuration": 30,
  "lastError": null
}
```

### 5. View best results

```bash
curl https://localhost:9996/app-benchmarks/bestresults
```

Response:
```json
{
  "tests": {
    "rest-read": {
      "id": "rest-read",
      "name": "REST Read",
      "binary": "load-rest",
      "duration": 30,
      "vus": 100,
      "category": "throughput",
      "order": 0,
      "best": {
        "throughput": 24531.2,
        "p50": 3.82,
        "p95": 8.14,
        "p99": 12.67,
        "p999": 28.41,
        "total": 735936,
        "errors": 0
      }
    },
    "rest-write": { "id": "rest-write", "name": "REST Write", "..." : "..." }
  },
  "categories": [
    { "category": "throughput", "label": "Throughput - 30s" }
  ]
}
```

Tests with no results are included (with no `best` field) so the dashboard can render empty cards.

### 6. View run history

```bash
curl https://localhost:9996/app-benchmarks/history/rest-read
```

Response:
```json
{
  "runs": [
    {
      "id": "run-1743292800-a1b2c3d4",
      "testName": "rest-read",
      "timestamp": "2026-03-29T12:00:00Z",
      "durationSecs": 30.0,
      "clients": 100,
      "results": "{\"throughput\":24531.2,\"p50\":3.82,...}",
      "summary": "735.9k requests in 30s (24531.2 req/s), p50=3.82ms p95=8.14ms ...",
      "snapshots": "[{\"second\":1,\"rps\":23104.0,\"p50_ms\":3.91,...},...]"
    }
  ]
}
```

### 7. Run with custom parameters

```bash
# Custom VU count
curl -X POST https://localhost:9996/app-benchmarks/runner \
  -H "Content-Type: application/json" \
  -d '{ "test": "ws", "vus": 30000 }'

# Multi-target cluster benchmark
curl -X POST https://localhost:9996/app-benchmarks/runner \
  -H "Content-Type: application/json" \
  -d '{
    "test": "rest-read",
    "targetUrl": "https://node1:9996,https://node2:9996,https://node3:9996"
  }'
```

### 8. Clear all results

```bash
curl -X DELETE https://localhost:9996/app-benchmarks/bestresults
```

Response:
```json
{ "deleted": 42 }
```

---

## Architecture

```
Dashboard (React SPA)                  Load Generator Binaries
    |                                     |
    +-- GET /runner ----+                 +-- load-rest
    +-- POST /runner ---+                 +-- load-graphql
    +-- GET /bestresults                  +-- load-vector
    +-- GET /history/{test}               +-- load-realtime
    +-- DELETE /bestresults               +-- load-blob
          |                                     |
          v                                     v
    +--------------------------------------------------+
    |                 app-benchmarks                    |
    |                                                  |
    |  Resources:                                      |
    |  +----------------+  +--------------+            |
    |  |BenchmarkRunner |  | BestResults  |            |
    |  | (orchestrate)  |  | (aggregate)  |            |
    |  +----------------+  +--------------+            |
    |  +----------------+                              |
    |  |    History     |                              |
    |  | (per-test log) |                              |
    |  +----------------+                              |
    |                                                  |
    |  Modules:                                        |
    |  +----------------+                              |
    |  |    Metrics     |  HdrHistogram + snapshots    |
    |  +----------------+                              |
    |                                                  |
    |  Tables:                                         |
    |  TestRun, ReadBook, WriteBook, UpdateBook,       |
    |  JoinBook, JoinAuthor, GqlReadBook, GqlWriteBook,|
    |  GqlUpdateBook, GqlJoinBook, GqlJoinAuthor,      |
    |  BatchWriteBook, GqlBatchWriteBook, EmbedArticle,|
    |  SearchArticle, BlobData, WsMessage, SseMessage, |
    |  WsPublishMessage, MqttMessage                   |
    +--------------------------------------------------+
          |
          v
    Yeti (embedded RocksDB, HNSW indexes, WS/SSE/MQTT broker)
```

**Execution flow:** Dashboard POST /runner -> BenchmarkRunner spawns binary as child process -> binary seeds test data -> warmup phase (5s, metrics discarded) -> measurement phase (30s, HdrHistogram recording) -> binary writes results to TestRun table -> BenchmarkRunner detects child exit -> dashboard polls GET /runner until idle -> fetches updated bestresults.

**Phase transitions:** The binary writes its current phase to a temp status file. The runner resource reads this file on each GET poll and reports: `seeding` -> `warming` -> `running` -> `cleaning` -> `idle`.

---

## Features

### Benchmark Runner (POST /app-benchmarks/runner)

Orchestrates benchmark execution by spawning native load test binaries as child processes:

| Field | Type | Description |
|-------|------|-------------|
| `test` | String (required) | Test ID from the test catalog (e.g., "rest-read") |
| `vus` | Integer | Virtual user count (overrides test default) |
| `processes` | Integer | Process count (auto-calculated: 1 per 5,000 VUs) |
| `targetUrl` | String | Target URL(s), comma-delimited for multi-target |

**Runner state machine:**

| Phase | Description |
|-------|-------------|
| `idle` | No test running. Ready to accept new test. |
| `seeding` | Binary is populating test data into tables. |
| `warming` | Binary is sending requests but metrics are discarded (5s). |
| `running` | Active measurement. HdrHistogram recording latencies. |
| `cleaning` | Binary is cleaning up test data from tables. |

**Process management:** The runner tracks all child processes and reaps them on completion. If a benchmark exceeds its maximum allowed time (ramp + warmup + duration + 60s grace), all children are killed. Stale state is detected and cleared on the next GET poll.

**Telemetry suppression:** While any benchmark is active, responses include `x-yeti-suppress-telemetry: true` to prevent tracing overhead from affecting results. The dynamic router intercepts this header and pauses telemetry collection.

### Runner Status (GET /app-benchmarks/runner)

Returns current runner state with timing information:

| Field | Type | Description |
|-------|------|-------------|
| `status` | String | Current phase: idle, seeding, warming, running, cleaning |
| `phase` | String | Same as status (for dashboard compatibility) |
| `testName` | String | Active test ID, or null if idle |
| `warmupSecs` | Float | Seconds elapsed in warmup phase |
| `elapsedSecs` | Float | Seconds elapsed in measurement phase (capped at duration) |
| `configuredDuration` | Integer | Total measurement duration in seconds |
| `lastError` | String | Error message from last failed run, or null |

### Best Results (GET /app-benchmarks/bestresults)

Aggregates the best run for each test based on highest throughput, filtering out runs with greater than 1% error rate:

| Field | Type | Description |
|-------|------|-------------|
| `tests` | Object | Keyed by test ID. Each entry includes test definition + best metrics. |
| `categories` | Array | Category definitions for dashboard grouping. |

**Best-result selection:** For each test, scans all TestRun records, parses the JSON `results` field, and selects the run with the highest `throughput` value that has an error rate below 1%.

### Clear Results (DELETE /app-benchmarks/bestresults)

Deletes all TestRun records from the database. Returns the count of deleted records.

### Run History (GET /app-benchmarks/history/{testName})

Returns all historical runs for a specific test, sorted by timestamp descending:

| Field | Type | Description |
|-------|------|-------------|
| `runs` | Array | TestRun records with full metrics and snapshots. |

Each run includes the raw `results` JSON (throughput, latency percentiles, totals, errors), a human-readable `summary` string, and optionally a `snapshots` JSON array of per-second time-series data points.

---

## Test Catalog

### REST Workloads

| Test ID | Name | VUs | Binary | Description |
|---------|------|-----|--------|-------------|
| `rest-read` | REST Read | 100 | load-rest | GET single records by ID |
| `rest-write` | REST Write | 100 | load-rest | POST new records |
| `rest-batch-write` | REST Batch Write | 100 | load-rest | POST records in batches |
| `rest-update` | REST Update | 100 | load-rest | PUT existing records |
| `rest-join` | REST Join | 100 | load-rest | GET records with @relationship joins |

### GraphQL Workloads

| Test ID | Name | VUs | Binary | Description |
|---------|------|-----|--------|-------------|
| `graphql-read` | GraphQL Read | 100 | load-graphql | Single-record queries |
| `graphql-mutation` | GraphQL Write | 100 | load-graphql | Create mutations |
| `graphql-batch-write` | GraphQL Batch Write | 100 | load-graphql | Batch create mutations |
| `graphql-update` | GraphQL Update | 100 | load-graphql | Update mutations |
| `graphql-join` | GraphQL Join | 100 | load-graphql | Queries with nested relationships |

### Realtime Workloads

| Test ID | Name | VUs | Binary | Description |
|---------|------|-----|--------|-------------|
| `ws` | WS Fan-Out | 15,000 | load-realtime | WebSocket subscribers receiving broadcasts |
| `ws-publish` | WS Fan-In | 100 | load-realtime | WebSocket publishers sending messages |
| `sse` | SSE Fan-Out | 15,000 | load-realtime | SSE subscribers receiving broadcasts |
| `mqtt` | MQTT Fan-Out | 15,000 | load-realtime | MQTT subscribers receiving broadcasts |

### Vector Workloads

| Test ID | Name | VUs | Binary | Description |
|---------|------|-----|--------|-------------|
| `vector-embed` | Vector Embed | 10 | load-vector | Embedding generation via ONNX on write |
| `vector-search` | Vector Search | 100 | load-vector | HNSW nearest-neighbor queries |

### Blob Workloads

| Test ID | Name | VUs | Binary | Description |
|---------|------|-----|--------|-------------|
| `blob-retrieval` | 150k Blob Retrieval | 100 | load-blob | GET 150KB records |

---

## Data Model

### TestRun Table

Stores results from every benchmark run, including per-second time-series snapshots.

| Field | Type | Indexed | Description |
|-------|------|---------|-------------|
| `id` | ID! | Primary key | Unique run identifier |
| `testName` | String! | Yes | Test ID (e.g., "rest-read") |
| `timestamp` | String! | -- | ISO 8601 timestamp of run start |
| `durationSecs` | Float | -- | Actual measurement duration |
| `clients` | Int | -- | Virtual user count |
| `results` | String | -- | JSON: throughput, p50/p95/p99/p999, total, errors |
| `summary` | String | -- | Human-readable one-line summary |
| `snapshots` | String | -- | JSON array of per-second Snapshot objects |

**Snapshot schema (inside `snapshots` JSON):**

| Field | Type | Description |
|-------|------|-------------|
| `second` | Integer | Seconds elapsed since measurement start |
| `rps` | Float | Requests per second in this interval |
| `p50_ms` | Float | 50th percentile latency (ms) |
| `p95_ms` | Float | 95th percentile latency (ms) |
| `p99_ms` | Float | 99th percentile latency (ms) |
| `p999_ms` | Float | 99.9th percentile latency (ms) |
| `errors` | Integer | Error count in this interval |
| `active_vus` | Integer | Active virtual users in this interval |

### Workload Tables

Isolated tables per workload prevent cross-contamination between concurrent or sequential test runs:

| Table | Used by | Fields |
|-------|---------|--------|
| `ReadBook` | rest-read | id, title, price, authorId |
| `WriteBook` | rest-write | id, title, price, authorId |
| `UpdateBook` | rest-update | id, title, price, authorId |
| `BatchWriteBook` | rest-batch-write | id, title, price, authorId |
| `JoinBook` + `JoinAuthor` | rest-join | Book fields + @relationship to Author |
| `GqlReadBook` | graphql-read | id, title, price, authorId |
| `GqlWriteBook` | graphql-mutation | id, title, price, authorId |
| `GqlUpdateBook` | graphql-update | id, title, price, authorId |
| `GqlBatchWriteBook` | graphql-batch-write | id, title, price, authorId |
| `GqlJoinBook` + `GqlJoinAuthor` | graphql-join | Book fields + @relationship to Author |
| `EmbedArticle` | vector-embed | id, title, author, category, content, embedding (Vector) |
| `SearchArticle` | vector-search | id, title, author, category, content, embedding (Vector) |
| `BlobData` | blob-retrieval | id, title, author, category, content (150KB payload) |
| `WsMessage` | ws | id, title, content (WS + SSE enabled) |
| `SseMessage` | sse | id, title, content (WS + SSE enabled) |
| `WsPublishMessage` | ws-publish | id, title, content (WS + SSE + publish enabled) |
| `MqttMessage` | mqtt | id, title, content (MQTT enabled) |

---

## Metrics Module

The shared `modules/metrics.rs` module provides the instrumentation layer used by all load test binaries:

| Component | Description |
|-----------|-------------|
| `Metrics` | Thread-safe counters and HdrHistogram with warmup gating |
| `MetricsSummary` | Aggregated results: throughput, percentiles, totals, errors, bytes |
| `Snapshot` | Per-second interval sample: rps, percentiles, errors, active VUs |
| `SnapshotCollector` | Background tokio task that drains interval histograms every second |

**Warmup gating:** While `is_warming` is true, `record_success()` and `record_error()` are no-ops. This ensures warmup traffic does not pollute measurement histograms.

**Interval snapshots:** The `SnapshotCollector` swaps and drains a separate interval histogram every second, producing `Snapshot` objects that capture instantaneous throughput and latency. These are serialized as JSON arrays in the TestRun `snapshots` field and rendered as time-series charts in the dashboard.

**Histogram precision:** HdrHistogram with bounds 1 to 60,000,000 microseconds and 3 significant figures. Latencies are recorded in microseconds and reported in milliseconds.

---

## Configuration

### config.yaml

```yaml
name: "Yeti Benchmarks"
app_id: "app-benchmarks"
version: "0.1.0"
customer_id: "yeti"
required_roles: [yeti_admin]

static_files:
  path: web
  spa: true
  build:
    sourceDir: source
    command: npm run build

schemas:
  - schema.graphql

resources:
  - "resources/*.rs"

binaries:
  - "bin/*.rs"

modules:
  - "modules/*.rs"

auth:
  methods: [oauth]
  oauth:
    google:
      clientId: "${GOOGLE_CLIENT_ID}"
      clientSecret: "${GOOGLE_CLIENT_SECRET}"
    rules:
      - strategy: email
        pattern: "*@yetirocks.com"
        role: yeti_admin
```

### Key configuration details

| Key | Value | Description |
|-----|-------|-------------|
| `required_roles` | `[yeti_admin]` | Restricts access to users with the yeti_admin role |
| `static_files.spa` | `true` | Serves the React dashboard as a single-page application |
| `static_files.build` | `npm run build` | Builds the Vite+React dashboard from source |
| `binaries` | `bin/*.rs` | Compiles load test binaries alongside the plugin |
| `modules` | `modules/*.rs` | Shared code (metrics) available to both resources and binaries |
| `auth.methods` | `[oauth]` | OAuth-only authentication via Google |
| `auth.oauth.rules` | email pattern match | Maps `*@yetirocks.com` to `yeti_admin` role |

### Environment variables

| Variable | Description |
|----------|-------------|
| `GOOGLE_CLIENT_ID` | Google OAuth client ID |
| `GOOGLE_CLIENT_SECRET` | Google OAuth client secret |
| `YETI_BENCHMARK_TARGET` | Default target URL when not specified in POST body |

---

## Authentication

app-benchmarks uses yeti's built-in auth system with OAuth (Google) and role-based access:

- **Required role:** `yeti_admin` -- all endpoints require this role
- **OAuth rule:** Email addresses matching `*@yetirocks.com` are mapped to the `yeti_admin` role
- **Public table access:** All workload tables (ReadBook, WriteBook, etc.) declare `public: [read, create, update, delete]` so load test binaries can access them without authentication
- **TestRun table:** Also publicly accessible so binaries can write results directly

In development mode, the role requirement is bypassed for the dashboard and resource endpoints, but the OAuth configuration is still active.

---

## Project Structure

```
app-benchmarks/
├── config.yaml              # App configuration
├── schema.graphql           # 20 tables: TestRun + isolated workload tables
├── resources/
│   ├── benchmark_runner.rs  # Orchestration: spawn binaries, track state
│   ├── best_results.rs      # Aggregate best throughput per test
│   └── history.rs           # Per-test run history
├── bin/
│   ├── load_rest.rs         # REST CRUD load generator
│   ├── load_graphql.rs      # GraphQL query/mutation load generator
│   ├── load_realtime.rs     # WS/SSE/MQTT subscriber/publisher
│   ├── load_vector.rs       # Vector embed + HNSW search generator
│   └── load_blob.rs         # Large object retrieval generator
├── modules/
│   └── metrics.rs           # HdrHistogram, snapshots, warmup gating
└── source/                  # React + Vite dashboard source
    ├── src/
    │   ├── App.tsx           # Main dashboard layout
    │   ├── api.ts            # API client
    │   ├── types.ts          # TypeScript type definitions
    │   ├── main.tsx          # Entry point
    │   └── components/
    │       └── BenchmarkChart.tsx  # Time-series chart component
    ├── index.html
    ├── package.json
    └── vite.config.ts
```

---

## REST Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/app-benchmarks/runner` | Current runner state and timing |
| `POST` | `/app-benchmarks/runner` | Start a benchmark test |
| `GET` | `/app-benchmarks/bestresults` | Best results for all tests |
| `DELETE` | `/app-benchmarks/bestresults` | Delete all TestRun records |
| `GET` | `/app-benchmarks/history/{testName}` | Run history for a specific test |
| `GET` | `/app-benchmarks/TestRun?limit=N` | List TestRun records (auto-generated) |
| `GET` | `/app-benchmarks/TestRun/{id}` | Get a specific TestRun (auto-generated) |
| `GET` | `/app-benchmarks/{Table}?stream=sse` | SSE stream for realtime tables |

---

## Comparison

| | app-benchmarks | k6 / wrk / vegeta |
|---|---|---|
| **Integration** | Native Yeti plugin, auto-compiles with the platform | External tool, separate install, custom scripts |
| **Transport coverage** | REST, GraphQL, WebSocket, SSE, MQTT, Vector, Blob | Typically HTTP only, WS via extensions |
| **Results storage** | Auto-stored in Yeti tables with full history | File output, manual aggregation |
| **Dashboard** | Built-in React SPA with time-series charts | Separate visualization tool required |
| **Latency precision** | HdrHistogram with p50/p95/p99/p99.9 | Varies by tool |
| **Multi-process** | Auto-scaled by VU count, staggered TLS | Manual process coordination |
| **Multi-target** | Comma-delimited URLs, round-robin distribution | Custom scripting |
| **Telemetry isolation** | Automatic suppression during measurement | No awareness of observed system |
| **Language** | Native Rust, compiled binaries | Lua (wrk), Go (k6/vegeta), JavaScript |
| **Cluster testing** | Distribute load across multiple Yeti nodes | Single target without custom wrappers |

---

Built with [Yeti](https://yetirocks.com) | The Performance Platform for Agent-Driven Development
