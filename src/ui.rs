use crate::app::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Alignment},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Table, Row},
    Frame,
};

pub fn draw(f: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(0)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.area());

    // Draw header
    draw_header(f, app, chunks[0]);

    // Draw transaction list
    draw_transaction_list(f, app, chunks[1]);

    // Draw footer
    draw_footer(f, app, chunks[2]);
}

fn draw_header(f: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let title = "Ethereum Mempool Monitor";
    let status_color = if app.connection_healthy {
        Color::Green
    } else {
        Color::Red
    };
    let status_text = if app.connection_healthy {
        "Connected"
    } else {
        "Disconnected"
    };

    let header = ratatui::widgets::Paragraph::new(
        Line::from(vec![
            Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" | "),
            Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            Span::raw(" | "),
            Span::raw(format!("Last Update: {}", app.last_update)),
        ])
    )
    .block(Block::default().borders(Borders::BOTTOM).style(Style::default().bg(Color::Black)))
    .alignment(Alignment::Left);

    f.render_widget(header, area);
}

fn draw_transaction_list(f: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    // Calculate how many rows can fit (subtract 3 for header, borders, etc)
    let available_rows = area.height.saturating_sub(3) as usize;
    let transactions = app.get_visible_transactions(available_rows);

    let rows: Vec<Row> = transactions
        .iter()
        .enumerate()
        .map(|(i, tx)| {
            let is_selected = app.selected_index == app.scroll_offset + i;
            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let to_display = match &tx.to {
                Some(addr) => addr.clone(),
                None => "Contract Creation".to_string(),
            };

            Row::new(vec![
                Span::styled(&tx.from, style),
                Span::styled(to_display, style),
                Span::styled(&tx.value_eth, style),
                Span::styled(&tx.gas_price_gwei, style),
                Span::styled(format!("{}", tx.nonce), style),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(10),
        ],
    )
    .header(
        Row::new(vec!["From", "To", "Value", "Gas Price", "Nonce"])
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Pending Transactions ({}) ", app.transactions.len()))
            .title_alignment(Alignment::Left)
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    );

    f.render_widget(table, area);
}

fn draw_footer(f: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let help_text = "↑/↓: Navigate  | q: Quit  | Scroll through pending transactions";
    let status = &app.status;

    let footer = ratatui::widgets::Paragraph::new(
        Line::from(vec![
            Span::raw(help_text),
            Span::raw("\n"),
            Span::styled(status, Style::default().fg(Color::Cyan)),
        ])
    )
    .block(Block::default().borders(Borders::TOP))
    .alignment(Alignment::Left);

    f.render_widget(footer, area);
}
