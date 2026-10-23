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
   All 86 pre-existing tests in `crates/coordinator/tests/phase20_tui_verification.rs` and 62 tests in `crates/coordinator/tests/phase21_tui_enhancements.rs` serve as the inviolable baseline. Every Phase 22 enhancement must preserve backward compatibility with existing screen indexes, keyboard shortcuts, and state schemas.

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

### Phase 21 Baseline Summary
- **Tier 1 (Feature Coverage)**: Screen navigation across all top-level screens, status header connectivity indicators, command palette modal (`:`), safety confirmation modal, project management table, recursive DAG trees, and fleet inspector cards.
- **Tier 2 (Boundary & Corner Cases)**: Terminal resizing across 5 profiles (`60x18`, `70x20`, `80x24`, `100x30`, `160x50`), event ring buffer bounds ($\le 300$), and selection index clamping.
- **Tier 3 (Combinations)**: Modal layering, focus trapping, overlap warning action gating, and project switching state resets.
- **Tier 4 (Real-World Workflows)**: End-to-end operator review sessions, DAG blocker drill-downs, and incident triage.

---

### Phase 22 Ergonomics & Plan Review 4-Tier Test Architecture

Phase 22 focuses on the visual layout, rendering, and usability of the Plan Review screen, task list, task inspector, keyboard navigation, and responsive terminal resizing per requirements R1 through R5:

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ Tier 4: Real-World Operator Workflows                                        │
│   - Complete operator review cycle (compact -> standard -> wide terminal)    │
│   - Rapid terminal resize with active multi-selection & inspector state      │
│   - DAG blocker traversal, overlap acknowledgement & selective bulk approval  │
├──────────────────────────────────────────────────────────────────────────────┤
│ Tier 3: Cross-Feature Combinations                                           │
│   - Combined focused + multi-selected card styling (▶ [✓] indicator)         │
│   - Mouse scroll column disambiguation (left -> task list, right -> inspector)│
│   - Inspector scroll offset auto-reset on task cursor navigation             │
│   - PlanReviewFocus pane switching (TaskList <-> TaskInspector via Tab/Arrows)│
│   - Selective Shift+A (subset approval) vs empty Shift+A (all pending tasks) │
├──────────────────────────────────────────────────────────────────────────────┤
│ Tier 2: Boundary & Corner Conditions                                         │
│   - Empty plan quick start guide rendering at 80x24, 120x30, 180x50          │
│   - Task with extensive multi-line description (>10 lines) & word wrapping   │
│   - Task with many dependencies (>3 items) dynamic height allocation         │
│   - Long task title graceful truncation with ellipsis (...)                  │
│   - Selection bounds (Ctrl+A all, Esc clear, Space toggle, cursor clamping)  │
├──────────────────────────────────────────────────────────────────────────────┤
│ Tier 1: Feature Coverage                                                     │
│   - Responsive two-column layout at 80x24, 120x30, 180x50 (compact/std/wide) │
│   - Content-adaptive task inspector sections (collapse empty, expand items)  │
│   - Task card metadata layout (status, agent, progress bar, dependencies)    │
│   - Keyboard navigation (arrows, Enter details, Space toggle, single-keys)   │
│   - Footer action hint visibility & absence of clipping at 80, 120, 180 cols │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### Tier 1: Feature Coverage (Primary Happy Paths)
1. **Responsive Two-Column Plan Layout**:
   - Compact (<90 cols, e.g. 80x24): 46%/54% split, compact title `" Proposed Tasks "`, unclipped borders.
   - Standard (90..139 cols, e.g. 120x30): 44%/56% split, standard title `" Proposed Task Decomposition (Human Review) "`.
   - Wide (>=140 cols, e.g. 180x50): Fixed 64-col task list, min 76-col inspector eliminating wasted whitespace.
2. **Content-Adaptive Task Inspector**:
   - All 5 sections render cleanly: Metadata, Description, Dependencies, Affected Resources, Overlap Analysis.
   - Empty sections collapse to `Length(3)` with green/dark badges, avoiding squashing descriptions.
   - Section content wraps cleanly with `Wrap { trim: true }`.
3. **Task Card Metadata & Progress Display**:
   - Formatted in multi-line structure so status badge (`HUMAN REVIEW`, `APPROVED`, `REJECTED`), agent assignee, and progress bar (`[██████████] 100%`) are fully visible without horizontal clipping.
4. **Keyboard Navigation & Hotkeys**:
   - Arrow keys (`Up`/`Down`) navigate cards smoothly.
   - `Enter` toggles details inspector pane.
   - `Space` toggles task selection.
   - Single-key actions: `[Y]` approve, `[N]` reject, `[E]` edit description, `[C]` cancel, `[A]` ack overlap.
5. **Contextual Footer Hints Synchronization**:
   - Footer hints render without truncation at 80x24, 120x30, and 180x50.
   - Primary operator actions (`[Y]`, `[N]`, `[E]`, `[C]`, `[Shift+A]`) preserved in compact mode (<85 cols).

#### Tier 2: Boundary & Corner Conditions
1. **Empty Plan State Resilience**:
   - Zero tasks in plan: renders 4-step quick start guide (`[2]` Proj, `[I]` Input, `[Enter]` Submit) across 80x24, 120x30, and 180x50 without panics or border collisions.
2. **Extensive Multi-Line Description (>10 lines)**:
   - Paragraph wraps cleanly without ragged indents (`trim: true`) and scrolls vertically via `inspector_scroll_offset`.
3. **Extensive Dependency Chain**:
   - Tasks with >3 prerequisites allocate dynamic height or render scrollable content without clipping outer borders.
4. **Long Task Titles with Ellipsis**:
   - Long titles (e.g. 60+ chars on 32-col inner width) are gracefully truncated with ellipsis (`...`) preserving short ID and selection prefix.
5. **Selection Boundary Handling**:
   - `Ctrl+A` selects all tasks, `Esc` clears selection, `Space` toggles individual tasks, cursor clamps at `review_tasks.len() - 1`.

#### Tier 3: Cross-Feature Combinations
1. **Focused + Multi-Selected Visual Distinction**:
   - Card simultaneously focused and selected renders combined indicator `▶ [✓] ` with dual-highlight border/badge, distinguishing it from merely focused (`▶   `) or merely selected (`  ✓ `).
2. **Mouse Scroll Column Disambiguation**:
   - Mouse wheel over left column (`column < list_width`) moves `selected_task_index`.
   - Mouse wheel over right column (`column >= list_width`) moves `inspector_scroll_offset`.
3. **Inspector Scroll Reset on Task Switch**:
   - Moving cursor (`selected_task_index` changes) resets `inspector_scroll_offset = 0`, ensuring inspector always opens at top of newly selected task.
4. **PlanReviewFocus Pane Switching**:
   - `Tab` or `Left`/`Right` switches `plan_review_focus` between `TaskList` and `TaskInspector` when details pane is open. Active pane displays `[▶ FOCUSED]` and `BorderType::Thick` / `Color::Cyan`.
5. **Selective `Shift+A` vs All-Tasks `Shift+A`**:
   - When `selected_task_indices` is non-empty: `Shift+A` prompts `"Approve selected X task(s)?"` and approves only the selected subset.
   - When `selected_task_indices` is empty: `Shift+A` prompts to approve all pending tasks in the plan.

#### Tier 4: Real-World Operator Workflows
1. **End-to-End Operator Plan Review Workflow**:
   - Operator starts in compact terminal -> resizes to 120x30 -> navigates tasks -> switches focus to inspector -> scrolls long description -> selects tasks 1 and 3 -> bulk-approves selected with `Shift+A` -> confirms dialog -> verifies final state.
2. **Live Resize Invariant Preservation**:
   - Resizing terminal dynamically (80x24 -> 120x30 -> 180x50 -> 80x24) retains cursor position, multi-selected set, and inspector state without visual tearing.
3. **DAG Blocker Traversal & State Transition Cycle**:
   - Traverse blockers via `[` / `]`, toggle DAG view with `G`, acknowledge overlap warnings with `A`, approve unblocked tasks with `Y`, cancel blocked task with `C`.

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

// Synthetic keyboard & mouse event generators
fn press_key(code: KeyCode) -> KeyEvent;
fn press_char(c: char) -> KeyEvent;
fn press_modified(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent;
fn mouse_scroll(column: u16, row: u16, kind: MouseEventKind) -> MouseEvent;
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
| Resize Resilience | No panics, valid buffer layout for dimensions from 60x18 to 180x50 | Multi-size render loop |
| **Plan Two-Column Split (R1)** | Compact (<90 cols) 46%/54%, Standard 44%/56%, Wide (>=140 cols) fixed 64 / min 76 | Layout chunk width assertion & buffer check |
| **Inspector Dynamic Heights (R2)** | Empty sections collapse to `Length(3)`, occupied sections expand dynamically | Section height & paragraph bounds |
| **Inspector Word Wrapping (R2)** | Clean wrapping with `Wrap { trim: true }`, no ragged leading spaces | Line start trim assertion |
| **Inspector Vertical Scroll (R2)** | `inspector_scroll_offset` scrolls description/content; resets on task change | Buffer content shift assertion |
| **Card Focus+Select Contrast (R3)** | Distinct indicator `▶ [✓]` when both focused and selected | Buffer match on card prefix |
| **Card Metadata Alignment (R3)** | Status, agent, and progress bar visible on 80 and 120 col screens | Buffer match on all 3 metadata items |
| **Title Ellipsis Truncation (R3)** | Titles exceeding inner width gracefully truncated with `...` | Buffer match on `...` and absence of overflow |
| **Sliding-Window Card Scroll (R3)** | Upward navigation scrolls smoothly without pinning cursor to bottom row | `scroll_offset` bounds assertion |
| **Pane Focus Switching (R4)** | `Tab`/Arrows switch between `TaskList` and `TaskInspector`; active pane displays `[▶ FOCUSED]` | Focus enum & buffer check |
| **Mouse Column Routing (R4)** | Left pane mouse scroll moves task index; right pane mouse scroll adjusts inspector offset | MouseEvent dispatch & state verification |
| **Selective Bulk Approval (R4)** | `Shift+A` with multi-selection approves only selected; empty selection approves all pending | Task status transitions verification |
| **Footer Hints & Toast (R4)** | No hint truncation at 80, 120, 180 cols; status toast allocated >=34 cols | Buffer match on hints and full toast string |
| **Visual Consistency (R5)** | Cyan rounded borders, dark background, standard status color badges | Style and border assertions |

---

## 5. Execution Command

To execute the test battery:

```bash
# Run existing Phase 20 baseline verification
cargo test -p coordinator --test phase20_tui_verification

# Run Phase 21 Operator TUI enhancements test suite
cargo test -p coordinator --test phase21_tui_enhancements

# Run Phase 22 Plan Review Ergonomics test suite
cargo test -p coordinator --test phase22_plan_review_ergonomics

# Run all coordinator TUI test suites together
cargo test -p coordinator --test phase4_tui --test phase20_tui_verification --test phase21_tui_enhancements --test phase22_plan_review_ergonomics

# Run entire workspace
cargo test --workspace
```
