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
    
    // Use cached filtered/sorted data (cache is pre-populated in main loop for performance)
    // Direct access to cached indices to avoid expensive re-sorting
    let transactions = app.cached_filtered_sorted
        .iter()
        .skip(app.scroll_offset)
        .take(available_rows)
        .map(|&i| &app.transactions[i])
        .collect::<Vec<_>>();

    // Pre-allocate row vector for better performance
    let mut rows = Vec::with_capacity(transactions.len());
    
    for (i, tx) in transactions.iter().enumerate() {
        let is_selected = app.selected_index == app.scroll_offset + i;
        
        // Style based on selection and DeFi activity type
        let style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            // Different colors for different DeFi activity types
            use crate::eth_client::DefiActivityType;
            match tx.defi_activity_type {
                DefiActivityType::Stablecoin => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                DefiActivityType::TokenContract => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                DefiActivityType::DexPool => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                DefiActivityType::DexRouter => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                DefiActivityType::None => Style::default(),
            }
        };

        // Avoid string allocation for common case
        let to_display = tx.to.as_deref().unwrap_or("Contract Creation");

        // Create row - balance performance with lifetime requirements
        rows.push(Row::new(vec![
            tx.from.clone(), // Clone needed for owned string
            to_display.to_string(), // Clone needed for owned string
            tx.value_eth.clone(), // Clone needed for owned string 
            tx.gas_price_gwei.clone(), // Clone needed for owned string
            tx.nonce.to_string(), // Convert to owned string
        ]).style(style));
    }

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
            .title(&*app.cached_title) // Use cached title to avoid repeated format! calls
            .title_alignment(Alignment::Left)
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
    let help_text = "↑/↓ or Mouse Scroll: Navigate  | f: Filter (DeFi/All) | s: Sort  | q: Quit";
    let color_legend = "Colors: Blue=Stablecoin | Yellow=Token | Green=DEX Pool | Cyan=DEX Router";
    let status = &app.status;
    let filter_status = format!("Filter: {} | Sort: {} | TX Count: {}", 
        match app.filter_mode {
            crate::app::FilterMode::All => "All Transactions",
            crate::app::FilterMode::DexOnly => "DeFi Only",
        },
        app.sort_field.label(),
        app.get_filtered_transaction_count_display()
    );

    let footer = ratatui::widgets::Paragraph::new(
        vec![
            Line::from(vec![Span::raw(help_text)]),
            Line::from(vec![Span::styled(color_legend, Style::default().fg(Color::Gray))]),
            Line::from(vec![
                Span::styled(&filter_status, Style::default().fg(Color::Yellow)),
                Span::raw(" | "),
                Span::styled(status, Style::default().fg(Color::Cyan)),
            ]),
        ]
    )
    .block(Block::default().borders(Borders::TOP))
    .alignment(Alignment::Left);

    f.render_widget(footer, area);
}
