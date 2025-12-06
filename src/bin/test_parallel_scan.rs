//! Test binary to compare parallel vs sequential pool scanning performance

use anyhow::Result;
use ethereum_transaction_pool_monitor::{pool_fetcher::PoolFetcher, pool_db::PoolDatabase};
use std::time::Instant;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());

    println!("🚀 Testing Parallel vs Sequential Pool Scanning");
    println!("📡 RPC URL: {}", rpc_url);

    // Create test databases
    let parallel_db_path = "test_parallel.db";
    let sequential_db_path = "test_sequential.db";

    // Clean up old test databases
    let _ = std::fs::remove_file(parallel_db_path);
    let _ = std::fs::remove_file(sequential_db_path);

    let pool_fetcher = PoolFetcher::new(&rpc_url);

    // Test Parallel Scanning
    println!("\n⚡ Testing PARALLEL scanning...");
    let parallel_start = Instant::now();
    let parallel_db = PoolDatabase::new(parallel_db_path)?;
    
    let parallel_count = pool_fetcher
        .fetch_pools_parallel(&parallel_db, 1, None)
        .await?;
    
    let parallel_duration = parallel_start.elapsed();
    
    println!("✅ Parallel scanning complete!");
    println!("   - Pools found: {}", parallel_count);
    println!("   - Time taken: {:?}", parallel_duration);

    // Test Sequential Scanning (limit to last 1M blocks for fair comparison)
    println!("\n🐌 Testing SEQUENTIAL scanning (V2 only for comparison)...");
    let sequential_start = Instant::now();
    let sequential_db = PoolDatabase::new(sequential_db_path)?;
    
    let v2_count = pool_fetcher
        .fetch_uniswap_v2_pools(&sequential_db, 1)
        .await?;
    
    let sequential_duration = sequential_start.elapsed();
    
    println!("✅ Sequential V2 scanning complete!");
    println!("   - V2 pools found: {}", v2_count);
    println!("   - Time taken: {:?}", sequential_duration);

    // Performance comparison
    println!("\n📊 PERFORMANCE COMPARISON");
    println!("┌─────────────────┬─────────────┬─────────────┬─────────────┐");
    println!("│ Method          │ Pools Found │ Time Taken  │ Speed       │");
    println!("├─────────────────┼─────────────┼─────────────┼─────────────┤");
    println!("│ Parallel        │ {:>11} │ {:>11.1?} │ {:>11.1} │", 
             parallel_count, 
             parallel_duration, 
             parallel_count as f64 / parallel_duration.as_secs_f64());
    println!("│ Sequential (V2) │ {:>11} │ {:>11.1?} │ {:>11.1} │", 
             v2_count, 
             sequential_duration, 
             v2_count as f64 / sequential_duration.as_secs_f64());
    println!("└─────────────────┴─────────────┴─────────────┴─────────────┘");

    if parallel_duration < sequential_duration {
        let speedup = sequential_duration.as_secs_f64() / parallel_duration.as_secs_f64();
        println!("🎉 Parallel scanning is {:.1}x faster!", speedup);
    } else {
        println!("⚠️ Sequential scanning was faster this time.");
    }

    // Verify database counts
    let parallel_v2 = parallel_db.get_pool_count_by_protocol("UniswapV2").unwrap_or(0);
    let parallel_v3 = parallel_db.get_pool_count_by_protocol("UniswapV3").unwrap_or(0);
    
    println!("\n🔍 DETAILED RESULTS");
    println!("Parallel scanning breakdown:");
    println!("   - V2 pools: {}", parallel_v2);
    println!("   - V3 pools: {}", parallel_v3);
    println!("   - Total: {}", parallel_v2 + parallel_v3);

    // Clean up test files
    let _ = std::fs::remove_file(parallel_db_path);
    let _ = std::fs::remove_file(sequential_db_path);

    println!("\n✅ Performance test complete!");
    Ok(())
}