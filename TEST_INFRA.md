# Test Infrastructure & Testing Methodology

## 1. Overview & Dual-Track Testing Methodology

AgentMesh adopts a **dual-track testing methodology** to achieve high velocity, determinism, and end-to-end operational confidence:

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                 AgentMesh Dual-Track Test Strategy                                │
├─────────────────────────────────────────────────┬────────────────────────────────────────────────┤
│ Track 1: Fast Headless & Deterministic TUI      │ Track 2: Stateful Integration & E2E            │
├─────────────────────────────────────────────────┼────────────────────────────────────────────────┤
│ • Ratatui `TestBackend` in-memory rendering     │ • PostgreSQL 16 persistent database schema     │
│ • Pure state machine transitions (AppState)     │ • NATS 2.10 + JetStream message delivery       │
│ • Keybinding action emission (TuiAction)        │ • Distributed agent adapters (Agy, Mock)       │
│ • Exact cell coordinate buffer assertion        │ • Concurrent worktree management & Git hooks   │
│ • Execution time: < 100ms                       │ • Execution time: 5s - 60s                     │
│ • Flakiness rate: 0.0%                          │ • Managed via Docker Compose                   │
│ • Zero external daemons required                │ • Gated behind environment / DB availability   │
└─────────────────────────────────────────────────┴────────────────────────────────────────────────┘
```

### Track 1: Fast Headless Unit & TUI Deterministic Testing
The operator TUI (`crates/coordinator/src/tui/`) is strictly decoupled from storage and messaging side-effects. It functions as a pure unidirectional state machine:
- **Input**: User keypresses (`crossterm::event::KeyEvent`) and incoming updates (`TuiUpdateEvent`).
- **Processing**: Local in-memory mutations in `AppState`.
- **Output**: Pure enum action intents (`TuiAction`) and frame rendering (`TerminalApp::draw`).

By utilizing `ratatui::backend::TestBackend`, tests run completely headless without needing an interactive terminal (TTY) or display server. Every character, color, and modifier rendered into the virtual cell matrix is deterministically inspected.

### Track 2: Stateful Integration & E2E Testing
Validates real database transactions, atomic row locks (`FOR UPDATE SKIP LOCKED`), JetStream stream consumption, and process execution:
- Database schema migrations and RLS policies (`crates/coordinator/tests/phase2_integration.rs`, `phase6_dashboard_overlap.rs`).
- LLM planning and proposal decomposition (`phase3_planning.rs`, `phase10_intelligent_planning.rs`).
- Stale task sweepers and delivery outbox reconcilers (`phase14_reliability.rs`).
- Real `agy` subprocess lifecycle and protocol framing (`phase8_agy_integration.rs`).

---

## 2. 4-Tier Test Taxonomy

Testing for Phase 20 Operator TUI & CLI Preservation is structured across 4 distinct tiers:

| Tier | Focus | Minimum Target | Key Verification Areas |
|---|---|---|---|
| **Tier 1** | Feature Coverage | >= 5 tests per feature | Screen navigation, Plan review workflow, Dashboard rendering, Diagnostics KPI & event log, Project input, CLI argument parsing |
| **Tier 2** | Boundary & Corner Cases | >= 5 tests per feature | Scrolling window with 15+ tasks, Responsive resizing (`80x24`, `100x30`, `160x50`, `70x20`), Description buffer cursor & boundaries, Event log bounds (0 vs 100+), Fleet status diversity |
| **Tier 3** | Cross-Feature Combinations & State Transitions | Comprehensive | Full task lifecycle event stream projection (`TaskStarted` -> `ProgressUpdate` -> `Blocked` -> `Completed` -> `Failed`), Overlap warning unblocking, DAG dependency rendering, Help modal overlay, Agent capability inspector |
| **Tier 4** | Real-World Operator Workflows | Multi-step end-to-end | Operator review flow (proposal -> review -> edit -> approve), Operator fleet monitoring flow (multi-agent concurrent updates, diagnostics inspection) |

---

## 3. Test Harness Architecture & Inspection Helpers

### 3.1 Deterministic Buffer Extraction
Rather than asserting on fragile visual layout spacing, tests convert the `TestBackend` cell buffer into a structured multi-line string:

```rust
fn buffer_to_string(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut s = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            s.push_str(buffer.get(x, y).symbol());
        }
        s.push('\n');
    }
    s
}

fn assert_buffer_contains(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, expected: &str) {
    let content = buffer_to_string(terminal);
    assert!(
        content.contains(expected),
        "Buffer did not contain '{expected}'. Rendered buffer:\n{content}"
    );
}
```

### 3.2 Synthetic Key Press Generation
Tests generate exact Crossterm `KeyEvent` structures:

```rust
fn press_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press_key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}
```

### 3.3 State Seeding Helpers
- `AppState::new()`: Empty initial state on Screen 1 (`ProjectInput`).
- `AppState::new().with_mock_data()`: Populated state on Screen 2 (`PlanReview`) with 3 tasks, 2 agents, 1 critical overlap, system metrics, and coordinator events.

---

## 4. Test Suite Inventory

| Test Binary | Scope | Execution Command | Dependencies |
|---|---|---|---|
| `crates/coordinator/tests/phase20_tui_verification.rs` | Comprehensive Phase 20 Ratatui `TestBackend` verification (Tier 1–4) | `cargo test -p coordinator --test phase20_tui_verification` | None (In-memory) |
| `crates/coordinator/tests/phase4_tui.rs` | Baseline TUI keybindings and headless draw | `cargo test -p coordinator --test phase4_tui` | None (In-memory) |
| `crates/coordinator/tests/phase6_dashboard_overlap.rs` | Resource overlap detection and dashboard warnings | `cargo test -p coordinator --test phase6_dashboard_overlap` | Postgres (Optional) |
| `crates/coordinator/src/lib.rs` unit tests | Domain models, state machines, repositories | `cargo test -p coordinator --lib` | None / Postgres |

---

## 5. Execution Commands & Verification Guide

### Fast Verification (Headless / Local / CI)
```bash
# Run comprehensive Phase 20 TUI verification suite
cargo test -p coordinator --test phase20_tui_verification

# Run all coordinator TUI tests
cargo test -p coordinator --test phase4_tui --test phase20_tui_verification

# Run full coordinator unit tests
cargo test -p coordinator --lib

# Verify compilation and lint status across workspace
cargo check --workspace
```

### Full Integration Run (Requires Docker Compose)
```bash
# Start Postgres and NATS
docker compose up -d

# Run all workspace integration tests
cargo test --workspace -- --test-threads=1
```

---

## 6. Flakiness Mitigation & Determinism Guarantees

1. **No Real TTY Requirement**: Tests do not attach to stdout or use Crossterm raw mode; `TestBackend` renders directly to a RAM buffer.
2. **No Sleep / Async Delay**: Tests interact synchronously with `AppState` and immediately assert outputs.
3. **Exact Buffer Dimensioning**: Tests specify explicit terminal boundaries (`TestBackend::new(width, height)`) preventing host terminal window variance from affecting results.
4. **Isolated State**: Each test instantiates fresh `AppState` instances with unique UUIDs, eliminating test order dependencies.
