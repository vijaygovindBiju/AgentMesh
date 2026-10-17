# AgentMesh Test Infrastructure & Ratatui Verification Architecture

## 1. Opaque-Box, Requirement-Driven Test Philosophy

Testing terminal user interfaces requires a clear separation between internal implementation mechanics and user-observable behavior. The test philosophy for AgentMesh's Operator TUI is **opaque-box, requirement-driven verification**:

1. **Observable Buffer Assertions**:
   Tests run against Ratatui's `TestBackend`, rendering full layout frames into an in-memory character cell buffer (`Buffer`). Test assertions evaluate rendered text, visual indicators, title strings, status badges, and layout positioning directly from the buffer cells rather than querying private struct fields.

2. **Deterministic State Modeling**:
   TUI components are purely functional state projections:
   $$\text{Frame} = \text{render}(\text{AppState}, \text{TerminalArea})$$
   Given an identical `AppState` and terminal dimensions $(W, H)$, the resulting `Buffer` must be deterministic and reproducible across platforms.

3. **Decoupled Asynchronous Boundaries**:
   The TUI operates as a pure event consumer and action emitter:
   - It receives domain events via `TuiUpdateEvent` (e.g. `Tasks`, `Agents`, `RecentEvents`, `Connectivity`).
   - It processes keyboard input via `crossterm::event::KeyEvent`.
   - It emits user intents via `TuiAction` (e.g. `ApproveTask`, `RejectTask`, `LoadProject`, `RetryTask`, `CancelTask`).
   No network requests, database pools, or filesystem I/O are invoked within the rendering pipeline.

4. **Zero Regressions Baseline**:
   All 86 pre-existing tests in `crates/coordinator/tests/phase20_tui_verification.rs` serve as the inviolable baseline. Every Phase 21 enhancement must preserve backward compatibility with existing screen indexes, keyboard shortcuts, and state schemas.

---

## 2. Four-Tier Testing Methodology

The test suite is structured around a rigorous 4-tier verification hierarchy designed to test behavior from unit feature contracts up to multi-screen end-to-end operational workflows:

```
┌─────────────────────────────────────────────────────────────┐
│ Tier 4: Real-World Workflows                                │
│   - End-to-end operator sessions & state transitions        │
├─────────────────────────────────────────────────────────────┤
│ Tier 3: Cross-Feature Combinations                          │
│   - Modal dialogs over complex screens, action gating       │
├─────────────────────────────────────────────────────────────┤
│ Tier 2: Boundary & Corner Conditions                        │
│   - Extreme dimensions (60x18), buffer bursts, empty states │
├─────────────────────────────────────────────────────────────┤
│ Tier 1: Feature Coverage                                    │
│   - Dedicated unit tests per screen, widget, and hotkey     │
└─────────────────────────────────────────────────────────────┘
```

### Tier 1: Feature Coverage (Primary Happy Paths)
Validates core functionality for every distinct component and screen:
- **Screen Navigation**: Verification of active screen titles, numeric switching (`1`–`5`), `Tab` / `BackTab` cycling across all 5 screens (`Dashboard`, `ProjectManagement`, `PlanReview`, `FleetInspector`, `Diagnostics`).
- **Global Status Header**: Validates rendering of connectivity indicators (`[PG: ● OK]`, `[NATS: ● OK]`), active agent ratio (`[Agents: M/N]`), and pending approvals counter (`[Pending: K]`).
- **Command Palette Modal (`:`)**: Opening via `:`, input typing, fuzzy/substring query filtering, selection navigation (`Up`/`Down`), and action dispatch.
- **Safety Confirmation Dialog**: Interception of destructive actions (`CancelTask`, `RejectTask`), rendering title and warning message, confirming via `y` / `Enter`, aborting via `n` / `Esc`.
- **Project Management Screen**: Navigation of projects table (`j`/`k`/`Up`/`Down`), emitting `TuiAction::LoadProject`, display of project metadata and Git branch info.
- **Interactive DAG Dependency Tree**: Rendering multi-level recursive dependency trees ($A \rightarrow B \rightarrow C$), topological execution tiers, status indicators (`[P]`, `[A]`, `[E]`, `[C]`, `[F]`), and blocker/dependent jump shortcuts (`[` / `]`).
- **Fleet Inspector & Telemetry Cards**: Rendering of agent cards, liveness timestamps, heartbeat latency, runtime badges, error banners, and capability pills.
- **Diagnostics Event Stream**: Event table rendering, timestamp formatting, entity badge display, and severity/entity filter controls.

### Tier 2: Boundary & Corner Cases
Stresses resilience against extreme conditions, empty buffers, and input bounds:
- **Terminal Resize Resilience**: Deterministic rendering across 5 standard and compact profiles without panics or clipping:
  1. Compact Minimal: `60x18`
  2. Compact Standard: `70x20`
  3. Standard Terminal: `80x24`
  4. Expanded Desktop: `100x30`
  5. Ultra-Wide Console: `160x50`
- **Rapid Event Burst Stress**: Flooding the event ring buffer with 1,000 to 10,000 events to verify the invariant:
  $$\text{capacity} \le 300 \quad \text{and} \quad O(1)\text{ insertion amortized}$$
- **Empty State Fallbacks**: Rendering when zero tasks, zero agents, zero projects, or zero events exist without crashes or visual corruption.
- **Selection Index Clamping**: Scrolling beyond buffer boundaries (`index > len`) clamped safely to zero or `len - 1`.

### Tier 3: Cross-Feature Combinations
Tests interactions between overlapping UI systems:
- **Modal Layering & Focus Trapping**: Verifying that opening a modal (Help, Command Palette, Confirmation) captures keyboard events and prevents background screen shortcut leakage.
- **Action Gating with Overlaps**: Tasks with unacknowledged overlap warnings require explicit acknowledgement (`a`) or safety confirmation before approval transitions.
- **Project Switching State Reset**: Loading a new project clears old review states, updates DAG selection, and refreshes the status header.

### Tier 4: Real-World Operator Workflows
Simulates complete operator sessions:
- **Review & Approval Session**: Operator loads project $\rightarrow$ inspects DAG blockers $\rightarrow$ acknowledges overlap warning $\rightarrow$ approves unblocked tasks $\rightarrow$ attempts cancellation on failed task with confirmation.
- **Incident Response & Fleet Triage**: Alert arrives $\rightarrow$ operator navigates to Fleet Inspector $\rightarrow$ identifies degraded agent with high heartbeat latency $\rightarrow$ drills down into Diagnostics event stream filtered by entity ID $\rightarrow$ issues retry.

---

## 3. Test Harness & Helper Architecture

The test suite relies on lightweight, zero-dependency helpers built on Ratatui's `TestBackend`:

```rust
// In-memory terminal instantiation
fn create_terminal(width: u16, height: u16) -> Terminal<TestBackend>;

// Full buffer dump to multi-line string
fn buffer_to_string(terminal: &Terminal<TestBackend>) -> String;

// Case-sensitive assertion that expected string appears in rendered terminal
fn assert_buffer_contains(terminal: &Terminal<TestBackend>, expected: &str);

// Case-sensitive assertion that string does NOT appear in rendered terminal
fn assert_buffer_does_not_contain(terminal: &Terminal<TestBackend>, unexpected: &str);

// Synthetic keyboard event generator
fn press_key(code: KeyCode) -> KeyEvent;
fn press_char(c: char) -> KeyEvent;
fn press_modified(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent;
```

---

## 4. Verification Invariants Reference Table

| Target Feature | Invariant / Requirement | Verification Method |
|---|---|---|
| Header Connectivity | Displays `[PG: ● OK]` / `[PG: ○ OFF]` and `[NATS: ● OK]` / `[NATS: ○ OFF]` | String match in top header line |
| Header Agent Ratio | Displays `[Agents: {active}/{total}]` | String match in top header line |
| Header Approvals | Displays `[Pending: {count}]` | String match in top header line |
| Command Palette | Active on `:` key; filters commands on text input; closes on `Esc` | Modal render assertion & key dispatch |
| Confirmation Modal | Destructive actions require confirmation; cancels on `n`/`Esc`; executes on `y`/`Enter` | Dialog render assertion & action check |
| Recursive DAG Tree | Recursively displays children up to leaf nodes ($A \rightarrow B \rightarrow C$) | Depth indent & dependency link assertions |
| DAG Jump Navigation | `[` jumps to upstream blocker; `]` jumps to downstream dependent | `selected_task_index` update verification |
| Fleet Inspector | Displays health badges (`Healthy`, `Degraded`, `Unhealthy`) & latency | Buffer check in agent cards |
| Bounded Event Ring Buffer | `recent_events.len() <= 300` under 10,000 rapid event inserts | Vector/deque length assertion |
| Event Filters | Filters events by severity (`ERROR`, `WARN`, `INFO`) or entity ID | Buffer check of filtered stream |
| Resize Resilience | No panics, valid buffer layout for dimensions from 60x18 to 160x50 | Multi-size render loop |

---

## 5. Execution Command

To execute the full test battery:

```bash
# Run existing Phase 20 baseline verification
cargo test -p coordinator --test phase20_tui_verification

# Run Phase 21 Operator TUI enhancements test suite
cargo test -p coordinator --test phase21_tui_enhancements

# Run entire workspace
cargo test --workspace
```
