<p align="center">
  <img src="https://cdn.prod.website-files.com/68e09cef90d613c94c3671c0/697e805a9246c7e090054706_logo_horizontal_grey.png" alt="Yeti" width="200" />
</p>

---

# app-benchmarks

[![Yeti](https://img.shields.io/badge/Yeti-App-blue)](https://yetirocks.com)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

> **[Yeti](https://yetirocks.com)** - The Performance Platform for Agent-Driven Development.
> Schema-driven APIs, real-time streaming, and vector search. From prompt to production.

Automated performance benchmarking suite for Yeti. Measures throughput, latency percentiles, and scalability across REST, GraphQL, WebSocket, SSE, MQTT, vector search, and blob retrieval workloads.

## Features

- REST CRUD benchmarks (read, write, update, join)
- GraphQL benchmarks (queries, mutations, joins)
- Real-time benchmarks (WebSocket fan-in/fan-out, SSE, MQTT)
- Vector workloads (embedding generation, nearest-neighbor search)
- Large object handling (150KB blob retrieval)
- Progressive load ramp (10 to 200 VUs)
- Sustained throughput stability (5-minute soak tests)
- Results dashboard with historical comparison

## Project Structure

```
app-benchmarks/
├── config.yaml              # App configuration
├── schema.graphql           # Test tables (isolated per workload)
├── resources/
│   ├── benchmark_runner.rs  # Orchestration and result collection
│   ├── best_results.rs      # Historical best tracking
│   └── history.rs           # Run history
├── bin/
│   ├── load_rest.rs         # REST load generator
│   ├── load_graphql.rs      # GraphQL load generator
│   ├── load_realtime.rs     # WS/SSE/MQTT load generator
│   ├── load_vector.rs       # Vector embed/search generator
│   └── load_blob.rs         # Blob retrieval generator
├── modules/                 # Shared benchmark utilities
└── web/                     # Results dashboard
```

## Configuration

```yaml
name: "Yeti Benchmarks"
app_id: "app-benchmarks"
version: "0.1.0"

static_files:
  path: web
  route: /
  index: index.html

schemas:
  - schema.graphql

resources:
  - "resources/*.rs"

binaries:
  - "bin/*.rs"

modules:
  - "modules/*.rs"
```

## Running Benchmarks

Benchmarks run against a live Yeti instance. The runner app manages test execution and result collection:

```bash
# Start Yeti
cd ~/yeti && yeti start

# Open the benchmark dashboard
open https://localhost/app-benchmarks
```

Results are stored in the `TestRun` table and displayed in the web dashboard with historical comparisons.

## Development

```bash
# Benchmarks compile as native binaries via the Yeti plugin system.
# Changes to resources/*.rs and bin/*.rs hot-reload on save.
```

---

Built with [Yeti](https://yetirocks.com) | The Performance Platform for Agent-Driven Development
