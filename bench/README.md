# Rpress Load Test Suite

Comprehensive load testing suite for validating Rpress under production-level traffic. Every scenario is fully configurable via environment variables.

## Prerequisites

### Required

- **Rust** (stable) -- to build the bench server
- **oha** -- HTTP load generator written in Rust
  ```bash
  cargo install oha
  ```
- **curl** -- for health checks and stress tests
- **Python 3** -- for JSON result parsing

### Optional (for Socket.IO tests)

- **Node.js** + **Artillery** with Socket.IO v3 engine
  ```bash
  npm install -g artillery artillery-engine-socketio-v3
  ```

## Quick Start

Run the full suite (builds the server, starts it, runs all tests, stops it):

```bash
./bench/scripts/run_all.sh
```

Run only specific phases:

```bash
./bench/scripts/run_all.sh http      # HTTP load tests only
./bench/scripts/run_all.sh socketio  # Socket.IO tests only
./bench/scripts/run_all.sh stress    # Stress tests only
```

## Configuration

### Server Configuration

Environment variables to customize the bench server:

| Variable | Default | Description |
|---|---|---|
| `BENCH_PORT` | `9090` | Server port |
| `BENCH_HOST` | `127.0.0.1:$BENCH_PORT` | Target host for test scripts |
| `BENCH_MAX_CONN` | `4096` | Max concurrent connections |
| `BENCH_COMPRESSION` | `true` | Enable gzip/brotli compression |
| `BENCH_RATE_LIMIT` | *(disabled)* | Max requests per minute (enables rate limiting) |
| `BENCH_READ_TIMEOUT` | `30` | Read timeout in seconds |
| `BENCH_IDLE_TIMEOUT` | `60` | Idle timeout in seconds |
| `BENCH_MAX_BODY_MB` | `10` | Max body size in MB |
| `BENCH_RESULTS_DIR` | `./bench/results` | Directory for JSON results |

### HTTP Scenario Configuration

Every HTTP scenario's request count and concurrency level can be tuned:

| Variable | Default | Scenario |
|---|---|---|
| `BENCH_WARMUP_N` | `1000` | Warmup: total requests |
| `BENCH_WARMUP_C` | `50` | Warmup: concurrent connections |
| `BENCH_THROUGHPUT_N` | `50000` | Max Throughput: total requests |
| `BENCH_THROUGHPUT_C` | `500` | Max Throughput: concurrent connections |
| `BENCH_JSON_N` | `20000` | JSON Serialization: total requests |
| `BENCH_JSON_C` | `200` | JSON Serialization: concurrent connections |
| `BENCH_POST_N` | `20000` | POST Echo: total requests |
| `BENCH_POST_C` | `200` | POST Echo: concurrent connections |
| `BENCH_POST_BODY_SIZE` | `1024` | POST Echo: body size in bytes |
| `BENCH_LARGE_N` | `10000` | Large + Compression: total requests |
| `BENCH_LARGE_C` | `100` | Large + Compression: concurrent connections |
| `BENCH_STATIC_N` | `10000` | Static File: total requests |
| `BENCH_STATIC_C` | `100` | Static File: concurrent connections |
| `BENCH_EXTREME_N` | `10000` | Extreme Concurrency: total requests |
| `BENCH_EXTREME_C` | `1000` | Extreme Concurrency: concurrent connections |
| `BENCH_SUSTAINED_DURATION` | `60s` | Sustained Load: duration (e.g. `30s`, `2m`) |
| `BENCH_SUSTAINED_C` | `200` | Sustained Load: concurrent connections |

### Stress Test Configuration

| Variable | Default | Description |
|---|---|---|
| `BENCH_STRESS_CONN_N` | `5000` | Connection limit test: total connections |
| `BENCH_STRESS_CONN_C` | `5000` | Connection limit test: concurrency |
| `BENCH_STRESS_RATE_N` | `200` | Rate limit test: total requests |
| `BENCH_STRESS_SLOWLORIS_N` | `20` | Slowloris test: slow connections to open |
| `BENCH_STRESS_BODY_MB` | `15` | Oversized body test: body size in MB |
| `BENCH_STRESS_HEALTH_N` | `10` | Post-stress health: number of checks |

## Examples

### Quick smoke test (low volume)

```bash
BENCH_PORT=8080 \
BENCH_WARMUP_N=100 BENCH_WARMUP_C=10 \
BENCH_THROUGHPUT_N=1000 BENCH_THROUGHPUT_C=50 \
BENCH_JSON_N=500 BENCH_JSON_C=20 \
BENCH_POST_N=500 BENCH_POST_C=20 \
BENCH_LARGE_N=200 BENCH_LARGE_C=10 \
BENCH_STATIC_N=200 BENCH_STATIC_C=10 \
BENCH_EXTREME_N=500 BENCH_EXTREME_C=100 \
BENCH_SUSTAINED_DURATION=10s BENCH_SUSTAINED_C=50 \
./bench/scripts/run_all.sh http
```

### Heavy production simulation

```bash
BENCH_PORT=8080 \
BENCH_MAX_CONN=8192 \
BENCH_THROUGHPUT_N=200000 BENCH_THROUGHPUT_C=2000 \
BENCH_EXTREME_N=50000 BENCH_EXTREME_C=4000 \
BENCH_SUSTAINED_DURATION=5m BENCH_SUSTAINED_C=500 \
./bench/scripts/run_all.sh http
```

### Rate limiting validation

```bash
BENCH_PORT=8080 \
BENCH_RATE_LIMIT=100 \
BENCH_STRESS_RATE_N=500 \
./bench/scripts/run_all.sh stress
```

### POST with large bodies

```bash
BENCH_POST_N=5000 \
BENCH_POST_C=50 \
BENCH_POST_BODY_SIZE=65536 \
./bench/scripts/run_all.sh http
```

## Test Scenarios

### HTTP Load Tests (`run_http.sh`)

| # | Scenario | Default Requests | Default Concurrency | Target |
|---|---|---|---|---|
| 1 | Warmup | 1,000 | 50 | `/health` |
| 2 | Max Throughput | 50,000 | 500 | `/health` |
| 3 | JSON Serialization | 20,000 | 200 | `/api/json` |
| 4 | POST Echo | 20,000 | 200 | `/api/echo` |
| 5 | Large Body + Compression | 10,000 | 100 | `/api/large` |
| 6 | Static File | 10,000 | 100 | `/assets/bench.css` |
| 7 | Extreme Concurrency | 10,000 | 1,000 | `/health` |
| 8 | Sustained | 60s duration | 200 | `/api/json` |

### Socket.IO Load Tests (`run_socketio.yml`)

| Scenario | Weight | Duration | Description |
|---|---|---|---|
| Ping-Pong | 60% | ~2s | Connect, emit 5 pings, receive pongs |
| Room Join + Message | 30% | ~3s | Join room, send 5 messages |
| Broadcast Storm | 10% | ~2s | Emit 20 broadcasts in rapid succession |

Ramp-up: 2 to 20 arrivals/sec over 20 seconds, then 15 seconds sustained at 20/sec (~350 total vusers).

> **Note:** The Artillery config uses intentionally moderate concurrency. Since Artillery runs on a single Node.js process, very high arrival rates (50+/sec) cause vuser queue buildup that inflates `session_length` metrics — this is an Artillery limitation, not a server issue. If you need heavier Socket.IO testing, run multiple Artillery instances in parallel.

### Stress Tests (`run_stress.sh`)

| Test | Default | What it validates |
|---|---|---|
| Connection Limit | 5,000 connections | Server handles burst above max_connections |
| Rate Limiting | 200 requests | 429 responses when rate limit is enabled |
| Slowloris | 20 slow connections | Server stays responsive under slow clients |
| Oversized Body | 15MB POST | Server rejects with 413/400 |
| Post-Stress Health | 10 checks | Server still healthy after all stress tests |

## Results

Results are saved as JSON in `bench/results/`:

- `01_warmup.json` through `08_sustained.json` -- oha HTTP results
- `socketio_report.json` -- Artillery report
- `socketio_output.txt` -- Artillery console output

Each oha JSON file contains:

```json
{
  "summary": { "requestsPerSec": 146512.7, "successRate": 1.0, ... },
  "latencyPercentiles": { "p50": 0.00287, "p90": 0.00602, "p99": 0.01101 },
  "statusCodeDistribution": { "200": 50000 }
}
```

## Performance Targets

Suggested thresholds for a production-ready framework:

| Metric | Target |
|---|---|
| p99 latency (`/health`) | < 15ms |
| p99 latency (`/api/json`) | < 25ms |
| Throughput (`/health`, 500c) | > 50,000 req/s |
| Error rate (HTTP) | < 0.1% |
| Socket.IO connections (500) | 100% success |
| Post-stress health | all checks pass |

## Profiling Tips

For deeper analysis, combine with:

- **flamegraph**: `cargo install flamegraph && cargo flamegraph --bin bench-server`
- **tokio-console**: Add `console-subscriber` to the bench server for async task inspection
- **perf**: `perf record -g ./target/release/bench-server && perf report`

## Running Manually

If you prefer to start the server separately:

```bash
# Terminal 1: Start server
cd /path/to/rpress
BENCH_PORT=9090 cargo run --release --manifest-path bench/Cargo.toml

# Terminal 2: Run tests
BENCH_HOST=127.0.0.1:9090 ./bench/scripts/run_http.sh
BENCH_HOST=127.0.0.1:9090 ./bench/scripts/run_socketio.sh
BENCH_HOST=127.0.0.1:9090 ./bench/scripts/run_stress.sh
```
