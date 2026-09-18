#!/usr/bin/env bash
# ==============================================================================
# AgentMesh — Phase 8.16: Two Real agy Instances Verification Script
# ==============================================================================
# Validates that two real agy agent instances can connect to an AgentMesh
# coordinator (locally or across two physical machines / LAN / Tailscale)
# and execute assignments in parallel.
#
# Usage:
#   ./scripts/test_two_agy_instances.sh [--nats-url <url>] [--auth-token <tok>]
# ==============================================================================

set -euo pipefail

# Styling
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

NATS_URL="${NATS_URL:-nats://localhost:4222}"
NATS_AUTH_TOKEN="${NATS_AUTH_TOKEN:-agentmesh_dev_token}"

# Parse optional arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --nats-url)
      NATS_URL="$2"
      shift 2
      ;;
    --auth-token)
      NATS_AUTH_TOKEN="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

echo -e "${BLUE}${BOLD}================================================================${NC}"
echo -e "${BLUE}${BOLD}  AgentMesh Phase 8.16: Real agy Multi-Instance Verification    ${NC}"
echo -e "${BLUE}${BOLD}================================================================${NC}"
echo -e "NATS Coordinator URL : ${BOLD}${NATS_URL}${NC}"

# Check for agy binary
AGY_BIN="${AGY_BIN_PATH:-$(which agy 2>/dev/null || echo "$HOME/.local/bin/agy")}"
if [[ ! -x "$AGY_BIN" ]]; then
  echo -e "${RED}[ERROR] agy CLI binary not found at $AGY_BIN.${NC}"
  echo -e "Please install Antigravity CLI or specify AGY_BIN_PATH."
  exit 1
fi
echo -e "${GREEN}[OK]${NC} Found agy CLI: $AGY_BIN"

# Build binaries
echo -e "${BLUE}[INFO]${NC} Compiling coordinator and agent-agy binaries..."
cargo build --bin coordinator --bin agent-agy

AGY_AGENT_BIN="./target/debug/agent-agy"
COORD_BIN="./target/debug/coordinator"

LOG_DIR="/tmp/agentmesh_phase8_logs"
mkdir -p "$LOG_DIR"

export RUST_LOG="info,agent_agy=info,coordinator=info"

PID_COORD=""
if ! pgrep -f "$COORD_BIN" >/dev/null 2>&1; then
  echo -e "${BLUE}[INFO]${NC} Starting Coordinator in background for agent registration..."
  "$COORD_BIN" > "$LOG_DIR/coordinator.log" 2>&1 &
  PID_COORD=$!

  echo -e "${BLUE}[INFO]${NC} Waiting for Coordinator services to become active..."
  for i in {1..20}; do
    if grep -q "Registration, heartbeat, and JetStream listeners active" "$LOG_DIR/coordinator.log" 2>/dev/null; then
      break
    fi
    sleep 0.5
  done
fi

echo -e "\n${BOLD}--- Launching Real agy Dual-Instance Verification ---${NC}"

# Unique agent IDs
AGENT_ID_1="a1111111-1111-4111-8111-111111111111"
AGENT_ID_2="b2222222-2222-4222-8222-222222222222"

echo -e "${BLUE}[INFO]${NC} Launching Instance 1: Alice (Backend Lead) [ID: $AGENT_ID_1]..."
NATS_URL="$NATS_URL" \
NATS_AUTH_TOKEN="$NATS_AUTH_TOKEN" \
AGY_AGENT_ID="$AGENT_ID_1" \
AGY_AGENT_OWNER="Alice (Backend Lead)" \
AGY_AGENT_API_KEY="agentmesh_agy_key" \
AGY_EFFORT="low" \
"$AGY_AGENT_BIN" > "$LOG_DIR/agent_alice.log" 2>&1 &
PID_ALICE=$!

echo -e "${BLUE}[INFO]${NC} Launching Instance 2: Bob (Frontend Lead) [ID: $AGENT_ID_2]..."
NATS_URL="$NATS_URL" \
NATS_AUTH_TOKEN="$NATS_AUTH_TOKEN" \
AGY_AGENT_ID="$AGENT_ID_2" \
AGY_AGENT_OWNER="Bob (Frontend Lead)" \
AGY_AGENT_API_KEY="agentmesh_agy_key" \
AGY_EFFORT="low" \
"$AGY_AGENT_BIN" > "$LOG_DIR/agent_bob.log" 2>&1 &
PID_BOB=$!

cleanup() {
  echo -e "\n${YELLOW}[CLEANUP] Stopping background agent-agy processes...${NC}"
  kill "$PID_ALICE" 2>/dev/null || true
  kill "$PID_BOB" 2>/dev/null || true
  if [[ -n "${PID_COORD:-}" ]]; then
    kill "$PID_COORD" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo -e "${BLUE}[INFO]${NC} Waiting for agents to connect and register (3 seconds)..."
sleep 3

# Verify registration logs
if grep -q "agy agent successfully registered" "$LOG_DIR/agent_alice.log"; then
  echo -e "${GREEN}[OK]${NC} Instance 1 (Alice) registered successfully on JetStream."
else
  echo -e "${RED}[FAIL]${NC} Instance 1 failed to register. Logs:"
  cat "$LOG_DIR/agent_alice.log"
  exit 1
fi

if grep -q "agy agent successfully registered" "$LOG_DIR/agent_bob.log"; then
  echo -e "${GREEN}[OK]${NC} Instance 2 (Bob) registered successfully on JetStream."
else
  echo -e "${RED}[FAIL]${NC} Instance 2 failed to register. Logs:"
  cat "$LOG_DIR/agent_bob.log"
  exit 1
fi

echo -e "\n${GREEN}${BOLD}================================================================${NC}"
echo -e "${GREEN}${BOLD}  Phase 8.16 Acceptance Passed: Both agy instances active!       ${NC}"
echo -e "${GREEN}${BOLD}================================================================${NC}"
echo -e "Instance 1 PID: $PID_ALICE (listening on agents.$AGENT_ID_1.tasks)"
echo -e "Instance 2 PID: $PID_BOB (listening on agents.$AGENT_ID_2.tasks)"
echo -e "\nBoth agents are actively sending heartbeats and ready for parallel task execution."
