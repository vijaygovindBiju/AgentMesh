#!/usr/bin/env bash
# ==============================================================================
# AgentMesh — Production & Local Installation Script
# ==============================================================================
# Builds and installs AgentMesh binaries (coordinator, agent-mock, agent-agy)
# into a user-specified or standard binary directory (default: ~/.local/bin).
#
# Usage:
#   ./scripts/install.sh [OPTIONS]
#
# Options:
#   -p, --prefix <DIR>     Installation directory for binaries (default: ~/.local/bin)
#   -d, --debug            Build debug binaries instead of release (cargo build)
#   --skip-build           Skip cargo compilation (use existing build artifacts)
#   --skip-env             Do not create or verify .env configuration
#   -u, --uninstall        Remove installed AgentMesh binaries from prefix
#   -y, --yes              Non-interactive mode (accept all defaults)
#   -h, --help             Display this help message
# ==============================================================================

set -euo pipefail

# Styling
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Defaults
DEFAULT_PREFIX="${HOME}/.local/bin"
PREFIX="${PREFIX:-$DEFAULT_PREFIX}"
BUILD_PROFILE="release"
SKIP_BUILD=false
SKIP_ENV=false
UNINSTALL=false
NON_INTERACTIVE=false

# Helper functions
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
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

check_cmd() {
    command -v "$1" >/dev/null 2>&1
}

# Parse options
while [[ $# -gt 0 ]]; do
    case "$1" in
        -p|--prefix)
            PREFIX="$2"
            shift 2
            ;;
        -d|--debug)
            BUILD_PROFILE="debug"
            shift
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --skip-env)
            SKIP_ENV=true
            shift
            ;;
        -u|--uninstall)
            UNINSTALL=true
            shift
            ;;
        -y|--yes|--non-interactive)
            NON_INTERACTIVE=true
            shift
            ;;
        -h|--help)
            cat << 'EOF'
AgentMesh Installer

Installs the AgentMesh coordination suite:
  - coordinator  (TUI, planning engine, Git coordination, reliability loops)
  - agent-mock   (Mock worker agent for simulated development and testing)
  - agent-agy    (Antigravity agy CLI adapter for real AI coding agents)

Usage:
  ./scripts/install.sh [OPTIONS]

Options:
  -p, --prefix <DIR>     Installation directory (default: ~/.local/bin)
  -d, --debug            Build in debug mode instead of release
  --skip-build           Skip cargo build and install existing binaries
  --skip-env             Skip creating .env from .env.example
  -u, --uninstall        Remove installed AgentMesh binaries from prefix
  -y, --yes              Non-interactive mode
  -h, --help             Show this help message

Environment:
  PREFIX                 Override target directory (same as --prefix)
  CARGO_TARGET_DIR       Custom cargo target directory if configured
EOF
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            echo "Run './scripts/install.sh --help' for usage."
            exit 1
            ;;
    esac
done

# Resolve repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

# Uninstall flow
if [ "$UNINSTALL" = true ]; then
    info "Uninstalling AgentMesh binaries from: ${PREFIX}"
    REMOVED=0
    for bin in coordinator agent-mock agent-agy; do
        target="${PREFIX}/${bin}"
        if [ -f "$target" ] || [ -L "$target" ]; then
            rm -f "$target"
            info "Removed: ${target}"
            REMOVED=$((REMOVED + 1))
        fi
    done
    if [ "$REMOVED" -gt 0 ]; then
        success "Uninstallation complete. Removed ${REMOVED} binaries."
    else
        warn "No AgentMesh binaries were found in ${PREFIX}."
    fi
    exit 0
fi

echo -e "${CYAN}${BOLD}"
cat << 'EOF'
    ___                    __  __           _     
   / _ \ ___ ____ ___  ___/ / /  |/  /__ ___ / /    
  / ___// -_) __// _ \/ _  / / /|_/ // -_) _// _ \  
 /_/    \__/_/   \___/\_,_/ /_/  /_/ \__/__//_//_/  
EOF
echo -e "   Parallel Coding Agent Coordinator Installer${NC}"
echo -e "${BLUE}================================================================${NC}\n"

# Step 1: Detect Platform & Prerequisites
info "Step 1: Detecting platform and system prerequisites..."
OS="$(uname -s)"
ARCH="$(uname -m)"
info "Platform: ${OS} (${ARCH})"

if [ "$OS" != "Linux" ] && [ "$OS" != "Darwin" ]; then
    warn "Untested operating system: ${OS}. Linux and macOS are officially supported."
fi

MISSING=0
if check_cmd git; then
    success "Git: $(git --version)"
else
    error "Git is required. Please install git."
    MISSING=1
fi

if check_cmd cargo && check_cmd rustc; then
    success "Rust toolchain: $(rustc --version)"
else
    error "Rust toolchain (cargo/rustc) not found. Install from https://rustup.rs"
    MISSING=1
fi

if [ "$MISSING" -ne 0 ]; then
    error "Missing critical prerequisites. Aborting."
    exit 1
fi

# Step 2: Environment Configuration (.env)
if [ "$SKIP_ENV" = false ]; then
    info "Step 2: Checking environment configuration..."
    if [ ! -f .env ]; then
        if [ -f .env.example ]; then
            cp .env.example .env
            success "Created .env from .env.example template."
        fi
    else
        info "Preserving existing .env file."
    fi
fi

# Step 3: Compilation
TARGET_DIR="${CARGO_TARGET_DIR:-${REPO_ROOT}/target}"
BUILD_DIR="${TARGET_DIR}/${BUILD_PROFILE}"

if [ "$SKIP_BUILD" = false ]; then
    info "Step 3: Compiling AgentMesh binaries (${BUILD_PROFILE} mode)..."
    if [ "$BUILD_PROFILE" = "release" ]; then
        cargo build --release --workspace
    else
        cargo build --workspace
    fi
    success "Compilation complete."
else
    info "Step 3: Skipping build (--skip-build specified)."
fi

# Step 4: Installation to Prefix
info "Step 4: Installing binaries to ${PREFIX}..."
mkdir -p "${PREFIX}"

BINARIES=("coordinator" "agent-mock" "agent-agy")
for bin in "${BINARIES[@]}"; do
    src="${BUILD_DIR}/${bin}"
    dst="${PREFIX}/${bin}"
    if [ ! -f "$src" ]; then
        error "Compiled binary not found: ${src}"
        exit 1
    fi
    install -m 0755 "$src" "$dst"
    success "Installed ${bin} -> ${dst}"
done

# Step 5: Verification & PATH Advice
echo ""
info "Step 5: Verifying installed binaries..."
for bin in "${BINARIES[@]}"; do
    if [ -x "${PREFIX}/${bin}" ]; then
        echo -e "  ${GREEN}✔${NC} ${PREFIX}/${bin}"
    else
        error "Verification failed for ${PREFIX}/${bin}"
        exit 1
    fi
done

echo ""
echo -e "${BLUE}================================================================${NC}"
success "AgentMesh v0.1.0 installed successfully!"
echo -e "${BLUE}================================================================${NC}"

# Check if PREFIX is in PATH
if [[ ":$PATH:" != *":$PREFIX:"* ]]; then
    echo -e "\n${YELLOW}${BOLD}Notice:${NC} '${PREFIX}' is not currently in your PATH."
    echo -e "To run binaries directly by name, add this to your shell profile (~/.bashrc, ~/.zshrc):"
    echo -e "  ${CYAN}export PATH=\"${PREFIX}:\$PATH\"${NC}\n"
fi

echo -e "${BOLD}Installed Executables:${NC}"
echo -e "  • ${BOLD}coordinator${NC}  — Terminal UI and orchestration engine"
echo -e "  • ${BOLD}agent-mock${NC}   — Simulated test agent"
echo -e "  • ${BOLD}agent-agy${NC}    — Antigravity agy CLI adapter\n"

echo -e "${BOLD}Next Steps:${NC}"
echo -e "  1. Start backing services:   ${CYAN}docker compose up -d${NC}"
echo -e "  2. Start the coordinator:    ${CYAN}${PREFIX}/coordinator${NC} (or 'coordinator' if in PATH)"
echo -e "  3. Start an agent:           ${CYAN}${PREFIX}/agent-mock${NC} or ${CYAN}${PREFIX}/agent-agy${NC}\n"
echo -e "To uninstall anytime:        ${CYAN}./scripts/install.sh --uninstall --prefix ${PREFIX}${NC}\n"
