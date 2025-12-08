//! Comprehensive Pool Database Populator
//! 
//! This binary populates the main dex_pools.db database with pools from all major DEXs
//! using the optimized 100k block chunk scanning approach.

use anyhow::Result;
use ethereum_transaction_pool_monitor::{pool_fetcher::PoolFetcher, pool_db::PoolDatabase};
use std::sync::Arc;
use std::sync::Mutex;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());

    println!("🚀 Comprehensive DEX Pool Database Population");
    println!("📡 RPC URL: {}", rpc_url);

    let db_path = "dex_pools.db";
    let pool_db = PoolDatabase::new(db_path)?;
    let pool_fetcher = PoolFetcher::new(&rpc_url);

    // Create progress callback for real-time updates
    let progress_callback = Arc::new(Mutex::new(Box::new(|msg: String, count: u32| {
        println!("🔄 Progress: {} (Pools found: {})", msg, count);
    }) as Box<dyn Fn(String, u32) + Send>));

    println!("📊 Current database status:");
    match pool_db.pool_count() {
        Ok(count) => println!("   - Total pools: {}", count),
        Err(e) => println!("   - Error counting pools: {}", e),
    }

    // Get protocol-specific counts
    let v2_count = pool_db.get_pool_count_by_protocol("UniswapV2").unwrap_or(0);
    let v3_count = pool_db.get_pool_count_by_protocol("UniswapV3").unwrap_or(0);
    let sushi_count = pool_db.get_pool_count_by_protocol("SushiSwap").unwrap_or(0);
    let pancake_count = pool_db.get_pool_count_by_protocol("PancakeSwap").unwrap_or(0);
    
    println!("   - UniswapV2: {}", v2_count);
    println!("   - UniswapV3: {}", v3_count);
    println!("   - SushiSwap: {}", sushi_count);
    println!("   - PancakeSwap: {}", pancake_count);

    println!("\n🚀 Starting comprehensive multi-DEX parallel scanning...");
    println!("⚙️  Using 100k block chunks optimized for local node");
    
    let total_pools = pool_fetcher
        .fetch_pools_parallel(&pool_db, 1, Some(progress_callback))
        .await?;

    println!("\n✅ Comprehensive scan complete!");
    println!("   - Total pools found in this scan: {}", total_pools);

    // Show final database status
    println!("\n📊 Final database status:");
    match pool_db.pool_count() {
        Ok(count) => println!("   - Total pools: {}", count),
        Err(e) => println!("   - Error counting pools: {}", e),
    }

    let final_v2 = pool_db.get_pool_count_by_protocol("UniswapV2").unwrap_or(0);
    let final_v3 = pool_db.get_pool_count_by_protocol("UniswapV3").unwrap_or(0);
    let final_sushi = pool_db.get_pool_count_by_protocol("SushiSwap").unwrap_or(0);
    let final_pancake = pool_db.get_pool_count_by_protocol("PancakeSwap").unwrap_or(0);
    let final_shibaswap = pool_db.get_pool_count_by_protocol("ShibaSwap").unwrap_or(0);
    let final_fraxswap = pool_db.get_pool_count_by_protocol("FraxSwap").unwrap_or(0);
    let final_curve_stable = pool_db.get_pool_count_by_protocol("CurveStableswapNG").unwrap_or(0);
    let final_curve_twocrypto = pool_db.get_pool_count_by_protocol("CurveTwocryptoNG").unwrap_or(0);
    
    println!("   - UniswapV2: {} (+{})", final_v2, final_v2 - v2_count);
    println!("   - UniswapV3: {} (+{})", final_v3, final_v3 - v3_count);
    println!("   - SushiSwap: {} (+{})", final_sushi, final_sushi - sushi_count);
    println!("   - PancakeSwap: {} (+{})", final_pancake, final_pancake - pancake_count);
    println!("   - ShibaSwap: {}", final_shibaswap);
    println!("   - FraxSwap: {}", final_fraxswap);
    println!("   - Curve StableswapNG: {}", final_curve_stable);
    println!("   - Curve TwocryptoNG: {}", final_curve_twocrypto);

    if final_v2 + final_v3 > 150_000 {
        println!("\n🎉 SUCCESS! Database now contains 150,000+ pools - ready for MEV detection!");
    } else {
        println!("\n⚠️  Database still needs more pools for comprehensive MEV detection");
        println!("   Expected: 200,000+ pools");
        println!("   Current: {} pools", final_v2 + final_v3);
    }

    Ok(())
}