use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::state::{AppState, PlanReviewFocus};
use crate::tui::widgets::{DagViewWidget, DetailsPaneWidget, TaskCardWidget};

pub struct PlanReviewScreen;

impl PlanReviewScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if state.show_dag_view {
            if state.show_details_pane {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(50), // DAG Graph view
                        Constraint::Percentage(50), // Task details inspector
                    ])
                    .split(area);

                DagViewWidget::render(frame, chunks[0], state);
                DetailsPaneWidget::render(frame, chunks[1], state);
            } else {
                DagViewWidget::render(frame, area, state);
            }
        } else if state.show_details_pane {
            let (left_constraint, right_constraint) = if area.width < 90 {
                (Constraint::Percentage(46), Constraint::Percentage(54))
            } else if area.width < 140 {
                (Constraint::Percentage(44), Constraint::Percentage(56))
            } else {
                (Constraint::Length(64), Constraint::Min(76))
            };

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([left_constraint, right_constraint])
                .split(area);

            Self::render_task_list(frame, chunks[0], state);
            DetailsPaneWidget::render(frame, chunks[1], state);
        } else {
            Self::render_task_list(frame, area, state);
        }
    }

    fn render_task_list(frame: &mut Frame, area: Rect, state: &AppState) {
        let is_focused = state.plan_review_focus == PlanReviewFocus::TaskList;
        let (border_type, border_color) = if is_focused {
            (BorderType::Thick, Color::Cyan)
        } else {
            (BorderType::Rounded, Color::DarkGray)
        };

        let base_title = if area.width >= 62 {
            "Proposed Task Decomposition (Human Review)"
        } else if area.width >= 30 {
            "Proposed Tasks (Review)"
        } else {
            "Tasks"
        };

        let title_text = if is_focused {
            format!(" {base_title} [▶ FOCUSED] ")
        } else {
            format!(" {base_title} ")
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                title_text,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if state.review_tasks.is_empty() {
            let empty_lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  ℹ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(
                        "No tasks proposed yet. Go to [2: Proj] to submit a project.",
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Quick Start Guide:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("  1. Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[2]", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(" to open Project Management.", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(vec![
                    Span::styled("  2. Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[I]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(" to enter a new repository path and development goal.", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(vec![
                    Span::styled("  3. Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(" to submit. Intelligent Planner will decompose it into a DAG.", Style::default().fg(Color::DarkGray)),
                ]),
                Line::from(vec![
                    Span::styled("  4. Return here to review, inspect, edit, or approve tasks [Y] / [Shift+A].", Style::default().fg(Color::DarkGray)),
                ]),
            ];
            let empty = Paragraph::new(empty_lines);
            frame.render_widget(empty, inner_area);
            return;
        }

        // Each card takes ~5 lines
        let card_height = 5u16;
        let visible_cards = (inner_area.height / card_height).max(1) as usize;

        // Sliding-window scrolling offset so active card is visible without pinning to bottom
        let scroll_offset = state.plan_task_scroll_offset(visible_cards);

        let mut y = inner_area.y;
        for (idx, task_item) in state
            .review_tasks
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_cards)
        {
            if y + card_height > inner_area.y + inner_area.height {
                break;
            }

            let card_rect = Rect {
                x: inner_area.x,
                y,
                width: inner_area.width,
                height: card_height,
            };

            TaskCardWidget::render(
                frame,
                card_rect,
                task_item,
                idx == state.selected_task_index,
                state.selected_task_indices.contains(&idx),
            );
            y += card_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;
    use uuid::Uuid;
    use crate::domain::{Task, TaskStatus};
    use crate::tui::state::ReviewTaskState;

    fn make_test_task(short_id: &str, title: &str) -> ReviewTaskState {
        ReviewTaskState {
            task: Task {
                id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                short_id: short_id.to_string(),
                title: title.to_string(),
                description: format!("Description for {short_id}"),
                affected_resources: json!(["crates/coordinator"]),
                status: TaskStatus::HumanReview,
                assigned_agent_id: None,
                estimated_size: Some("M".to_string()),
                proposal_id: Uuid::new_v4(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            suggested_agent_name: Some("WorkerAgent".to_string()),
            overlap_warnings: Vec::new(),
            dependencies: Vec::new(),
            human_decision: None,
            progress_percent: None,
        }
    }

    #[test]
    fn test_plan_review_adaptive_title_and_focus_badge() {
        let backend = TestBackend::new(120, 25);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.review_tasks.push(make_test_task("T-1", "First Task"));
        state.review_tasks.push(make_test_task("T-2", "Second Task"));
        state.plan_review_focus = PlanReviewFocus::TaskList;

        terminal.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rendered: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rendered.contains("Proposed Tasks (Review) [▶ FOCUSED]"));

        // Switch focus away from TaskList to TaskInspector
        state.plan_review_focus = PlanReviewFocus::TaskInspector;
        terminal.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rendered2: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rendered2.contains(" Proposed Tasks (Review) "));
        assert!(!rendered2.contains("Proposed Tasks (Review) [▶ FOCUSED]"));
        assert!(rendered2.contains("[▶ FOCUSED] Task Details & Inspector"));

        // On wide terminals (>=140 cols), task list width is 64 >= 62, displaying full title and focus badge
        let backend_wide = TestBackend::new(180, 25);
        let mut term_wide = Terminal::new(backend_wide).unwrap();
        state.plan_review_focus = PlanReviewFocus::TaskList;
        term_wide.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rendered_wide: String = term_wide.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rendered_wide.contains("Proposed Task Decomposition (Human Review)"));
        assert!(rendered_wide.contains("[▶ FOCUSED]"));
    }

    #[test]
    fn test_plan_review_compact_and_wide_rendering() {
        // Compact (<90 cols)
        let backend_compact = TestBackend::new(80, 24);
        let mut term_compact = Terminal::new(backend_compact).unwrap();
        let mut state = AppState::new();
        state.review_tasks.push(make_test_task("T-1", "Compact Task"));
        term_compact.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rend_compact: String = term_compact.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rend_compact.contains("Proposed Tasks (Review)"));

        // Wide (>=140 cols)
        let backend_wide = TestBackend::new(180, 40);
        let mut term_wide = Terminal::new(backend_wide).unwrap();
        term_wide.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rend_wide: String = term_wide.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rend_wide.contains("Proposed Task Decomposition (Human Review)"));
    }

    #[test]
    fn test_plan_review_sliding_window_scrolling() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        for i in 1..=10 {
            state.review_tasks.push(make_test_task(&format!("T-{i}"), &format!("Task {i}")));
        }

        // Navigate downwards to task 8
        state.selected_task_index = 8;
        terminal.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rend_down: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rend_down.contains("[T-8]"));

        // Navigate upwards to task 5
        state.selected_task_index = 5;
        terminal.draw(|f| PlanReviewScreen::render(f, f.area(), &state)).unwrap();
        let rend_up: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(rend_up.contains("[T-5]"));
    }
}
