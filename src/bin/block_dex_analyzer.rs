//! Block DEX Transaction Analyzer
//!
//! This tool analyzes the last N blocks from Ethereum mainnet to find and analyze
//! major DEX transactions. It helps validate our DEX detection pipeline against
//! real recent blockchain data.
//!
//! Features:
//! - Fetch recent blocks from Ethereum node
//! - Identify DEX transactions using our detection pipeline
//! - Provide detailed statistics on transaction types
//! - Show function selectors and routers used
//! - Calculate DEX transaction density per block

use ethereum_transaction_pool_monitor::{
    mempool_monitor::{MempoolMonitor, MempoolConfig},
    eth_client::EthereumClient,
    pool_db::PoolDatabase,
};
use alloy_primitives::{Address, Bytes};
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{info, warn, error, debug};

#[derive(Parser)]
#[command(name = "block-dex-analyzer")]
#[command(about = "Analyze recent blocks for major DEX transactions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// RPC URL for Ethereum node
    #[arg(long, default_value = "http://192.168.0.14:8545")]
    rpc_url: String,

    /// Database path for pool storage
    #[arg(long, default_value = "./database.sqlite3")]
    db_path: String,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze the last N blocks for DEX transactions
    AnalyzeBlocks {
        /// Number of blocks to analyze (default: 100)
        #[arg(long, default_value = "100")]
        count: u64,
        /// Show detailed transaction info for each DEX transaction found
        #[arg(long)]
        detailed: bool,
        /// Only show blocks with DEX transactions
        #[arg(long)]
        dex_only: bool,
    },
    /// Analyze a specific block by number
    AnalyzeBlock {
        /// Block number to analyze
        #[arg(long)]
        number: u64,
        /// Show detailed transaction info
        #[arg(long)]
        detailed: bool,
    },
    /// Show live monitoring of new blocks as they arrive
    LiveMonitor {
        /// Show detailed transaction info
        #[arg(long)]
        detailed: bool,
    },
}

#[derive(Debug, Clone)]
struct BlockInfo {
    number: u64,
    hash: String,
    timestamp: u64,
    transaction_count: usize,
}

#[derive(Debug, Clone)]
struct DexTransactionInfo {
    hash: String,
    from: String,
    to: String,
    function_selector: String,
    router_name: String,
    pool_address: Option<String>,
    gas_used: Option<u64>,
    gas_price: Option<u64>,
}

#[derive(Debug, Default)]
struct AnalysisStats {
    total_blocks: u64,
    total_transactions: u64,
    dex_transactions: u64,
    blocks_with_dex: u64,
    router_counts: HashMap<String, u64>,
    function_counts: HashMap<String, u64>,
    unique_pools: std::collections::HashSet<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let log_level = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("ethereum_transaction_pool_monitor={},block_dex_analyzer={}", log_level, log_level))
        .init();

    info!("🔍 Starting Block DEX Transaction Analyzer...");

    match &cli.command {
        Commands::AnalyzeBlocks { count, detailed, dex_only } => {
            analyze_recent_blocks(&cli, *count, *detailed, *dex_only).await
        },
        Commands::AnalyzeBlock { number, detailed } => {
            analyze_single_block(&cli, *number, *detailed).await
        },
        Commands::LiveMonitor { detailed } => {
            live_monitor_blocks(&cli, *detailed).await
        },
    }
}

async fn analyze_recent_blocks(cli: &Cli, count: u64, detailed: bool, dex_only: bool) -> Result<()> {
    info!("📊 Analyzing last {} blocks for DEX transactions...", count);
    
    // Initialize components
    let eth_client = EthereumClient::new(&cli.rpc_url).await?;
    let pool_db = PoolDatabase::new(&cli.db_path)?;
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    // Get latest block number
    let latest_block = eth_client.get_block_number().await?;
    info!("🔗 Latest block: {}", latest_block);

    let start_block = latest_block.saturating_sub(count - 1);
    info!("📈 Analyzing blocks {} to {} ({} blocks)", start_block, latest_block, count);

    let mut stats = AnalysisStats::default();
    let mut blocks_analyzed = 0u64;

    for block_num in start_block..=latest_block {
        if let Ok(Some(block_info)) = get_block_info(&eth_client, block_num).await {
            blocks_analyzed += 1;
            stats.total_blocks += 1;
            stats.total_transactions += block_info.transaction_count as u64;

            info!("🔍 Analyzing block {} ({} transactions)...", block_num, block_info.transaction_count);

            let mut block_dex_count = 0;
            let mut dex_transactions = Vec::new();

            // Analyze each transaction in the block
            if let Ok(transactions) = get_block_transactions(&eth_client, block_num).await {
                for tx in transactions {
                    if let Some(dex_info) = analyze_transaction(&monitor, &pool_db, &tx).await {
                        block_dex_count += 1;
                        stats.dex_transactions += 1;
                        dex_transactions.push(dex_info.clone());

                        // Update statistics
                        *stats.router_counts.entry(dex_info.router_name.clone()).or_insert(0) += 1;
                        *stats.function_counts.entry(dex_info.function_selector.clone()).or_insert(0) += 1;
                        if let Some(pool) = &dex_info.pool_address {
                            stats.unique_pools.insert(pool.clone());
                        }

                        if detailed {
                            info!("  🟢 DEX TX: {} -> {} via {} ({})", 
                                  &dex_info.hash[..10], 
                                  dex_info.router_name, 
                                  dex_info.function_selector,
                                  dex_info.pool_address.as_deref().unwrap_or("unknown pool"));
                        }
                    }
                }
            }

            if block_dex_count > 0 {
                stats.blocks_with_dex += 1;
            }

            // Show block summary
            if !dex_only || block_dex_count > 0 {
                let dex_percentage = if block_info.transaction_count > 0 {
                    (block_dex_count as f32 / block_info.transaction_count as f32) * 100.0
                } else {
                    0.0
                };

                info!("📊 Block {}: {}/{} DEX transactions ({:.1}%)", 
                      block_num, block_dex_count, block_info.transaction_count, dex_percentage);
            }
        } else {
            warn!("⚠️  Failed to fetch block {}", block_num);
        }

        // Show progress every 10 blocks
        if blocks_analyzed % 10 == 0 {
            info!("📈 Progress: {}/{} blocks analyzed", blocks_analyzed, count);
        }
    }

    // Print final statistics
    print_analysis_summary(&stats);

    Ok(())
}

async fn analyze_single_block(cli: &Cli, block_number: u64, detailed: bool) -> Result<()> {
    info!("🔍 Analyzing block {} for DEX transactions...", block_number);
    
    let eth_client = EthereumClient::new(&cli.rpc_url).await?;
    let pool_db = PoolDatabase::new(&cli.db_path)?;
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    if let Ok(Some(block_info)) = get_block_info(&eth_client, block_number).await {
        info!("📊 Block {}: {} transactions", block_number, block_info.transaction_count);

        let mut dex_count = 0;
        if let Ok(transactions) = get_block_transactions(&eth_client, block_number).await {
            for tx in transactions {
                if let Some(dex_info) = analyze_transaction(&monitor, &pool_db, &tx).await {
                    dex_count += 1;
                    
                    if detailed {
                        info!("🟢 DEX Transaction:");
                        info!("  Hash: {}", dex_info.hash);
                        info!("  Router: {}", dex_info.router_name);
                        info!("  Function: {}", dex_info.function_selector);
                        if let Some(pool) = &dex_info.pool_address {
                            info!("  Pool: {}", pool);
                        }
                        if let Some(gas_used) = dex_info.gas_used {
                            info!("  Gas Used: {}", gas_used);
                        }
                    } else {
                        info!("🟢 DEX TX: {} via {} ({})", 
                              &dex_info.hash[..10], 
                              dex_info.router_name, 
                              dex_info.function_selector);
                    }
                }
            }
        }

        let dex_percentage = if block_info.transaction_count > 0 {
            (dex_count as f32 / block_info.transaction_count as f32) * 100.0
        } else {
            0.0
        };

        info!("📈 Summary: {}/{} DEX transactions ({:.1}%)", 
              dex_count, block_info.transaction_count, dex_percentage);
    } else {
        error!("❌ Failed to fetch block {}", block_number);
    }

    Ok(())
}

async fn live_monitor_blocks(cli: &Cli, detailed: bool) -> Result<()> {
    info!("🔴 Starting live block monitoring for DEX transactions...");
    
    let eth_client = EthereumClient::new(&cli.rpc_url).await?;
    let pool_db = PoolDatabase::new(&cli.db_path)?;
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    let mut last_block = eth_client.get_block_number().await?;
    info!("🔗 Starting from block: {}", last_block);

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(12)).await; // ~Ethereum block time

        match eth_client.get_block_number().await {
            Ok(current_block) => {
                if current_block > last_block {
                    for block_num in (last_block + 1)..=current_block {
                        analyze_single_block(cli, block_num, detailed).await.unwrap_or_else(|e| {
                            error!("Error analyzing block {}: {}", block_num, e);
                        });
                    }
                    last_block = current_block;
                }
            }
            Err(e) => {
                error!("Error fetching latest block: {}", e);
            }
        }
    }
}

// Helper functions

async fn get_block_info(eth_client: &EthereumClient, block_number: u64) -> Result<Option<BlockInfo>> {
    if let Some(block) = eth_client.get_block_info(block_number).await? {
        let number = block_number;
        let hash = block["hash"].as_str().unwrap_or("unknown").to_string();
        let timestamp = block["timestamp"].as_str()
            .and_then(|t| u64::from_str_radix(&t[2..], 16).ok())
            .unwrap_or(0);
        
        // Get transaction count - either from transactions array or separate field
        let transaction_count = if let Some(transactions) = block.get("transactions").and_then(|t| t.as_array()) {
            transactions.len()
        } else {
            // Fallback to getting transactions separately
            match eth_client.get_block_transactions(block_number).await {
                Ok(transactions) => transactions.len(),
                Err(_) => 0,
            }
        };

        return Ok(Some(BlockInfo {
            number,
            hash,
            timestamp,
            transaction_count,
        }));
    }
    Ok(None)
}

async fn get_block_transactions(eth_client: &EthereumClient, block_number: u64) -> Result<Vec<serde_json::Value>> {
    eth_client.get_block_transactions(block_number).await
}

async fn analyze_transaction(
    monitor: &MempoolMonitor, 
    pool_db: &PoolDatabase, 
    tx: &Value
) -> Option<DexTransactionInfo> {
    // Extract transaction fields
    let hash = tx["hash"].as_str()?.to_string();
    let from = tx["from"].as_str()?.to_string();
    let to = tx["to"].as_str()?.to_string();
    let input = tx["input"].as_str()?;

    // Check if it's a DEX router
    if !pool_db.is_dex_router(&to) {
        return None;
    }

    // Parse input data
    if let Ok(input_bytes) = Bytes::from_str(input) {
        if input_bytes.len() >= 4 {
            let selector = hex::encode(&input_bytes[0..4]);
            let function_selector = format!("0x{}", selector);

            // Get router name
            let router_name = get_router_name(&to);

            // Try to extract pool information
            let pool_address = monitor.parse_router_target_pool(&input_bytes, &to)
                .await
                .ok()
                .flatten()
                .map(|addr| format!("{:#x}", addr));

            return Some(DexTransactionInfo {
                hash,
                from,
                to,
                function_selector,
                router_name,
                pool_address,
                gas_used: tx["gas"].as_str().and_then(|g| u64::from_str_radix(&g[2..], 16).ok()),
                gas_price: tx["gasPrice"].as_str().and_then(|g| u64::from_str_radix(&g[2..], 16).ok()),
            });
        }
    }

    None
}

fn get_router_name(address: &str) -> String {
    match address.to_lowercase().as_str() {
        "0x7a250d5630b4cf539739df2c5dacb4c659f2488d" => "Uniswap V2".to_string(),
        "0xe592427a0aece92de3edee1f18e0157c05861564" => "Uniswap V3".to_string(),
        "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45" => "Uniswap V3 Router02".to_string(),
        "0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f" => "SushiSwap".to_string(),
        "0x1111111254eeb25477b68fb85ed929f73a960582" => "1inch V5".to_string(),
        "0xdef1c0ded9bec7f1a1670819833240f027b25eff" => "0x Protocol".to_string(),
        "0xba12222222228d8ba445958a75a0704d566bf2c8" => "Balancer V2".to_string(),
        _ => format!("Unknown ({})", &address[..10]),
    }
}

fn print_analysis_summary(stats: &AnalysisStats) {
    info!("");
    info!("📊 ===== ANALYSIS SUMMARY =====");
    info!("🔢 Total Blocks Analyzed: {}", stats.total_blocks);
    info!("🔢 Total Transactions: {}", stats.total_transactions);
    info!("🟢 DEX Transactions: {}", stats.dex_transactions);
    info!("📈 Blocks with DEX: {}", stats.blocks_with_dex);
    
    if stats.total_transactions > 0 {
        let dex_percentage = (stats.dex_transactions as f32 / stats.total_transactions as f32) * 100.0;
        info!("📊 DEX Transaction Rate: {:.2}%", dex_percentage);
    }

    if stats.total_blocks > 0 {
        let blocks_with_dex_percentage = (stats.blocks_with_dex as f32 / stats.total_blocks as f32) * 100.0;
        info!("📊 Blocks with DEX: {:.1}%", blocks_with_dex_percentage);
    }

    info!("🏊 Unique Pools Found: {}", stats.unique_pools.len());

    info!("");
    info!("🔧 Top Routers:");
    let mut router_vec: Vec<(&String, &u64)> = stats.router_counts.iter().collect();
    router_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (router, count) in router_vec.iter().take(10) {
        info!("  {} - {} transactions", router, count);
    }

    info!("");
    info!("🔧 Top Function Selectors:");
    let mut function_vec: Vec<(&String, &u64)> = stats.function_counts.iter().collect();
    function_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (function, count) in function_vec.iter().take(10) {
        let function_name = get_function_name(function);
        info!("  {} ({}) - {} times", function, function_name, count);
    }
}

fn get_function_name(selector: &str) -> &'static str {
    match selector {
        "0x38ed1739" => "swapExactTokensForTokens",
        "0x7ff36ab5" => "swapExactETHForTokens",
        "0x18cbafe5" => "swapExactTokensForETH",
        "0x414bf389" => "exactInputSingle",
        "0xdb3e2198" => "exactOutputSingle",
        "0x7c025e60" => "swap (1inch)",
        "0x41346767" => "unoswap (1inch)",
        "0xd9627aa4" => "sellToUniswap",
        "0x415565b0" => "fillQuote",
        "0xa6c3bf33" => "clipperSwap",
        _ => "unknown",
    }
}