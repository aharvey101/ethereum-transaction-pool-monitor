mod app;
mod ui;
mod eth_client;
mod pool_db;
mod pool_fetcher;
mod pool_loader;
mod transaction_updater;
mod transaction_decoder;
mod graph_client;

use app::AppState;
use pool_loader::BackgroundPoolLoader;
use transaction_updater::{BackgroundTransactionUpdater, TransactionUpdateMessage};
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use chrono::Local;

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

    // Don't load pools on startup - they're loaded from database if available
    // Pool loading is very slow and blocks the TUI
    tracing::info!("Application started with {} pools in database", app.pool_count);

    // Spawn background transaction updater FIRST (before pool loading)
    tracing::info!("Spawning background transaction updater task");
    let (_tx_updater, tx_updater_rx) = BackgroundTransactionUpdater::spawn(
        rpc_url.clone(),
        db_path.to_string(),
        chain_id,
    );
    tracing::info!("Background transaction updater spawned successfully");

    // Spawn background pool loader (runs independently) 
    tracing::info!("Spawning background pool loader task");
    let (_loader, pool_loader_rx) = BackgroundPoolLoader::spawn(
        rpc_url.clone(),
        db_path.to_string(),
        chain_id,
    );
    app.is_loading_pools = true;
    app.pools_loading_progress = "Pool scan starting...".to_string();

    // Update status to show transaction monitoring is starting
    app.status = "Transaction monitoring starting...".to_string();
    app.needs_redraw = true;
    
    // Run the main loop
    let _ = run_app(&mut terminal, &mut app, pool_loader_rx, tx_updater_rx).await;

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
    mut pool_loader_rx: tokio::sync::mpsc::UnboundedReceiver<pool_loader::PoolLoaderMessage>,
    mut tx_updater_rx: tokio::sync::mpsc::UnboundedReceiver<TransactionUpdateMessage>,
) -> io::Result<()> {
    use pool_loader::PoolLoaderMessage;
    
    // Background tasks will handle initial updates
    let _last_update = std::time::Instant::now();
    const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100); // Fast polling for responsiveness

    loop {
        tracing::trace!("Main loop iteration starting");
        
        // Get terminal size for dynamic row calculation
        let terminal_height = terminal.size()?.height as usize;
        let available_rows = terminal_height.saturating_sub(7); // 3 for header, 3 for footer, 1 margin

        // Check for pool loader messages (non-blocking)
        while let Ok(msg) = pool_loader_rx.try_recv() {
            match msg {
                PoolLoaderMessage::Progress(progress_msg, v2_count, v3_count, progress_pct) => {
                    app.v2_pools_found = v2_count;
                    app.v3_pools_found = v3_count;
                    app.pools_found_count = v2_count + v3_count;
                    app.pool_loading_progress_percent = progress_pct;
                    app.pools_loading_progress = progress_msg;
                    app.needs_redraw = true;
                    tracing::debug!("Pool loader progress: V2={} V3={} ({}%)", v2_count, v3_count, progress_pct);
                }
                PoolLoaderMessage::Complete(v2_count, v3_count, final_count) => {
                    app.v2_pools_found = v2_count;
                    app.v3_pools_found = v3_count;
                    app.pool_count = final_count;
                    app.pools_found_count = final_count;
                    app.pool_loading_progress_percent = 100;
                    app.is_loading_pools = false;
                    app.pools_loading_progress = format!("Loaded {} pools (V2: {}, V3: {})", final_count, v2_count, v3_count);
                    app.needs_redraw = true;
                    tracing::info!("Pool loader complete: V2={} V3={} Total={}", v2_count, v3_count, final_count);
                }
                PoolLoaderMessage::Error(err) => {
                    app.is_loading_pools = false;
                    app.pools_loading_progress = format!("Error: {}", err);
                    app.needs_redraw = true;
                    tracing::error!("Pool loader error: {}", err);
                }
            }
        }

        // Check for transaction update messages (non-blocking)
        while let Ok(msg) = tx_updater_rx.try_recv() {
            tracing::info!("Main: Received transaction update message");
            match msg {
                TransactionUpdateMessage::NewTransactions(transactions) => {
                    tracing::info!("Main: Received {} transactions from background", transactions.len());
                    
                    // Add new transactions to existing set (don't replace, just add new ones)
                    let mut added_count = 0;
                    for tx in transactions {
                        // Check if this transaction is already in our list  
                        if !app.transactions.iter().any(|existing| existing.hash == tx.hash) {
                            app.transactions.push_back(tx);
                            added_count += 1;
                        }
                    }
                    
                    // Limit total transactions
                    while app.transactions.len() > 2000 {
                        app.transactions.pop_front();
                    }
                    
                    let pending_count = app.transactions.len();
                    app.status = format!("{} transactions targeting block #{} (+{} new)", 
                        pending_count, app.next_block_number, added_count);
                    
                    app.last_update = Local::now().format("%H:%M:%S").to_string();
                    app.connection_healthy = true;
                    app.mark_cache_dirty();
                    app.cached_title = format!(" Targeting Block #{} ({} txs) ", 
                        app.next_block_number, app.transactions.len());
                    app.needs_redraw = true;
                    
                    tracing::info!("Main: Added {} new transactions, total: {} targeting block #{}", 
                        added_count, app.transactions.len(), app.next_block_number);
                }
                TransactionUpdateMessage::BlockChange { new_block_number, transactions } => {
                    tracing::info!("Main: NEW BLOCK #{} with {} transactions", new_block_number, transactions.len());
                    
                    // Clear all existing transactions - they were targeting the previous block
                    app.transactions.clear();
                    app.current_block_number = Some(new_block_number);
                    app.next_block_number = new_block_number + 1;
                    
                    // Add all new transactions targeting the next block
                    for tx in transactions {
                        app.transactions.push_back(tx);
                    }
                    
                    let pending_count = app.transactions.len();
                    app.status = format!("🆕 Block #{} mined! {} transactions now targeting block #{}", 
                        new_block_number, pending_count, app.next_block_number);
                    
                    app.last_update = Local::now().format("%H:%M:%S").to_string();
                    app.connection_healthy = true;
                    app.mark_cache_dirty();
                    app.cached_title = format!(" Targeting Block #{} ({} txs) ", 
                        app.next_block_number, app.transactions.len());
                    app.needs_redraw = true;
                    
                    tracing::info!("Main: Block change processed - {} transactions targeting block #{}", 
                        app.transactions.len(), app.next_block_number);
                }
                TransactionUpdateMessage::Error(err) => {
                    tracing::error!("Main: Received error from background: {}", err);
                    app.status = format!("Error: {}", err);
                    app.connection_healthy = false;
                    app.needs_redraw = true;
                    tracing::warn!("Background transaction update error: {}", err);
                }
            }
        }

        // Check if we should load more transactions (pagination)
        if app.should_load_more() {
            if let Err(e) = app.load_more_transactions().await {
                tracing::error!("Failed to load more transactions: {}", e);
            }
        }

        // Yield to allow background tasks to run
        tokio::task::yield_now().await;

        // Only redraw if something changed
        if app.needs_redraw {
            // Update cache before drawing to ensure performance and fresh display data
            let _ = app.get_filtered_transaction_count();
            let _ = app.get_filtered_sorted_transactions(); // Pre-populate the sorted cache
            terminal.draw(|f| ui::draw(f, app))?;
            app.needs_redraw = false;
        }

        // Poll for input with fast responsiveness (background tasks handle data updates)
        if crossterm::event::poll(INPUT_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.is_running = false;
                            break;
                        }
                        KeyCode::Char('f') => {
                            // Toggle filter between All and DEX only
                            app.toggle_filter();
                        }
                        KeyCode::Char('s') => {
                            // Toggle sort field
                            app.toggle_sort();
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
                        KeyCode::Char('r') => {
                            // Manually refresh pools - force a new pool loading process
                            if !app.is_loading_pools {
                                tracing::info!("User requested manual pool refresh");
                                app.is_loading_pools = true;
                                app.pools_loading_progress = "Manual pool refresh starting...".to_string();
                                app.pool_loading_progress_percent = 0;
                                app.needs_redraw = true;
                                
                                // TODO: Ideally we'd start a new BackgroundPoolLoader here,
                                // but that requires more complex channel management.
                                // For now, just show feedback that refresh was requested.
                                tokio::spawn(async move {
                                    // Simulate pool refresh feedback
                                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                                    tracing::info!("Pool refresh completed (placeholder)");
                                });
                            }
                        }
                        _ => {}
                    }
                }
                 Event::Mouse(mouse) => {
                     match mouse.kind {
                         MouseEventKind::ScrollUp => {
                             app.scroll_up(1);
                         }
                         MouseEventKind::ScrollDown => {
                             app.scroll_down(1, available_rows);
                         }
                         _ => {}
                     }
                 }
                _ => {}
            }
        }
        // Background tasks handle all data updates - no blocking operations in main loop
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

    // Pool loading - check if we need a comprehensive scan
    let force_refresh = std::env::var("FORCE_POOL_REFRESH").is_ok();
    let existing_pool_count = app.pool_count;
    
    if force_refresh {
        tracing::info!("FORCE_POOL_REFRESH enabled - running comprehensive pool scan");
        tracing::info!("Starting complete blockchain scan for all DEX pools (existing: {})", existing_pool_count);
        tracing::info!("This will collect pools from The Graph Protocol and take ~30 seconds");
        
        if let Err(e) = app.sync_pools_comprehensive().await {
            tracing::error!("Failed to sync DEX pools comprehensively: {}", e);
        } else {
            tracing::info!("✅ Comprehensive pool collection finished! Total pools: {}", app.pool_count);
        }
    } else if existing_pool_count < 100_000 {
        tracing::info!("Pool count low ({}), syncing DEX pools comprehensively from The Graph", existing_pool_count);
        if let Err(e) = app.sync_pools_comprehensive().await {
            tracing::error!("Failed to sync DEX pools from node: {}", e);
        } else {
            tracing::info!("DEX pools synced from node. Count: {}", app.pool_count);
        }
    } else {
        tracing::info!("Sufficient pools already in database ({})", existing_pool_count);
        tracing::info!("Use FORCE_POOL_REFRESH=1 to force a complete refresh");
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
