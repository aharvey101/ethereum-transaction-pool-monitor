//! Advanced Ethereum MEV Bot - Direct mempool execution for sandwich attacks
//!
//! Production-ready MEV bot with arboo-style direct mempool submission capabilities.
//! Bypasses Flashbots for immediate transaction execution with competitive gas pricing.

#![allow(dead_code)] // Allow unused code in production MEV bot

mod app;
mod bot_runner;
mod dex;
// mod dynamic_pool_discovery;
mod enhanced_revm_simulator;
mod eth_client;
mod flash_loan_manager;
mod graph_client;
mod mempool_monitor;
mod mev_bundle_builder;
// mod pool_calculators;
mod pool_db;
mod pool_fetcher;
mod pool_loader;
mod pool_state_fetcher;
mod sandwich_pool_integration;
mod transaction_decoder;
mod transaction_executor;
mod transaction_updater;
mod ui;

use alloy_primitives::U256;
use anyhow::Result;
use app::AppState;
use bot_runner::{BotConfig, MevBotRunner};
use chrono::Local;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen},
};
use enhanced_revm_simulator::{EnhancedSandwichSimulator, PoolSelectionCriteria};
use pool_loader::BackgroundPoolLoader;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use transaction_updater::{BackgroundTransactionUpdater, TransactionUpdateMessage};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(name = "ethereum-transaction-pool-monitor")]
#[command(about = "Advanced Ethereum Transaction Pool Monitor with MEV Sandwich Simulation")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// RPC URL for Ethereum node
    #[arg(long, default_value = "http://192.168.0.14:8545")]
    rpc_url: String,

    /// Database path for pool storage
    #[arg(long, default_value = "./dex_pools.db")]
    db_path: String,

    /// Enable debug mode (headless operation)
    #[arg(long)]
    debug: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the TUI transaction monitor (default)
    Monitor,

    /// Run enhanced REVM sandwich simulation
    Sandwich {
        /// Victim trade amount in ETH
        #[arg(long, default_value = "5.0")]
        victim_amount: f64,

        /// Minimum liquidity threshold in USD
        #[arg(long, default_value = "100000.0")]
        min_liquidity: f64,

        /// Maximum price impact allowed
        #[arg(long, default_value = "0.05")]
        max_price_impact: f64,

        /// Number of pools to analyze
        #[arg(long, default_value = "10")]
        pool_count: usize,
    },

    /// Run multi-DEX comprehensive analysis
    MultiDex {
        /// Number of pools per protocol to test
        #[arg(long, default_value = "20")]
        pools_per_protocol: usize,
    },

    /// Run basic sandwich integration test
    Integration,

    /// Run continuous MEV bot (real-time mempool monitoring)
    Bot {
        /// Minimum profit threshold in ETH
        #[arg(long, default_value = "0.01")]
        min_profit: f64,

        /// Maximum gas price in gwei
        #[arg(long, default_value = "50")]
        max_gas_price: u64,

        /// Enable Flashbots bundle submission
        #[arg(long)]
        enable_flashbots: bool,

        /// Use direct mempool submission instead of Flashbots (like arboo)
        #[arg(long)]
        direct_mempool: bool,

        /// Private key for direct mempool submission (hex format)
        #[arg(long)]
        private_key: Option<String>,

        /// Simulation only mode (no real transactions)
        #[arg(long)]
        simulation_only: bool,
    },

    /// Test and compare gas price estimates (OLD vs NEW)
    TestGas,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    setup_logging();

    // Check for debug mode (CLI flag or environment variable)
    let debug_mode = cli.debug || std::env::var("DEBUG_MODE").is_ok();

    match cli.command {
        Some(Commands::Monitor) | None => {
            if debug_mode {
                tracing::info!("Running monitor in DEBUG mode (headless)");
                run_headless(&cli.rpc_url, &cli.db_path).await?;
            } else {
                run_tui(&cli.rpc_url, &cli.db_path).await?;
            }
        }
        Some(Commands::Sandwich {
            victim_amount,
            min_liquidity,
            max_price_impact,
            pool_count,
        }) => {
            println!("🥪 Enhanced REVM Sandwich Simulation");
            println!("====================================");
            run_sandwich_simulation(
                &cli.rpc_url,
                &cli.db_path,
                victim_amount,
                min_liquidity,
                max_price_impact,
                pool_count,
            )
            .await?;
        }
        Some(Commands::MultiDex { pools_per_protocol }) => {
            println!("🌐 Multi-DEX Comprehensive Analysis");
            println!("===================================");
            run_multi_dex_analysis(&cli.rpc_url, &cli.db_path, pools_per_protocol).await?;
        }
        Some(Commands::Integration) => {
            println!("🔧 Sandwich Integration Test");
            println!("============================");
            run_integration_test(&cli.rpc_url, &cli.db_path).await?;
        }
        Some(Commands::Bot {
            min_profit,
            max_gas_price,
            enable_flashbots,
            direct_mempool,
            private_key,
            simulation_only,
        }) => {
            println!("🤖 Continuous MEV Bot Runner");
            println!("============================");
            run_mev_bot(
                &cli.rpc_url,
                &cli.db_path,
                min_profit,
                max_gas_price,
                enable_flashbots,
                direct_mempool,
                private_key,
                simulation_only,
            )
            .await?;
        }
        Some(Commands::TestGas) => {
            println!("⛽ Gas Price Testing");
            println!("===================");
            test_gas_prices(&cli.rpc_url).await?;
        }
    }

    Ok(())
}

/// Setup logging to stdout for real-time visibility
fn setup_logging() {
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .with_target(true)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false);

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(stdout_layer);

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global default subscriber");
}

/// Run the TUI application
async fn run_tui(rpc_url: &str, db_path: &str) -> Result<()> {
    // Setup terminal
    if let Err(e) = enable_raw_mode() {
        return Err(anyhow::anyhow!("Failed to enable raw mode: {}", e));
    }

    let mut stdout = io::stdout();
    if let Err(e) = execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        DisableMouseCapture
    ) {
        return Err(anyhow::anyhow!("Failed to setup terminal: {}", e));
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(t) => t,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to create terminal: {}", e));
        }
    };

    // Get RPC URL from parameter
    let rpc_url = rpc_url.to_string();

    // Database path and chain ID (1 = Ethereum mainnet)
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
            return Err(anyhow::anyhow!("Failed to initialize application: {}", e));
        }
    };

    // Don't load pools on startup - they're loaded from database if available
    // Pool loading is very slow and blocks the TUI
    tracing::info!(
        "Application started with {} pools in database",
        app.pool_count
    );

    // Spawn background transaction updater FIRST (before pool loading)
    tracing::info!("Spawning background transaction updater task");
    let (_tx_updater, tx_updater_rx) =
        BackgroundTransactionUpdater::spawn(rpc_url.clone(), db_path.to_string(), chain_id);
    tracing::info!("Background transaction updater spawned successfully");

    // Spawn background pool loader (runs independently)
    tracing::info!("Spawning background pool loader task");
    let (_loader, pool_loader_rx) =
        BackgroundPoolLoader::spawn(rpc_url.clone(), db_path.to_string(), chain_id);
    app.is_loading_pools = true;
    app.pools_loading_progress = "Pool scan starting...".to_string();

    // Update status to show transaction monitoring is starting
    app.status = "Transaction monitoring starting...".to_string();
    app.needs_redraw = true;

    // Run the main loop
    run_app(&mut terminal, &mut app, pool_loader_rx, tx_updater_rx).await?;

    // Restore terminal
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    Ok(())
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
                    tracing::debug!(
                        "Pool loader progress: V2={} V3={} ({}%)",
                        v2_count,
                        v3_count,
                        progress_pct
                    );
                }
                PoolLoaderMessage::Complete(v2_count, v3_count, final_count) => {
                    app.v2_pools_found = v2_count;
                    app.v3_pools_found = v3_count;
                    app.pool_count = final_count;
                    app.pools_found_count = final_count;
                    app.pool_loading_progress_percent = 100;
                    app.is_loading_pools = false;
                    app.pools_loading_progress = format!(
                        "Loaded {} pools (V2: {}, V3: {})",
                        final_count, v2_count, v3_count
                    );
                    app.needs_redraw = true;
                    tracing::info!(
                        "Pool loader complete: V2={} V3={} Total={}",
                        v2_count,
                        v3_count,
                        final_count
                    );
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
                    tracing::info!(
                        "Main: Received {} transactions from background",
                        transactions.len()
                    );

                    // Add new transactions to existing set (don't replace, just add new ones)
                    let mut added_count = 0;
                    for tx in transactions {
                        // Check if this transaction is already in our list
                        if !app
                            .transactions
                            .iter()
                            .any(|existing| existing.hash == tx.hash)
                        {
                            app.transactions.push_back(tx);
                            added_count += 1;
                        }
                    }

                    // Limit total transactions
                    while app.transactions.len() > 2000 {
                        app.transactions.pop_front();
                    }

                    let pending_count = app.transactions.len();
                    app.status = format!(
                        "{} transactions targeting block #{} (+{} new)",
                        pending_count, app.next_block_number, added_count
                    );

                    app.last_update = Local::now().format("%H:%M:%S").to_string();
                    app.connection_healthy = true;
                    app.mark_cache_dirty();
                    app.cached_title = format!(
                        " Targeting Block #{} ({} txs) ",
                        app.next_block_number,
                        app.transactions.len()
                    );
                    app.needs_redraw = true;

                    tracing::info!(
                        "Main: Added {} new transactions, total: {} targeting block #{}",
                        added_count,
                        app.transactions.len(),
                        app.next_block_number
                    );
                }
                TransactionUpdateMessage::BlockChange {
                    new_block_number,
                    transactions,
                } => {
                    tracing::info!(
                        "Main: NEW BLOCK #{} with {} transactions",
                        new_block_number,
                        transactions.len()
                    );

                    // Clear all existing transactions - they were targeting the previous block
                    app.transactions.clear();
                    app.current_block_number = Some(new_block_number);
                    app.next_block_number = new_block_number + 1;

                    // Add all new transactions targeting the next block
                    for tx in transactions {
                        app.transactions.push_back(tx);
                    }

                    let pending_count = app.transactions.len();
                    app.status = format!(
                        "🆕 Block #{} mined! {} transactions now targeting block #{}",
                        new_block_number, pending_count, app.next_block_number
                    );

                    app.last_update = Local::now().format("%H:%M:%S").to_string();
                    app.connection_healthy = true;
                    app.mark_cache_dirty();
                    app.cached_title = format!(
                        " Targeting Block #{} ({} txs) ",
                        app.next_block_number,
                        app.transactions.len()
                    );
                    app.needs_redraw = true;

                    tracing::info!(
                        "Main: Block change processed - {} transactions targeting block #{}",
                        app.transactions.len(),
                        app.next_block_number
                    );
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
                                app.pools_loading_progress =
                                    "Manual pool refresh starting...".to_string();
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
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll_up(1);
                    }
                    MouseEventKind::ScrollDown => {
                        app.scroll_down(1, available_rows);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        // Background tasks handle all data updates - no blocking operations in main loop
    }

    Ok(())
}

/// Run in headless mode (no TUI, just logging)
async fn run_headless(rpc_url: &str, db_path: &str) -> Result<()> {
    // Get RPC URL from parameter
    let rpc_url = rpc_url.to_string();

    // Database path and chain ID
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
            return Err(e);
        }
    };

    // Pool loading - check if we need a comprehensive scan
    let force_refresh = std::env::var("FORCE_POOL_REFRESH").is_ok();
    let existing_pool_count = app.pool_count;

    if force_refresh {
        tracing::info!("FORCE_POOL_REFRESH enabled - running comprehensive pool scan");
        tracing::info!(
            "Starting complete blockchain scan for all DEX pools (existing: {})",
            existing_pool_count
        );
        tracing::info!("This will collect pools from The Graph Protocol and take ~30 seconds");

        if let Err(e) = app.sync_pools_comprehensive().await {
            tracing::error!("Failed to sync DEX pools comprehensively: {}", e);
        } else {
            tracing::info!(
                "✅ Comprehensive pool collection finished! Total pools: {}",
                app.pool_count
            );
        }
    } else if existing_pool_count < 100_000 {
        tracing::info!(
            "Pool count low ({}), syncing DEX pools comprehensively from The Graph",
            existing_pool_count
        );
        if let Err(e) = app.sync_pools_comprehensive().await {
            tracing::error!("Failed to sync DEX pools from node: {}", e);
        } else {
            tracing::info!("DEX pools synced from node. Count: {}", app.pool_count);
        }
    } else {
        tracing::info!(
            "Sufficient pools already in database ({})",
            existing_pool_count
        );
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
                    &tx.to
                        .as_ref()
                        .map(|a| &a[..std::cmp::min(10, a.len())])
                        .unwrap_or(&"N/A"),
                    tx.value_eth,
                    tx.gas_price_gwei
                );
            }
        }

        // Wait 5 seconds before next update
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

/// Run enhanced REVM sandwich simulation
async fn run_sandwich_simulation(
    rpc_url: &str,
    db_path: &str,
    victim_amount: f64,
    min_liquidity: f64,
    max_price_impact: f64,
    pool_count: usize,
) -> Result<()> {
    use alloy_primitives::U256;
    use eth_client::EthereumClient;

    println!("🔧 Initializing Enhanced REVM Simulator...");
    let eth_client = EthereumClient::new(rpc_url).await?;

    let criteria = PoolSelectionCriteria {
        min_liquidity_usd: min_liquidity,
        max_price_impact,
        min_volume_24h_usd: 50_000.0,
        supported_protocols: vec![
            "UniswapV2".to_string(),
            "UniswapV3".to_string(),
            "SushiSwap".to_string(),
            "Curve".to_string(),
        ],
        max_gas_price_gwei: 100.0,
        min_profit_threshold_eth: 0.001,
    };

    let mut simulator =
        EnhancedSandwichSimulator::new(db_path, rpc_url, eth_client, Some(criteria)).await?;

    println!("✅ Enhanced simulator initialized with pool database integration");

    // Find optimal sandwich targets
    println!("\n🎯 Finding Optimal Sandwich Targets");
    println!("Scanning 545k+ pools for high-quality opportunities...");
    let targets = simulator.find_optimal_targets(pool_count).await?;

    if targets.is_empty() {
        println!("❌ No suitable sandwich targets found");
        return Ok(());
    }

    println!("✅ Found {} optimal sandwich targets", targets.len());

    // Test different victim amounts
    let victim_eth = U256::from((victim_amount * 1e18) as u64);

    println!("\n🥪 Enhanced Sandwich Simulation");
    println!("Selected target:");
    println!("   Pool: {}", targets[0].pool.address);
    println!("   Protocol: {}", targets[0].pool.protocol);
    println!("   Liquidity: ${:.0}", targets[0].pool.total_liquidity_usd);

    println!("\n📊 Simulating victim trade: {} ETH", victim_amount);
    let result = simulator
        .simulate_sandwich_enhanced(&targets[0], victim_eth, 2.0)
        .await?;

    println!("📋 Simulation Results:");
    println!("   Success: {}", if result.success { "✅" } else { "❌" });
    println!(
        "   Gross Profit: {:.6} ETH (${:.2})",
        result.profit_eth,
        result.profit_eth * 2000.0
    );
    println!(
        "   Gas Cost: {:.6} ETH ({} gas)",
        result.gas_cost_eth, result.gas_used
    );
    println!(
        "   Net Profit: {:.6} ETH (${:.2})",
        result.net_profit_eth,
        result.net_profit_eth * 2000.0
    );
    println!("   Price Impact: {:.2}%", result.price_impact * 100.0);
    println!("   Risk Score: {}/100", result.risk_score);
    println!("   Execution Time: {}ms", result.execution_time_ms);

    Ok(())
}

/// Run multi-DEX comprehensive analysis
async fn run_multi_dex_analysis(
    rpc_url: &str,
    db_path: &str,
    pools_per_protocol: usize,
) -> Result<()> {
    use eth_client::EthereumClient;

    println!("🔧 Initializing Multi-DEX Analysis...");
    let eth_client = EthereumClient::new(rpc_url).await?;

    let criteria = PoolSelectionCriteria {
        min_liquidity_usd: 50_000.0,
        max_price_impact: 0.10,
        min_volume_24h_usd: 10_000.0,
        supported_protocols: vec![
            "UniswapV2".to_string(),
            "UniswapV3".to_string(),
            "SushiSwap".to_string(),
            "Curve".to_string(),
        ],
        max_gas_price_gwei: 100.0,
        min_profit_threshold_eth: 0.001,
    };

    let mut simulator =
        EnhancedSandwichSimulator::new(db_path, rpc_url, eth_client, Some(criteria)).await?;

    println!("✅ Simulator initialized with 545k+ pool database");

    // Run comprehensive analysis
    println!("\n📈 Running Multi-Pool Analysis...");
    let analysis = simulator
        .analyze_multiple_pools(pools_per_protocol * 4)
        .await?;

    println!("🎯 Multi-Pool Analysis Results:");
    println!("   Pools Analyzed: {}", analysis.total_pools_analyzed);
    println!(
        "   Successful Simulations: {}",
        analysis.successful_simulations
    );
    println!("   Success Rate: {:.1}%", analysis.success_rate * 100.0);
    println!(
        "   Total Potential Profit: {:.4} ETH (${:.2})",
        analysis.total_potential_profit_eth, analysis.total_potential_profit_usd
    );

    if !analysis.best_opportunities.is_empty() {
        println!("   🏆 Best Opportunity:");
        let best = &analysis.best_opportunities[0];
        println!("      Pool: {} ({})", best.pool_address, best.protocol);
        println!(
            "      Net Profit: {:.4} ETH (${:.2})",
            best.net_profit_eth,
            best.net_profit_eth * 2000.0
        );
        println!("      Risk Score: {}/100", best.risk_score);
    }

    Ok(())
}

/// Run basic sandwich integration test
async fn run_integration_test(rpc_url: &str, db_path: &str) -> Result<()> {
    use eth_client::EthereumClient;
    use sandwich_pool_integration::SandwichPoolIntegration;

    println!("🔧 Initializing Sandwich Pool Integration...");
    let eth_client = EthereumClient::new(rpc_url).await?;
    let integration = SandwichPoolIntegration::new(db_path, eth_client, rpc_url).await?;

    println!("✅ Integration initialized");

    // Get some sandwich targets for testing
    println!("\n🎯 Finding sandwich targets...");
    let candidates = integration.get_sandwich_candidates(50000.0).await?;

    if candidates.is_empty() {
        println!("❌ No sandwich candidates found");
        return Ok(());
    }

    println!("✅ Found {} sandwich candidates", candidates.len());

    for (i, candidate) in candidates.iter().take(5).enumerate() {
        println!(
            "{}. Pool: {} | Protocol: {} | Liquidity: ${:.0} | Impact: {:.2}%",
            i + 1,
            candidate.pool_address,
            candidate.protocol,
            candidate.total_liquidity_usd,
            candidate.price_impact_1_eth * 100.0
        );
    }

    Ok(())
}

/// Run continuous MEV bot with real-time mempool monitoring
async fn run_mev_bot(
    rpc_url: &str,
    db_path: &str,
    min_profit: f64,
    max_gas_price_gwei: u64,
    enable_flashbots: bool,
    direct_mempool: bool,
    private_key: Option<String>,
    simulation_only: bool,
) -> Result<()> {
    use eth_client::EthereumClient;
    use pool_db::PoolDatabase;

    println!("🤖 Initializing MEV Bot Runner...");

    // Initialize components
    let eth_client = EthereumClient::new(rpc_url).await?;
    let pool_db = PoolDatabase::new(db_path)?;

    // Load pool data
    println!("📊 Loading pool database...");
    let total_pools = pool_db.get_total_pools().await?;
    println!("✅ Pool database loaded: {} pools", total_pools);

    if total_pools < 1000 {
        println!("⚠️  Warning: Low pool count may limit opportunity detection");
    }

    // Configure bot
    let config = BotConfig {
        min_value_usd: 1.0, // $1 minimum for testing
        min_gas_price_gwei: 5.0,
        max_gas_price_gwei: max_gas_price_gwei as f64,
        confidence_threshold: 0.3, // Lower threshold for testing
        enable_websocket: true,
        max_gas_price: U256::from(max_gas_price_gwei * 1_000_000_000),
        min_profit_threshold: min_profit,
        max_concurrent_bundles: 5,
        bundle_timeout_seconds: 15,
        stats_interval_seconds: 60, // Default 1 minute
        max_opportunities_per_block: 3,
        enable_flashbots,
        direct_mempool,
        aggressive_gas: false, // Default conservative
        signing_key: private_key.clone(),
        flashbots_api_key: None, // Not supported in simplified version
    };

    // Determine execution method
    let execution_mode = if enable_flashbots {
        "Flashbots Bundle (Simulation Only)"
    } else if direct_mempool && private_key.is_some() {
        "Direct Mempool"
    } else if simulation_only {
        "Simulation Only"
    } else {
        "Simulation Only"
    };

    println!("⚙️  Bot Configuration:");
    println!("   • Min Profit: {:.4} ETH", config.min_profit_threshold);
    println!("   • Max Gas Price: {} gwei", max_gas_price_gwei);
    println!("   • Execution Mode: {}", execution_mode);
    println!("   • Stats Interval: {}s", config.stats_interval_seconds);

    if execution_mode == "Simulation Only" {
        println!("📝 Running in SIMULATION mode - no actual transactions will be sent");
    } else {
        println!("⚠️  LIVE MODE - Real transactions will be submitted!");
    }

    // Create and start MEV bot
    let mut bot = MevBotRunner::new(
        config,
        eth_client,
        pool_db,
        rpc_url.to_string(),
        db_path.to_string(),
    )
    .await?;

    println!("\n🚀 Starting MEV bot runner...");
    println!("   Press Ctrl+C to stop gracefully\n");

    // Start the bot (this will run until Ctrl+C)
    bot.start().await?;

    Ok(())
}

/// Test gas price fetching and show comparison between old and new estimates
async fn test_gas_prices(rpc_url: &str) -> Result<()> {
    use eth_client::EthereumClient;

    tracing::info!("Creating Ethereum client for gas price testing...");
    let eth_client = EthereumClient::new(rpc_url).await?;

    eth_client.test_gas_prices().await?;

    Ok(())
}
