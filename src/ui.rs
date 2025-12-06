use crate::app::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Table, Row, Gauge},
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

    // Draw loading overlay or transaction list
    if app.is_loading_pools {
        draw_loading_overlay(f, app, chunks[1]);
    } else {
        // Draw transaction list
        draw_transaction_list(f, app, chunks[1]);
    }

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
            Span::raw(format!("TX: {}  | Pools: {} | Sync: {}", app.last_update, app.pool_count, app.last_pool_sync)),
        ])
    )
    .block(Block::default().borders(Borders::BOTTOM).style(Style::default().bg(Color::Black)))
    .alignment(Alignment::Left);

    f.render_widget(header, area);
}

fn draw_transaction_list(f: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    // Calculate how many rows can fit (subtract 3 for header, borders, etc)
    let available_rows = area.height.saturating_sub(3) as usize;
    let filtered_txs = app.get_filtered_transactions();
    let transactions = filtered_txs
        .into_iter()
        .skip(app.scroll_offset)
        .take(available_rows)
        .collect::<Vec<_>>();

    let rows: Vec<Row> = transactions
        .iter()
        .enumerate()
        .map(|(i, tx)| {
            let is_selected = app.selected_index == app.scroll_offset + i;
            
            // Style based on selection and DEX status
            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else if tx.is_dex {
                // Highlight DEX transactions in green
                Style::default()
                    .fg(Color::Green)
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

fn draw_loading_overlay(f: &mut Frame, app: &AppState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Loading Pools ")
        .title_alignment(Alignment::Center)
        .style(Style::default().bg(Color::Black));
    
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner area into sections
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),  // Title/status
            Constraint::Length(1),  // Pool counts
            Constraint::Length(1),  // Empty
            Constraint::Length(3),  // Progress bar
            Constraint::Min(0),     // Rest
        ])
        .split(inner);

    // Progress message
    let status_line = Line::from(vec![
        Span::styled(
            "⏳ ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        ),
        Span::styled(
            &app.pools_loading_progress,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        ),
    ]);
    
    let status_widget = ratatui::widgets::Paragraph::new(status_line)
        .alignment(Alignment::Center);
    f.render_widget(status_widget, chunks[0]);

    // Pool counts
    let pool_counts = Line::from(vec![
        Span::styled(
            "V2: ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        ),
        Span::styled(
            format!("{}", app.v2_pools_found),
            Style::default().fg(Color::Green)
        ),
        Span::raw("  |  "),
        Span::styled(
            "V3: ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        ),
        Span::styled(
            format!("{}", app.v3_pools_found),
            Style::default().fg(Color::Green)
        ),
        Span::raw("  |  "),
        Span::styled(
            "Total: ",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        ),
        Span::styled(
            format!("{}", app.pools_found_count),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        ),
    ]);
    
    let counts_widget = ratatui::widgets::Paragraph::new(pool_counts)
        .alignment(Alignment::Center);
    f.render_widget(counts_widget, chunks[1]);

    // Progress bar
    let progress_bar = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Progress "))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(app.pool_loading_progress_percent as u16)
        .label(format!("{}%", app.pool_loading_progress_percent));
    f.render_widget(progress_bar, chunks[3]);
}

fn draw_footer(f: &mut Frame, app: &AppState, area: ratatui::layout::Rect) {
    let help_text = "↑/↓ or Mouse Scroll: Navigate  | f: Filter (DEX/All)  | q: Quit  | Green = DEX transactions";
    let status = &app.status;
    let filter_status = format!("Filter: {} | TX Count: {}", 
        app.filter_mode.label(), 
        app.get_filtered_transaction_count()
    );

    let footer = ratatui::widgets::Paragraph::new(
        Line::from(vec![
            Span::raw(help_text),
            Span::raw("\n"),
            Span::styled(&filter_status, Style::default().fg(Color::Yellow)),
            Span::raw(" | "),
            Span::styled(status, Style::default().fg(Color::Cyan)),
        ])
    )
    .block(Block::default().borders(Borders::TOP))
    .alignment(Alignment::Left);

    f.render_widget(footer, area);
}
