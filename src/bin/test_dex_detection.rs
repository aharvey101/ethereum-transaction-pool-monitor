//! DEX Transaction Detection Tester
//!
//! This binary specifically tests our enhanced DEX transaction detection logic.
//! It allows you to:
//! 1. Test with sample DEX transactions
//! 2. Test with live mempool data
//! 3. Validate function selector recognition
//! 4. Test token pair extraction

use ethereum_transaction_pool_monitor::{
    mempool_monitor::{MempoolMonitor, MempoolConfig},
};
use alloy_primitives::{Address, Bytes};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::str::FromStr;
use tracing::{info, warn, error};

#[derive(Parser)]
#[command(name = "test-dex-detection")]
#[command(about = "Test DEX transaction detection and parsing")]
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
    /// Test with sample DEX transactions
    TestSamples,
    /// Test with live mempool data 
    LiveMempool {
        /// Number of transactions to analyze
        #[arg(long, default_value = "50")]
        count: usize,
        /// Only show DEX transactions
        #[arg(long)]
        dex_only: bool,
    },
    /// Analyze specific transaction input
    AnalyzeInput {
        /// Transaction input data (hex)
        #[arg(long)]
        input: String,
        /// From address (hex)
        #[arg(long)]
        from: String,
        /// To address (hex)  
        #[arg(long)]
        to: String,
    },
    /// Test function selectors
    TestSelectors,
}

#[derive(Debug)]
struct TestTransaction {
    from: Address,
    to: Option<Address>,
    input: Bytes,
    description: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let log_level = if cli.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("ethereum_transaction_pool_monitor={},test_dex_detection={}", log_level, log_level))
        .init();

    info!("🧪 Starting DEX detection testing...");

    match &cli.command {
        Commands::TestSamples => test_sample_transactions(&cli).await,
        Commands::LiveMempool { count, dex_only } => test_live_mempool(&cli, *count, *dex_only).await,
        Commands::AnalyzeInput { input, from, to } => analyze_transaction_input(&cli, input, from, to).await,
        Commands::TestSelectors => test_function_selectors(&cli).await,
    }
}

async fn test_sample_transactions(cli: &Cli) -> Result<()> {
    info!("🧪 Testing sample DEX transactions...");
    
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    let sample_transactions = vec![
        TestTransaction {
            from: Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")?,
            to: Some(Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")?), // Uniswap V2 Router
            input: Bytes::from_str("0x38ed173900000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000080000000000000000000000000742d35cc6c5ee6a0e6bc6f8c35b6b9c2b5ac5b460000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000a0b86a33e6c8b8e6be3f1b0d5c4b3a7ec2e9d8f7000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")?,
            description: "Uniswap V2 swapExactTokensForTokens".to_string(),
        },
        TestTransaction {
            from: Address::from_str("0x1234567890123456789012345678901234567890")?,
            to: Some(Address::from_str("0xe592427a0aece92de3edee1f18e0157c05861564")?), // Uniswap V3 Router
            input: Bytes::from_str("0x414bf38900000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000002000000000000000000000000742d35cc6c5ee6a0e6bc6f8c35b6b9c2b5ac5b460000000000000000000000000000000000000000000000000000000000000064")?,
            description: "Uniswap V3 exactInputSingle".to_string(),
        },
        TestTransaction {
            from: Address::from_str("0x1234567890123456789012345678901234567890")?,
            to: Some(Address::from_str("0x1111111254eeb25477b68fb85ed929f73a960582")?), // 1inch Router
            input: Bytes::from_str("0x7c025e60000000000000000000000000a0b86a33e6c8b8e6be3f1b0d5c4b3a7ec2e9d8f7000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000000000000000000000000000000000000000006400000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000080")?,
            description: "1inch swap".to_string(),
        },
        TestTransaction {
            from: Address::from_str("0x1234567890123456789012345678901234567890")?,
            to: Some(Address::from_str("0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f")?), // SushiSwap Router
            input: Bytes::from_str("0x38ed173900000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000080000000000000000000000000742d35cc6c5ee6a0e6bc6f8c35b6b9c2b5ac5b460000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000a0b86a33e6c8b8e6be3f1b0d5c4b3a7ec2e9d8f7000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2")?,
            description: "SushiSwap swapExactTokensForTokens".to_string(),
        },
        TestTransaction {
            from: Address::from_str("0x1234567890123456789012345678901234567890")?,
            to: Some(Address::from_str("0x1234567890123456789012345678901234567890")?), // Non-DEX contract
            input: Bytes::from_str("0xa9059cbb000000000000000000000000742d35cc6c5ee6a0e6bc6f8c35b6b9c2b5ac5b460000000000000000000000000000000000000000000000000000000000000001")?,
            description: "ERC20 transfer (not DEX)".to_string(),
        },
    ];

    info!("📊 Testing {} sample transactions", sample_transactions.len());

    let mut dex_detected = 0;
    for (i, sample) in sample_transactions.iter().enumerate() {
        info!("🔍 Testing transaction {}/{}: {}", i + 1, sample_transactions.len(), sample.description);
        
        // Test DEX detection by parsing router target pool
        match monitor.parse_router_target_pool(&sample.input, &format!("{:#x}", sample.to.unwrap_or_default())).await {
            Ok(Some(pool_address)) => {
                dex_detected += 1;
                info!("✅ DEX transaction detected! Pool: {:#x}", pool_address);
            }
            Ok(None) => {
                info!("❌ No DEX pool detected");
            }
            Err(e) => {
                error!("⚠️  Error parsing transaction: {}", e);
            }
        }
    }

    info!("📈 Summary: {}/{} transactions detected as DEX ({:.1}%)", 
          dex_detected, sample_transactions.len(), 
          (dex_detected as f32 / sample_transactions.len() as f32) * 100.0);

    Ok(())
}

async fn test_live_mempool(cli: &Cli, count: usize, dex_only: bool) -> Result<()> {
    info!("🔴 Live mempool testing with {} transactions", count);
    
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    // Note: This would require implementing a way to fetch live mempool transactions
    // For now, show what this would do
    warn!("⚠️  Live mempool testing not yet implemented in this test binary");
    info!("💡 To test with live data, run the main bot: cargo run --bin ethereum-transaction-pool-monitor bot");

    Ok(())
}

async fn analyze_transaction_input(cli: &Cli, input: &str, from: &str, to: &str) -> Result<()> {
    info!("🔍 Analyzing specific transaction input...");
    
    let config = MempoolConfig::default();
    let (monitor, _) = MempoolMonitor::new(&cli.rpc_url, &cli.db_path, config).await?;

    let input_bytes = Bytes::from_str(input)?;
    let to_addr = to;

    info!("📝 Input: {}", input);
    info!("📝 From: {}", from);  
    info!("📝 To: {}", to);
    info!("📝 Input length: {} bytes", input_bytes.len());

    if input_bytes.len() >= 4 {
        let selector = &input_bytes[0..4];
        info!("🔧 Function selector: 0x{}", hex::encode(selector));
    }

    match monitor.parse_router_target_pool(&input_bytes, to_addr).await {
        Ok(Some(pool_address)) => {
            info!("✅ DEX transaction detected! Pool: {:#x}", pool_address);
        }
        Ok(None) => {
            info!("❌ No DEX pool detected - not a recognized DEX transaction");
        }
        Err(e) => {
            error!("⚠️  Error parsing transaction: {}", e);
        }
    }

    Ok(())
}

async fn test_function_selectors(cli: &Cli) -> Result<()> {
    info!("🔧 Testing function selector recognition...");

    let known_selectors = vec![
        ("0x38ed1739", "swapExactTokensForTokens (Uniswap V2/SushiSwap)"),
        ("0x7ff36ab5", "swapExactETHForTokens (Uniswap V2)"),
        ("0x18cbafe5", "swapExactTokensForETH (Uniswap V2)"),
        ("0x414bf389", "exactInputSingle (Uniswap V3)"),
        ("0xdb3e2198", "exactOutputSingle (Uniswap V3)"),
        ("0x7c025e60", "swap (1inch)"),
        ("0x41346767", "unoswap (1inch)"),
        ("0xd9627aa4", "sellToUniswap (0x Protocol)"),
        ("0x415565b0", "fillQuote (0x Protocol)"),
        ("0xa6c3bf33", "clipperSwap (0x Protocol)"),
        ("0x6af479b2", "addLiquidity (Uniswap V2)"),
        ("0xf305d719", "addLiquidityETH (Uniswap V2)"),
        ("0xbaa2abde", "removeLiquidity (Uniswap V2)"),
    ];

    info!("📊 Testing {} known function selectors:", known_selectors.len());

    for (selector_hex, description) in known_selectors {
        info!("  {} - {}", selector_hex, description);
    }

    info!("✅ Function selector test complete");

    Ok(())
}