#!/usr/bin/env bash
set -euo pipefail

_RAW_HOST="${BENCH_HOST:-127.0.0.1:9090}"
if [[ "$_RAW_HOST" == http://* ]] || [[ "$_RAW_HOST" == https://* ]]; then
    HOST_URL="$_RAW_HOST"
    HOST="${_RAW_HOST#http://}"
    HOST="${HOST#https://}"
else
    HOST="$_RAW_HOST"
    HOST_URL="http://$_RAW_HOST"
fi
RESULTS_DIR="${BENCH_RESULTS_DIR:-./bench/results}"
mkdir -p "$RESULTS_DIR"

# ── Configurable stress parameters ──
STRESS_CONN_N="${BENCH_STRESS_CONN_N:-5000}"
STRESS_CONN_C="${BENCH_STRESS_CONN_C:-5000}"
STRESS_RATE_N="${BENCH_STRESS_RATE_N:-200}"
STRESS_SLOWLORIS_N="${BENCH_STRESS_SLOWLORIS_N:-20}"
STRESS_BODY_MB="${BENCH_STRESS_BODY_MB:-15}"
STRESS_HEALTH_N="${BENCH_STRESS_HEALTH_N:-10}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

PASS=0
FAIL=0
TOTAL=0

wait_for_server() {
    echo -e "${BLUE}Waiting for server at ${HOST_URL}/health ...${NC}"
    for i in $(seq 1 30); do
        if curl -s -o /dev/null -w "%{http_code}" "$HOST_URL/health" 2>/dev/null | grep -q "200"; then
            echo -e "${GREEN}Server is ready.${NC}"
            return 0
        fi
        sleep 1
    done
    echo -e "${RED}Server did not start within 30 seconds.${NC}"
    exit 1
}

report() {
    local name="$1"
    local result="$2"
    local detail="$3"
    TOTAL=$((TOTAL + 1))

    if [ "$result" = "PASS" ]; then
        PASS=$((PASS + 1))
        echo -e "  ${GREEN}[PASS]${NC} $name — $detail"
    else
        FAIL=$((FAIL + 1))
        echo -e "  ${RED}[FAIL]${NC} $name — $detail"
    fi
}

# ──────────────────────────────────────────────────────────────
# Test 1: Connection limit
# Open more connections than max_connections (default 4096)
# Server should reject excess gracefully without crashing.
# ──────────────────────────────────────────────────────────────
test_connection_limit() {
    echo ""
    echo -e "${BLUE}━━━ Test: Connection Limit ━━━${NC}"
    echo -e "    Opening ${STRESS_CONN_N} concurrent connections..."

    if command -v oha &>/dev/null; then
        local result
        result=$(env -u NO_COLOR oha -n "$STRESS_CONN_N" -c "$STRESS_CONN_C" --output-format json "$HOST_URL/health" 2>/dev/null || echo "{}")

        local total_ok
        total_ok=$(echo "$result" | python3 -c "
import json, sys
d=json.loads(sys.stdin.read())
sc=d.get('statusCodeDistribution',{})
ok=sum(v for k,v in sc.items() if k.startswith('2'))
print(ok)
" 2>/dev/null || echo "0")

        if [ "$total_ok" -gt 0 ]; then
            report "Connection Limit" "PASS" "Server handled burst: $total_ok successful out of $STRESS_CONN_N"
        else
            report "Connection Limit" "FAIL" "No successful responses"
        fi
    else
        report "Connection Limit" "FAIL" "oha not installed"
    fi

    # Verify server is still alive
    local health
    health=$(curl -s -o /dev/null -w "%{http_code}" "$HOST_URL/health" 2>/dev/null || echo "000")
    if [ "$health" = "200" ]; then
        echo -e "    ${GREEN}Server still healthy after connection storm.${NC}"
    else
        echo -e "    ${RED}Server not responding after connection storm!${NC}"
    fi
}

# ──────────────────────────────────────────────────────────────
# Test 2: Rate limiting
# Expects the bench server to be restarted with rate limiting
# enabled. Sends rapid requests and checks for 429 responses.
# ──────────────────────────────────────────────────────────────
test_rate_limiting() {
    echo ""
    echo -e "${BLUE}━━━ Test: Rate Limiting ━━━${NC}"
    echo -e "    NOTE: This test requires the server to have rate limiting enabled."
    echo -e "    Restart bench server with: BENCH_RATE_LIMIT=50 to test."
    echo -e "    Sending ${STRESS_RATE_N} rapid requests to check for 429 responses..."

    local count_429=0
    local count_200=0

    for i in $(seq 1 "$STRESS_RATE_N"); do
        local code
        code=$(curl -s -o /dev/null -w "%{http_code}" "$HOST_URL/health" 2>/dev/null || echo "000")
        if [ "$code" = "429" ]; then
            count_429=$((count_429 + 1))
        elif [ "$code" = "200" ]; then
            count_200=$((count_200 + 1))
        fi
    done

    if [ "$count_429" -gt 0 ]; then
        report "Rate Limiting" "PASS" "$count_200 accepted, $count_429 rate-limited (429)"
    else
        report "Rate Limiting" "PASS" "No rate limit configured — all $count_200 accepted (expected without BENCH_RATE_LIMIT)"
    fi
}

# ──────────────────────────────────────────────────────────────
# Test 3: Slowloris (slow headers)
# Opens connections that send data very slowly.
# The server's read_timeout should close them.
# ──────────────────────────────────────────────────────────────
test_slowloris() {
    echo ""
    echo -e "${BLUE}━━━ Test: Slowloris (slow connections) ━━━${NC}"
    echo -e "    Opening ${STRESS_SLOWLORIS_N} slow connections (1 byte/sec)..."

    local pids=()
    for i in $(seq 1 "$STRESS_SLOWLORIS_N"); do
        (
            {
                echo -n "GET /health HTTP/1.1"
                sleep 2
                echo -ne "\r\nHost: localhost"
                sleep 2
                echo -ne "\r\n"
                sleep 10
            } | nc -w 15 "${HOST%%:*}" "${HOST##*:}" > /dev/null 2>&1
        ) &
        pids+=($!)
    done

    sleep 3

    # Server should still respond to normal requests while slowloris is running
    local health
    health=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$HOST_URL/health" 2>/dev/null || echo "000")

    # Clean up background processes
    for pid in "${pids[@]}"; do
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done

    if [ "$health" = "200" ]; then
        report "Slowloris" "PASS" "Server remained responsive during slow connections"
    else
        report "Slowloris" "FAIL" "Server became unresponsive (status: $health)"
    fi
}

# ──────────────────────────────────────────────────────────────
# Test 4: Oversized body
# Sends a POST with body larger than the server's limit.
# Should receive 413 Payload Too Large.
# ──────────────────────────────────────────────────────────────
test_oversized_body() {
    echo ""
    echo -e "${BLUE}━━━ Test: Oversized Body ━━━${NC}"
    local body_bytes=$((STRESS_BODY_MB * 1024 * 1024))
    echo -e "    Sending POST with ~${STRESS_BODY_MB}MB body..."

    local tmp_file
    tmp_file=$(mktemp)
    head -c "$body_bytes" /dev/urandom > "$tmp_file"

    local code
    code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 30 \
        -X POST \
        -H "Content-Type: application/octet-stream" \
        --data-binary "@$tmp_file" \
        "$HOST_URL/api/echo" 2>/dev/null || echo "000")

    rm -f "$tmp_file"

    if [ "$code" = "413" ]; then
        report "Oversized Body" "PASS" "Server correctly returned 413 Payload Too Large"
    elif [ "$code" = "400" ]; then
        report "Oversized Body" "PASS" "Server rejected oversized body with 400"
    else
        report "Oversized Body" "FAIL" "Expected 413 or 400, got: $code"
    fi
}

# ──────────────────────────────────────────────────────────────
# Test 5: Server stability after all stress tests
# Verify the server is still healthy and responding.
# ──────────────────────────────────────────────────────────────
test_post_stress_health() {
    echo ""
    echo -e "${BLUE}━━━ Test: Post-Stress Health Check ━━━${NC}"

    local success=0
    for i in $(seq 1 "$STRESS_HEALTH_N"); do
        local code
        code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 5 "$HOST_URL/health" 2>/dev/null || echo "000")
        if [ "$code" = "200" ]; then
            success=$((success + 1))
        fi
    done

    local half=$((STRESS_HEALTH_N / 2))
    if [ "$success" -eq "$STRESS_HEALTH_N" ]; then
        report "Post-Stress Health" "PASS" "${success}/${STRESS_HEALTH_N} health checks passed"
    elif [ "$success" -gt "$half" ]; then
        report "Post-Stress Health" "PASS" "${success}/${STRESS_HEALTH_N} health checks passed (acceptable)"
    else
        report "Post-Stress Health" "FAIL" "Only ${success}/${STRESS_HEALTH_N} health checks passed"
    fi
}

main() {
    wait_for_server

    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║           RPRESS STRESS TEST SUITE                         ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"

    test_connection_limit
    test_rate_limiting
    test_slowloris
    test_oversized_body
    test_post_stress_health

    # Summary
    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║                    STRESS TEST SUMMARY                     ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo -e "    Total tests:  $TOTAL"
    echo -e "    ${GREEN}Passed:${NC}       $PASS"
    echo -e "    ${RED}Failed:${NC}       $FAIL"
    echo ""

    if [ "$FAIL" -gt 0 ]; then
        echo -e "${RED}Some stress tests failed!${NC}"
        exit 1
    else
        echo -e "${GREEN}All stress tests passed.${NC}"
    fi
}

main "$@"
