#!/usr/bin/env bash
set -euo pipefail

_RAW_HOST="${BENCH_HOST:-127.0.0.1:9090}"
# Ensure HOST always has http:// prefix
if [[ "$_RAW_HOST" == http://* ]] || [[ "$_RAW_HOST" == https://* ]]; then
    HOST="$_RAW_HOST"
else
    HOST="http://$_RAW_HOST"
fi
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="${BENCH_RESULTS_DIR:-./bench/results}"
mkdir -p "$RESULTS_DIR"

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

check_artillery() {
    if ! command -v artillery &>/dev/null; then
        echo -e "${RED}[ERROR] Artillery not found. Install with:${NC}"
        echo "  npm install -g artillery artillery-engine-socketio-v3"
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

main() {
    check_artillery
    wait_for_server

    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║         RPRESS SOCKET.IO LOAD TEST SUITE                   ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""

    local report_file="$RESULTS_DIR/socketio_report.json"

    echo -e "${BLUE}Running Artillery Socket.IO scenarios...${NC}"
    echo -e "    Config: $SCRIPT_DIR/run_socketio.yml"
    echo -e "    Target: $HOST"
    echo ""

    artillery run \
        --target "$HOST" \
        --output "$report_file" \
        "$SCRIPT_DIR/run_socketio.yml" 2>&1 | tee "$RESULTS_DIR/socketio_output.txt"

    echo ""
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

    if [ -f "$report_file" ]; then
        echo -e "${GREEN}Results saved to: $report_file${NC}"

        python3 -c "
import json, sys
try:
    d = json.load(open('$report_file'))
    agg = d.get('aggregate', {})
    counters = agg.get('counters', {})
    created = counters.get('socketio.connect', counters.get('vusers.created', 0))
    completed = counters.get('vusers.completed', 0)
    failed = counters.get('vusers.failed', 0)
    latency = agg.get('latency', agg.get('summaries', {}))

    print(f'    Connections created:    {created}')
    print(f'    Scenarios completed:   {completed}')
    print(f'    Scenarios failed:      {failed}')

    if isinstance(latency, dict) and latency:
        p50 = latency.get('median', latency.get('p50', 'N/A'))
        p95 = latency.get('p95', 'N/A')
        p99 = latency.get('p99', 'N/A')
        print(f'    Latency p50:           {p50}ms')
        print(f'    Latency p95:           {p95}ms')
        print(f'    Latency p99:           {p99}ms')
except Exception as e:
    print(f'    Could not parse report: {e}', file=sys.stderr)
" 2>/dev/null || true

    else
        echo -e "${RED}No report file generated.${NC}"
    fi

    echo ""
    echo -e "${GREEN}Socket.IO load test complete.${NC}"
}

main "$@"
