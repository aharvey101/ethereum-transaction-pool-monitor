mod app;
mod eth_client;
mod ui;
mod dex;
mod pool_db;
mod coingecko;

use app::AppState;
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;

#[tokio::main]
async fn main() {
    // Setup logging
    setup_logging();

    // Check for debug mode
    let debug_mode = std::env::var("DEBUG_MODE").is_ok();
    
    if debug_mode {
        tracing::info!("Running in DEBUG mode (headless)");
        run_headless().await;
    } else {
        run_tui().await;
    }
}

/// Setup logging to both file and stdout
fn setup_logging() {
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Arc::new(
            std::fs::File::create("ethereum-monitor.log").unwrap_or_else(|_| {
                eprintln!("Warning: Could not create log file");
                std::fs::File::open("/dev/null").unwrap()
            })
        ))
        .with_target(true)
        .with_level(true);
    
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .with(file_layer);
    
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");
}

/// Run the TUI application
async fn run_tui() {
    // Setup terminal
    if let Err(e) = enable_raw_mode() {
        eprintln!("Failed to enable raw mode: {}", e);
        return;
    }

    let mut stdout = io::stdout();
    if let Err(e) = execute!(stdout, crossterm::terminal::EnterAlternateScreen, DisableMouseCapture) {
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

/// Run in headless mode (no TUI, just logging)
async fn run_headless() {
    // Get RPC URL from environment or use default
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    
    // Database path and chain ID
    let db_path = "dex_pools.db";
    let chain_id = 1u32;

    tracing::info!("Initializing Ethereum mempool monitor");
    tracing::info!("RPC URL: {}", rpc_url);
    tracing::info!("Database: {}", db_path);
    
    // Create app state
    let mut app = match AppState::new(&rpc_url, db_path, chain_id).await {
        Ok(app) => {
            tracing::info!("Application initialized successfully");
            app
        }
        Err(e) => {
            tracing::error!("Failed to initialize application: {}", e);
            return;
        }
    };

    // Sync DEX pools
    tracing::info!("Syncing DEX pools from CoinGecko");
    if let Err(e) = app.sync_dex_pools().await {
        tracing::error!("Failed to sync DEX pools: {}", e);
    } else {
        tracing::info!("DEX pools synced. Count: {}", app.pool_count);
    }

    // Run update loop
    let mut update_count = 0;
    loop {
        update_count += 1;
        tracing::info!("Transaction update #{}", update_count);
        
        if let Err(e) = app.update_transactions().await {
            tracing::error!("Failed to fetch transactions: {}", e);
        } else {
            tracing::info!("Fetched {} pending transactions", app.transactions.len());
            
            // Log DEX transactions
            let dex_count = app.transactions.iter().filter(|tx| tx.is_dex).count();
            tracing::info!("DEX transactions: {}", dex_count);
            
            // Log details of DEX transactions
            for tx in app.transactions.iter().filter(|tx| tx.is_dex) {
                tracing::info!(
                    "DEX TX - From: {} | To: {} | Value: {} | Gas: {}",
                    &tx.from[..std::cmp::min(10, tx.from.len())],
                    &tx.to.as_ref().map(|a| &a[..std::cmp::min(10, a.len())]).unwrap_or(&"N/A"),
                    tx.value_eth,
                    tx.gas_price_gwei
                );
            }
        }

        // Wait 5 seconds before next update
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
