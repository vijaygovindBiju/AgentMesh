#!/usr/bin/env bash
# ==============================================================================
# AgentMesh — Automated Installation & Setup Script
# ==============================================================================
# This script verifies system dependencies, sets up the local environment,
# launches backing infrastructure (PostgreSQL 16 + NATS 2.10 JetStream),
# compiles workspace binaries, and verifies the installation.
#
# Usage:
#   ./install.sh [options]
#
# Options:
#   -r, --release          Build optimized release binaries (default: debug build)
#   --skip-docker          Skip starting Docker Compose services
#   --skip-build           Skip cargo compilation
#   -y, --yes              Non-interactive mode (accept all defaults)
#   -h, --help             Show this help message
# ==============================================================================

set -euo pipefail

# ─── Colors and Styling ───────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# ─── Script Flags ─────────────────────────────────────────────────────────────
RELEASE_MODE=false
SKIP_DOCKER=false
SKIP_BUILD=false
NON_INTERACTIVE=false
INSTALL_BIN=false
PREFIX="${PREFIX:-$HOME/.local/bin}"

# ─── Parse Command-Line Arguments ─────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        -r|--release)
            RELEASE_MODE=true
            shift
            ;;
        -p|--prefix)
            INSTALL_BIN=true
            PREFIX="$2"
            shift 2
            ;;
        --install)
            INSTALL_BIN=true
            shift
            ;;
        --skip-docker)
            SKIP_DOCKER=true
            shift
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        -y|--yes|--non-interactive)
            NON_INTERACTIVE=true
            shift
            ;;
        -h|--help)
            cat << 'EOF'
AgentMesh Installation & Setup Script

Usage:
  ./install.sh [options]

Options:
  -r, --release          Build optimized release binaries (cargo build --release)
  -p, --prefix <DIR>     Install compiled binaries to target directory (default: ~/.local/bin)
  --install              Install binaries to ~/.local/bin after building
  --skip-docker          Do not start Docker Compose containers
  --skip-build           Skip cargo workspace build
  -y, --yes              Non-interactive mode (accept all prompts)
  -h, --help             Display this help message and exit

Examples:
  ./install.sh                 # Standard dev setup (starts Docker & builds debug binaries)
  ./install.sh --release       # Production setup (builds optimized release binaries)
  ./install.sh --release -p ~/.local/bin  # Build and install binaries to ~/.local/bin
  ./install.sh --skip-docker   # Build binaries only, assume native DB/NATS are running
EOF
            exit 0
            ;;
        *)
            echo -e "${RED}Error: Unknown option '$1'${NC}"
            echo "Run './install.sh --help' for usage instructions."
            exit 1
            ;;
    esac
done

# ─── Banner ───────────────────────────────────────────────────────────────────
echo -e "${CYAN}${BOLD}"
cat << 'EOF'
    ___                    __  __           _     
   / _ \ ___ ____ ___  ___/ / /  |/  /__ ___ / /    
  / ___// -_) __// _ \/ _  / / /|_/ // -_) _// _ \  
 /_/    \__/_/   \___/\_,_/ /_/  /_/ \__/__//_//_/  
EOF
echo -e "   Parallel Coding Agent Coordinator Setup${NC}"
echo -e "${BLUE}================================================================${NC}\n"

# Determine script root directory
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# ─── Helper Functions ─────────────────────────────────────────────────────────
info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_cmd() {
    command -v "$1" >/dev/null 2>&1
}

# ─── Step 1: System Prerequisites Verification ────────────────────────────────
info "Step 1: Checking system requirements..."

MISSING_PREREQS=0

# 1.1 Git
if check_cmd git; then
    success "Git is installed: $(git --version)"
else
    error "Git is not installed. Please install git before continuing."
    MISSING_PREREQS=1
fi

# 1.2 Curl
if check_cmd curl; then
    success "Curl is installed."
else
    error "Curl is not installed. Please install curl before continuing."
    MISSING_PREREQS=1
fi

# 1.3 Rust and Cargo
if check_cmd cargo && check_cmd rustc; then
    RUST_VER="$(rustc --version)"
    success "Rust toolchain found: $RUST_VER"
else
    warn "Rust toolchain (cargo/rustc) was not found."
    if [ "$NON_INTERACTIVE" = false ]; then
        read -rp "Would you like to install Rust via rustup now? [y/N]: " INSTALL_RUST
        if [[ "$INSTALL_RUST" =~ ^[Yy]$ ]]; then
            info "Installing Rust toolchain via rustup..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            # shellcheck source=/dev/null
            source "$HOME/.cargo/env"
            success "Rust installed successfully: $(rustc --version)"
        else
            error "Rust is required to build AgentMesh. Install it via https://rustup.rs"
            MISSING_PREREQS=1
        fi
    else
        error "Rust is required. Please install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        MISSING_PREREQS=1
    fi
fi

# 1.4 Docker and Docker Compose
DOCKER_AVAILABLE=true
if check_cmd docker; then
    success "Docker is installed: $(docker --version)"
    
    # Check Docker Compose (plugin or standalone)
    if docker compose version >/dev/null 2>&1; then
        success "Docker Compose plugin found: $(docker compose version)"
    elif check_cmd docker-compose; then
        success "Docker Compose standalone found: $(docker-compose --version)"
    else
        warn "Docker Compose not detected. Docker services cannot be started automatically."
        DOCKER_AVAILABLE=false
    fi

    # Check if Docker daemon is running
    if ! docker info >/dev/null 2>&1; then
        warn "Docker daemon is not running. Please start Docker (e.g., 'sudo systemctl start docker' or launch Docker Desktop)."
        DOCKER_AVAILABLE=false
    fi
else
    warn "Docker is not installed. PostgreSQL and NATS services will not be started automatically."
    DOCKER_AVAILABLE=false
fi

if [ "$MISSING_PREREQS" -ne 0 ]; then
    error "Missing critical prerequisites. Please resolve the errors above and rerun ./install.sh"
    exit 1
fi

echo ""

# ─── Step 2: Environment Configuration (.env) ─────────────────────────────────
info "Step 2: Configuring environment variables..."

if [ ! -f .env ]; then
    if [ -f .env.example ]; then
        cp .env.example .env
        success "Created .env from .env.example template."
    else
        warn ".env.example not found; creating minimal .env file."
        cat << 'EOF' > .env
DATABASE_URL=postgres://agentmesh:agentmesh_dev@localhost:5432/agentmesh
POSTGRES_PASSWORD=agentmesh_dev
NATS_URL=nats://localhost:4222
NATS_AUTH_TOKEN=agentmesh_dev_token
AI_PROVIDER=mock
AI_MODEL=claude-3-5-sonnet-20241022
RUST_LOG=agentmesh=debug,coordinator=debug,info
MOCK_TASK_DELAY_MS=1000
MOCK_AGENT_OWNER=MockDev
MOCK_AGENT_API_KEY=agentmesh_mock_key
EOF
        success "Generated minimal .env configuration."
    fi
else
    info "Existing .env file detected; preserving your current configuration."
fi

echo ""

# ─── Step 3: Infrastructure Services (PostgreSQL & NATS) ──────────────────────
if [ "$SKIP_DOCKER" = true ]; then
    info "Step 3: Skipping Docker infrastructure startup (--skip-docker specified)."
elif [ "$DOCKER_AVAILABLE" = true ]; then
    info "Step 3: Starting backing services (PostgreSQL 16 + NATS 2.10 JetStream)..."
    
    # Launch containers
    docker compose up -d
    
    info "Waiting for container health checks (up to 15 seconds)..."
    HEALTHY=false
    for i in {1..15}; do
        PS_OUTPUT="$(docker compose ps)"
        if echo "$PS_OUTPUT" | grep -q "(healthy)"; then
            HEALTHY=true
            break
        fi
        sleep 1
    done

    if [ "$HEALTHY" = true ]; then
        success "All infrastructure containers are healthy and running:"
        docker compose ps
    else
        warn "Containers started, but healthcheck is still pending. Status:"
        docker compose ps
    fi
else
    warn "Step 3: Docker is not currently available or running."
    warn "Coordinator will fall back to Standalone Demo Mode unless native PostgreSQL & NATS are running."
fi

echo ""

# ─── Step 4: Cargo Workspace Compilation ──────────────────────────────────────
if [ "$SKIP_BUILD" = true ]; then
    info "Step 4: Skipping cargo build (--skip-build specified)."
else
    if [ "$RELEASE_MODE" = true ]; then
        info "Step 4: Compiling AgentMesh workspace in RELEASE mode (cargo build --release --workspace)..."
        cargo build --release --workspace
        success "Release compilation complete!"
        BUILD_DIR="./target/release"
    else
        info "Step 4: Compiling AgentMesh workspace in DEBUG mode (cargo build --workspace)..."
        cargo build --workspace
        success "Debug compilation complete!"
        BUILD_DIR="./target/debug"
    fi
    COORD_BIN="${BUILD_DIR}/coordinator"
    MOCK_BIN="${BUILD_DIR}/agent-mock"
    AGY_BIN="${BUILD_DIR}/agent-agy"

    if [ "$INSTALL_BIN" = true ]; then
        info "Installing binaries to ${PREFIX}..."
        mkdir -p "${PREFIX}"
        install -m 0755 "${COORD_BIN}" "${PREFIX}/coordinator"
        install -m 0755 "${MOCK_BIN}" "${PREFIX}/agent-mock"
        install -m 0755 "${AGY_BIN}" "${PREFIX}/agent-agy"
        success "Installed coordinator, agent-mock, and agent-agy to ${PREFIX}"
    fi
fi

echo ""

# ─── Step 5: Verification & Summary ───────────────────────────────────────────
info "Step 5: Setup Summary & Next Steps"
echo -e "${BLUE}================================================================${NC}"
success "AgentMesh installation and configuration completed successfully!"
echo -e "${BLUE}================================================================${NC}"

echo -e "\n${BOLD}How to Run AgentMesh:${NC}\n"

if [ "$INSTALL_BIN" = true ]; then
    echo -e "  ${CYAN}1. Launch the Coordinator TUI:${NC}"
    echo -e "     ${BOLD}${PREFIX}/coordinator${NC}   (or: ${BOLD}coordinator${NC} if ${PREFIX} is in PATH)\n"
    echo -e "  ${CYAN}2. Launch a Mock Worker:${NC}"
    echo -e "     ${BOLD}MOCK_AGENT_OWNER=\"Alice\" ${PREFIX}/agent-mock${NC}\n"
    echo -e "  ${CYAN}3. Launch an AGY Worker (requires Antigravity CLI installed separately):${NC}"
    echo -e "     ${BOLD}AGY_AGENT_OWNER=\"Bob\" ${PREFIX}/agent-agy${NC}\n"
elif [ "$RELEASE_MODE" = true ]; then
    echo -e "  ${CYAN}1. Launch the Coordinator TUI:${NC}"
    echo -e "     ${BOLD}./target/release/coordinator${NC}   (or: ${BOLD}cargo run --release --bin coordinator${NC})\n"
    echo -e "  ${CYAN}2. Launch a Mock Worker:${NC}"
    echo -e "     ${BOLD}MOCK_AGENT_OWNER=\"Alice\" ./target/release/agent-mock${NC}\n"
    echo -e "  ${CYAN}3. Launch an AGY Worker (requires Antigravity CLI installed separately):${NC}"
    echo -e "     ${BOLD}AGY_AGENT_OWNER=\"Bob\" ./target/release/agent-agy${NC}\n"
else
    echo -e "  ${CYAN}1. Launch the Coordinator TUI:${NC}"
    echo -e "     ${BOLD}cargo run --bin coordinator${NC}\n"
    echo -e "  ${CYAN}2. Launch a Mock Worker:${NC}"
    echo -e "     ${BOLD}MOCK_AGENT_OWNER=\"Alice\" cargo run --bin agent-mock${NC}\n"
    echo -e "  ${CYAN}3. Launch an AGY Worker (requires Antigravity CLI installed separately):${NC}"
    echo -e "     ${BOLD}AGY_AGENT_OWNER=\"Bob\" cargo run --bin agent-agy${NC}\n"
fi

echo -e "  ${CYAN}Backing Services Status:${NC}"
echo -e "     • NATS HTTP Monitoring:  ${BOLD}http://localhost:8222${NC}"
echo -e "     • PostgreSQL Port:       ${BOLD}localhost:5432${NC}"
echo -e "     • Docker Status:         ${BOLD}docker compose ps${NC}\n"

echo -e "  ${CYAN}Run Automated Test Suite:${NC}"
echo -e "     ${BOLD}cargo test --workspace --no-fail-fast${NC}\n"

echo -e "${GREEN}Ready to coordinate! Press Tab in the TUI to cycle screens.${NC}\n"
