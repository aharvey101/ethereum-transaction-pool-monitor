mod app;
mod eth_client;
mod ui;
mod dex;
mod pool_db;
mod coingecko;

use app::AppState;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Setup terminal
    if let Err(e) = enable_raw_mode() {
        eprintln!("Failed to enable raw mode: {}", e);
        return;
    }

    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        eprintln!("Failed to setup terminal: {}", e);
        return;
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to create terminal: {}", e);
            return;
        }
    };

    // Get RPC URL from environment or use default
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    
    // Database path and chain ID (1 = Ethereum mainnet)
    let db_path = "dex_pools.db";
    let chain_id = 1u32;

    // Create app state
    let mut app = match AppState::new(&rpc_url, db_path, chain_id).await {
        Ok(app) => app,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            );
            let _ = terminal.show_cursor();
            eprintln!("Failed to initialize application: {}", e);
            eprintln!("Make sure your local Ethereum node is running.");
            eprintln!("You can set the RPC URL with: export ETH_RPC_URL=http://your-rpc-url:port");
            return;
        }
    };

    // Initial sync of DEX pools (will run in background)
    let app_ref = &mut app;
    let _ = app_ref.sync_dex_pools().await;

    // Run the main loop
    let _ = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> io::Result<()> {
    // Initial update
    let _ = app.update_transactions().await;

    let mut last_update = std::time::Instant::now();
    const UPDATE_INTERVAL: Duration = Duration::from_secs(5);

    loop {
        // Get terminal size for dynamic row calculation
        let terminal_height = terminal.size()?.height as usize;
        let available_rows = terminal_height.saturating_sub(7); // 3 for header, 3 for footer, 1 margin

        // Only redraw if something changed
        if app.needs_redraw {
            terminal.draw(|f| ui::draw(f, app))?;
            app.needs_redraw = false;
        }

        // Check if it's time for periodic updates (5 seconds)
        let now = std::time::Instant::now();
        let time_until_update = if now.duration_since(last_update) >= UPDATE_INTERVAL {
            Duration::from_millis(0)
        } else {
            UPDATE_INTERVAL - now.duration_since(last_update)
        };

        // Handle input with timeout
        if crossterm::event::poll(time_until_update)? {
            match event::read()? {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.is_running = false;
                            break;
                        }
                        KeyCode::Up => {
                            app.select_previous(available_rows);
                        }
                        KeyCode::Down => {
                            app.select_next(available_rows);
                        }
                        KeyCode::PageUp => {
                            app.scroll_up(5);
                        }
                        KeyCode::PageDown => {
                            app.scroll_down(5, available_rows);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            app.select_previous(available_rows);
                        }
                        MouseEventKind::ScrollDown => {
                            app.select_next(available_rows);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        } else {
            // Timeout expired, time for periodic update
            last_update = std::time::Instant::now();
            let _ = app.update_transactions().await;
        }
    }

    Ok(())
}
