//! The Graph Pool Fetcher
//! 
//! This binary fetches all UniswapV2, UniswapV3, and UniswapV4 pools from The Graph Protocol
//! and populates our database with comprehensive pool data.
//!
//! Features:
//! - Fetches ~100,000+ UniswapV2 pairs
//! - Fetches ~50,000+ UniswapV3 pools
//! - Fetches UniswapV4 pools  
//! - Uses ~150-200 queries (well within 100,000 free tier)
//! - Progress tracking and query usage monitoring

use anyhow::Result;
use ethereum_transaction_pool_monitor::{graph_client::GraphClient, pool_db::PoolDatabase};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("🔗 The Graph Pool Data Collection");
    println!("=================================");
    println!("📡 Using The Graph Protocol's free tier");
            println!("🎯 Target: 150,000+ pools from Uniswap V2/V3/V4");
    println!("📊 Query budget: 100,000 free queries/month");
    println!();

    let db_path = "dex_pools.db";
    let pool_db = PoolDatabase::new(db_path)?;
    
    // Show current database status
    println!("📊 Current database status:");
    match pool_db.pool_count() {
        Ok(count) => println!("   - Total pools: {}", count),
        Err(e) => println!("   - Error counting pools: {}", e),
    }

    let v2_count = pool_db.get_pool_count_by_protocol("UniswapV2").unwrap_or(0);
    let v3_count = pool_db.get_pool_count_by_protocol("UniswapV3").unwrap_or(0);
    let v4_count = pool_db.get_pool_count_by_protocol("UniswapV4").unwrap_or(0);
    
    println!("   - UniswapV2: {}", v2_count);
    println!("   - UniswapV3: {}", v3_count);
    println!("   - UniswapV4: {}", v4_count);
    println!();

    // Create Graph client with API key
    let api_key = "79942a724597827e4cb8972667c0a355".to_string();
    let mut graph_client = GraphClient::new(api_key);
    
    println!("🔑 Using Graph Network with API key");
    println!("🚀 Starting comprehensive pool data collection...");
    let start_time = std::time::Instant::now();
    
    // Fetch all pools from The Graph with optimized progressive writes
    let (final_v2_count, final_v3_count, final_v4_count) = graph_client.populate_database_from_graph_optimized(&pool_db).await?;
    
    let duration = start_time.elapsed();
    println!();
    println!("✅ Pool collection completed in {:.1}s!", duration.as_secs_f32());
    println!();
    
    // Show final results
    println!("📊 Final Results:");
    println!("=================");
    println!("🔗 UniswapV2 pairs: {} (+{})", final_v2_count, final_v2_count - v2_count);
    println!("🔗 UniswapV3 pools: {} (+{})", final_v3_count, final_v3_count - v3_count);
    println!("🔗 UniswapV4 pools: {} (+{})", final_v4_count, final_v4_count - v4_count);
    println!("🔗 Total pools: {}", final_v2_count + final_v3_count + final_v4_count);
    println!("📈 Queries used: {}/100,000 ({:.2}%)", 
             graph_client.query_count(), 
             (graph_client.query_count() as f32 / 100_000.0) * 100.0);
    
    // Verify database
    let final_db_count = pool_db.pool_count()?;
    println!("💾 Database verification: {} pools stored", final_db_count);
    
    // Success criteria
    if final_v2_count + final_v3_count + final_v4_count >= 100_000 {
        println!();
        println!("🎉 SUCCESS! Database now contains 100,000+ pools");
        println!("🔍 Ready for comprehensive MEV opportunity detection!");
        println!("📊 Coverage: Major Uniswap V2/V3/V4 liquidity pools");
        println!("💰 Cost: FREE (within The Graph's free tier)");
    } else {
        println!();
        println!("⚠️  Expected more pools, but still substantial coverage:");
        println!("              Current: {} pools", final_v2_count + final_v3_count + final_v4_count);
        println!("   This should still provide good MEV detection coverage!");
    }
    
    println!();
    println!("🎯 Next steps:");
    println!("   - Implement real-time MEV opportunity detection");
    println!("   - Add transaction analysis for arbitrage opportunities");
    println!("   - Monitor high-value pools for profitable trades");

    Ok(())
}