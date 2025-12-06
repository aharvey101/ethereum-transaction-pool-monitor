//! Multi-DEX Pool Detection Test
//! 
//! This binary tests the new multi-DEX pool scanning functionality
//! to see how many pools each protocol has found.

use anyhow::Result;
use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
use ethereum_transaction_pool_monitor::pool_fetcher::PoolFetcher;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🚀 Multi-DEX Pool Detection Test");
    println!("=================================");
    
    let rpc_url = env::var("ETH_RPC_URL")
        .unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "test_multi_dex.db";
    
    println!("📡 RPC URL: {}", rpc_url);
    println!("💾 Database: {}", db_path);
    
    // Remove old database for fresh test
    let _ = std::fs::remove_file(db_path);
    
    // Initialize database
    let pool_db = PoolDatabase::new(db_path)?;
    let fetcher = PoolFetcher::new(&rpc_url);
    
    println!("\n🔍 Starting Multi-DEX Pool Scan...");
    println!("This will scan:");
    println!("• UniswapV2 pools (from block 10,000,835)");
    println!("• UniswapV3 pools (from block 12,369,739)");  
    println!("• SushiSwap pools (from block 10,794,229)");
    println!("• PancakeSwap pools (from block 15,614,590)");
    println!("• ShibaSwap pools (from block 12,771,744)");
    println!("• FraxSwap pools (from block 15,463,108)");
    println!("• Curve Stableswap-NG pools (from block 17,000,000)");
    println!("• Curve Twocrypto-NG pools (from block 18,000,000)");
    println!();
    
    // Run parallel multi-DEX scan
    let total_pools = fetcher.fetch_pools_parallel(&pool_db, 1, None).await?;
    
    println!("\n📊 Multi-DEX Scan Results");
    println!("=========================");
    
    // Get counts by protocol
    let protocol_counts = pool_db.get_all_protocol_counts()?;
    
    for (protocol, count) in &protocol_counts {
        println!("🔹 {}: {} pools", protocol, count);
    }
    
    println!("🔹 Total: {} pools", total_pools);
    
    // Compare to previous results
    println!("\n📈 Comparison with Previous Uniswap-only Results:");
    println!("• Previous (Uniswap only): 11,611 pools");
    println!("• New (Multi-DEX): {} pools", total_pools);
    if total_pools > 11611 {
        println!("• Improvement: +{} pools (+{:.1}%)", 
                total_pools - 11611, 
                ((total_pools as f64 - 11611.0) / 11611.0) * 100.0);
    }
    
    println!("\n✅ Multi-DEX scan complete!");
    
    Ok(())
}