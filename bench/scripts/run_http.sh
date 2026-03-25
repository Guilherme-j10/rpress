#!/usr/bin/env bash
set -euo pipefail

_RAW_HOST="${BENCH_HOST:-127.0.0.1:9090}"
if [[ "$_RAW_HOST" == http://* ]] || [[ "$_RAW_HOST" == https://* ]]; then
    HOST="$_RAW_HOST"
else
    HOST="http://$_RAW_HOST"
fi
RESULTS_DIR="${BENCH_RESULTS_DIR:-./bench/results}"
mkdir -p "$RESULTS_DIR"

# ── Configurable scenario parameters ──
WARMUP_N="${BENCH_WARMUP_N:-1000}"
WARMUP_C="${BENCH_WARMUP_C:-50}"

THROUGHPUT_N="${BENCH_THROUGHPUT_N:-50000}"
THROUGHPUT_C="${BENCH_THROUGHPUT_C:-500}"

JSON_N="${BENCH_JSON_N:-20000}"
JSON_C="${BENCH_JSON_C:-200}"

POST_N="${BENCH_POST_N:-20000}"
POST_C="${BENCH_POST_C:-200}"
POST_BODY_SIZE="${BENCH_POST_BODY_SIZE:-1024}"

LARGE_N="${BENCH_LARGE_N:-10000}"
LARGE_C="${BENCH_LARGE_C:-100}"

STATIC_N="${BENCH_STATIC_N:-10000}"
STATIC_C="${BENCH_STATIC_C:-100}"

EXTREME_N="${BENCH_EXTREME_N:-10000}"
EXTREME_C="${BENCH_EXTREME_C:-1000}"

SUSTAINED_DURATION="${BENCH_SUSTAINED_DURATION:-60s}"
SUSTAINED_C="${BENCH_SUSTAINED_C:-200}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

PASS=0
FAIL=0
TOTAL=0

check_oha() {
    if ! command -v oha &>/dev/null; then
        echo -e "${RED}[ERROR] oha not found. Install with: cargo install oha${NC}"
        exit 1
    fi
}

wait_for_server() {
    echo -e "${BLUE}Waiting for server at ${HOST}/health ...${NC}"
    for i in $(seq 1 30); do
        if curl -s -o /dev/null -w "%{http_code}" "$HOST/health" 2>/dev/null | grep -q "200"; then
            echo -e "${GREEN}Server is ready.${NC}"
            return 0
        fi
        sleep 1
    done
    echo -e "${RED}Server did not start within 30 seconds.${NC}"
    exit 1
}

run_scenario() {
    local name="$1"
    local description="$2"
    shift 2
    local args=("$@")

    TOTAL=$((TOTAL + 1))
    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}[$TOTAL] $name${NC}"
    echo -e "    $description"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

    local json_file="$RESULTS_DIR/${name}.json"

    if env -u NO_COLOR oha "${args[@]}" --output-format json > "$json_file" 2>/dev/null; then
        python3 -c "
import json, sys
try:
    d = json.load(open('$json_file'))
    s = d.get('summary', {})
    lp = d.get('latencyPercentiles', {})
    sc = d.get('statusCodeDistribution', {})

    rps = s.get('requestsPerSec', 0)
    p50 = lp.get('p50', 0) * 1000
    p90 = lp.get('p90', 0) * 1000
    p99 = lp.get('p99', 0) * 1000
    success = s.get('successRate', 0) * 100

    ok = sum(v for k,v in sc.items() if k.startswith('2'))
    total = sum(sc.values()) if sc else 0

    print(f'    Requests/sec:  {rps:,.1f}')
    print(f'    Latency p50:   {p50:.2f}ms')
    print(f'    Latency p90:   {p90:.2f}ms')
    print(f'    Latency p99:   {p99:.2f}ms')
    print(f'    Success:       {ok}/{total} ({success:.1f}%)')
    print(f'    Results:       $json_file')
except Exception as e:
    print(f'    Parse error: {e}', file=sys.stderr)
    sys.exit(1)
" 2>/dev/null
        PASS=$((PASS + 1))
    else
        echo -e "    ${RED}FAILED to run scenario${NC}"
        FAIL=$((FAIL + 1))
    fi
}

main() {
    check_oha
    wait_for_server

    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║           RPRESS HTTP LOAD TEST SUITE                      ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"

    run_scenario "01_warmup" \
        "Warmup: ${WARMUP_N} requests, ${WARMUP_C} concurrent on /health" \
        -n "$WARMUP_N" -c "$WARMUP_C" "$HOST/health"

    run_scenario "02_throughput_max" \
        "Max throughput: ${THROUGHPUT_N} requests, ${THROUGHPUT_C} concurrent on /health" \
        -n "$THROUGHPUT_N" -c "$THROUGHPUT_C" "$HOST/health"

    run_scenario "03_json_response" \
        "JSON serialization: ${JSON_N} requests, ${JSON_C} concurrent on /api/json" \
        -n "$JSON_N" -c "$JSON_C" "$HOST/api/json"

    run_scenario "04_post_echo" \
        "POST echo: ${POST_N} requests, ${POST_C} concurrent, ${POST_BODY_SIZE}B body on /api/echo" \
        -n "$POST_N" -c "$POST_C" -m POST \
        -d "$(python3 -c "print('A' * $POST_BODY_SIZE)")" \
        "$HOST/api/echo"

    run_scenario "05_large_compressed" \
        "Large body + compression: ${LARGE_N} requests, ${LARGE_C} concurrent on /api/large" \
        -n "$LARGE_N" -c "$LARGE_C" -H "Accept-Encoding: br,gzip" "$HOST/api/large"

    run_scenario "06_static_file" \
        "Static file: ${STATIC_N} requests, ${STATIC_C} concurrent on /assets/bench.css" \
        -n "$STATIC_N" -c "$STATIC_C" "$HOST/assets/bench.css"

    run_scenario "07_extreme_concurrency" \
        "Extreme concurrency: ${EXTREME_N} requests, ${EXTREME_C} concurrent on /health" \
        -n "$EXTREME_N" -c "$EXTREME_C" "$HOST/health"

    run_scenario "08_sustained" \
        "Sustained load: ${SUSTAINED_DURATION}, ${SUSTAINED_C} concurrent on /api/json" \
        -z "$SUSTAINED_DURATION" -c "$SUSTAINED_C" "$HOST/api/json"

    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║                        SUMMARY                             ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo -e "    Total scenarios: $TOTAL"
    echo -e "    ${GREEN}Passed:${NC}  $PASS"
    echo -e "    ${RED}Failed:${NC}  $FAIL"
    echo -e "    Results saved to: $RESULTS_DIR/"
    echo ""

    if [ "$FAIL" -gt 0 ]; then
        echo -e "${RED}Some scenarios failed!${NC}"
        exit 1
    else
        echo -e "${GREEN}All scenarios completed successfully.${NC}"
    fi
}

main "$@"
