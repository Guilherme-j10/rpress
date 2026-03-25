#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BENCH_DIR="$PROJECT_ROOT/bench"

export BENCH_PORT="${BENCH_PORT:-9090}"
export BENCH_HOST="${BENCH_HOST:-127.0.0.1:$BENCH_PORT}"
export BENCH_RESULTS_DIR="${BENCH_RESULTS_DIR:-$BENCH_DIR/results}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        echo -e "\n${YELLOW}Stopping bench server (PID: $SERVER_PID)...${NC}"
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
        echo -e "${GREEN}Server stopped.${NC}"
    fi
}
trap cleanup EXIT INT TERM

wait_for_server() {
    echo -e "${BLUE}Waiting for bench server on port $BENCH_PORT...${NC}"
    for i in $(seq 1 30); do
        if curl -s -o /dev/null -w "%{http_code}" "http://$BENCH_HOST/health" 2>/dev/null | grep -q "200"; then
            echo -e "${GREEN}Bench server is ready (PID: $SERVER_PID).${NC}"
            return 0
        fi
        sleep 1
    done
    echo -e "${RED}Server failed to start within 30 seconds.${NC}"
    exit 1
}

build_server() {
    echo -e "${BLUE}Building bench server (release mode)...${NC}"
    cd "$BENCH_DIR"
    cargo build --release 2>&1 | tail -1
    echo -e "${GREEN}Build complete.${NC}"
}

start_server() {
    echo -e "${BLUE}Starting bench server...${NC}"
    cd "$PROJECT_ROOT"
    BENCH_PORT="$BENCH_PORT" \
    BENCH_MAX_CONN="${BENCH_MAX_CONN:-4096}" \
    BENCH_COMPRESSION="${BENCH_COMPRESSION:-true}" \
    BENCH_RATE_LIMIT="${BENCH_RATE_LIMIT:-}" \
    BENCH_READ_TIMEOUT="${BENCH_READ_TIMEOUT:-30}" \
    BENCH_IDLE_TIMEOUT="${BENCH_IDLE_TIMEOUT:-60}" \
    BENCH_MAX_BODY_MB="${BENCH_MAX_BODY_MB:-10}" \
    "$BENCH_DIR/target/release/bench-server" &
    SERVER_PID=$!
    wait_for_server
}

main() {
    echo ""
    echo -e "${BOLD}${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${BLUE}║                                                              ║${NC}"
    echo -e "${BOLD}${BLUE}║         RPRESS LOAD TEST SUITE — FULL RUN                   ║${NC}"
    echo -e "${BOLD}${BLUE}║                                                              ║${NC}"
    echo -e "${BOLD}${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""

    mkdir -p "$BENCH_RESULTS_DIR"

    local run_what="${1:-all}"

    # Build & start server
    build_server
    start_server

    local exit_code=0

    # Phase 1: HTTP Load Tests
    if [ "$run_what" = "all" ] || [ "$run_what" = "http" ]; then
        echo ""
        echo -e "${BOLD}${BLUE}▶ Phase 1: HTTP Load Tests (oha)${NC}"
        echo ""
        if "$SCRIPT_DIR/run_http.sh"; then
            echo -e "${GREEN}HTTP load tests completed.${NC}"
        else
            echo -e "${RED}HTTP load tests had failures.${NC}"
            exit_code=1
        fi
    fi

    # Phase 2: Socket.IO Load Tests
    if [ "$run_what" = "all" ] || [ "$run_what" = "socketio" ]; then
        echo ""
        echo -e "${BOLD}${BLUE}▶ Phase 2: Socket.IO Load Tests (Artillery)${NC}"
        echo ""
        if "$SCRIPT_DIR/run_socketio.sh"; then
            echo -e "${GREEN}Socket.IO load tests completed.${NC}"
        else
            echo -e "${YELLOW}Socket.IO load tests had issues (Artillery may not be installed).${NC}"
        fi
    fi

    # Phase 3: Stress Tests
    if [ "$run_what" = "all" ] || [ "$run_what" = "stress" ]; then
        echo ""
        echo -e "${BOLD}${BLUE}▶ Phase 3: Stress Tests${NC}"
        echo ""
        if "$SCRIPT_DIR/run_stress.sh"; then
            echo -e "${GREEN}Stress tests completed.${NC}"
        else
            echo -e "${RED}Stress tests had failures.${NC}"
            exit_code=1
        fi
    fi

    # Final Summary
    echo ""
    echo -e "${BOLD}${BLUE}╔══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${BLUE}║                    FINAL SUMMARY                            ║${NC}"
    echo -e "${BOLD}${BLUE}╚══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "    Results directory: $BENCH_RESULTS_DIR/"
    echo -e "    Files generated:"
    ls -1 "$BENCH_RESULTS_DIR/" 2>/dev/null | while read -r f; do
        local size
        size=$(du -h "$BENCH_RESULTS_DIR/$f" 2>/dev/null | cut -f1)
        echo -e "      - $f ($size)"
    done
    echo ""

    if [ "$exit_code" -eq 0 ]; then
        echo -e "${GREEN}${BOLD}All load test phases completed successfully.${NC}"
    else
        echo -e "${RED}${BOLD}Some test phases had failures. Check results above.${NC}"
    fi

    exit "$exit_code"
}

main "$@"
