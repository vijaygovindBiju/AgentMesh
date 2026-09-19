use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::tui::state::AppState;
use crate::tui::widgets::{DetailsPaneWidget, TaskCardWidget};

pub struct PlanReviewScreen;

impl PlanReviewScreen {
    pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
        if state.show_details_pane {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(45), // Task list
                    Constraint::Percentage(55), // Task details inspector
                ])
                .split(area);

            Self::render_task_list(frame, chunks[0], state);
            DetailsPaneWidget::render(frame, chunks[1], state);
        } else {
            Self::render_task_list(frame, area, state);
        }
    }

    fn render_task_list(frame: &mut Frame, area: Rect, state: &AppState) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Proposed Task Decomposition (Human Review) ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        if state.review_tasks.is_empty() {
            let empty =
                Paragraph::new("No tasks proposed yet. Go to [1: Input] to submit a project.")
                    .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, inner_area);
            return;
        }

        // Each card takes ~5 lines
        let card_height = 5u16;
        let visible_cards = (inner_area.height / card_height).max(1) as usize;

        // Compute scrolling window so selected item is always visible
        let scroll_offset = if state.selected_task_index >= visible_cards {
            state.selected_task_index - visible_cards + 1
        } else {
            0
        };

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
            );
            y += card_height;
        }
    }
}
