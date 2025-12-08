use anyhow::Result;
use ethereum_transaction_pool_monitor::{eth_client::EthereumClient, pool_db::PoolDatabase};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🚀 Fast Performance Test...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "dex_pools.db";
    let chain_id = 1;
    
    let client = EthereumClient::new(&rpc_url).await?;
    let pool_db = PoolDatabase::new(db_path)?;
    
    println!("⚡ Testing different pagination ranges...");
    
    let start = std::time::Instant::now();
    let top_100 = client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 100).await?;
    let time_100 = start.elapsed();
    
    let start = std::time::Instant::now();
    let mid_100 = client.get_pending_transactions_paginated(&pool_db, chain_id, 500, 100).await?;
    let time_mid = start.elapsed();
    
    let start = std::time::Instant::now();  
    let bottom_100 = client.get_pending_transactions_paginated(&pool_db, chain_id, 2000, 100).await?;
    let time_bottom = start.elapsed();
    
    println!("📊 Results:");
    println!("   • Top 100 txs (highest gas):     {} txs in {:?}", top_100.len(), time_100);
    println!("   • Middle 100 txs (offset 500):  {} txs in {:?}", mid_100.len(), time_mid);
    println!("   • Bottom 100 txs (offset 2000): {} txs in {:?}", bottom_100.len(), time_bottom);
    
    if !top_100.is_empty() && !mid_100.is_empty() {
        println!("\n🔥 Gas price comparison:");
        println!("   • Highest gas price: {}", top_100[0].gas_price_gwei);
        if let Some(last_top) = top_100.last() {
            println!("   • 100th highest:     {}", last_top.gas_price_gwei);
        }
        if let Some(first_mid) = mid_100.first() {
            println!("   • 500th highest:     {}", first_mid.gas_price_gwei);
        }
        if !bottom_100.is_empty() {
            if let Some(first_bottom) = bottom_100.first() {
                println!("   • 2000th highest:    {}", first_bottom.gas_price_gwei);
            }
        }
    }
    
    // Test if middle transactions change more frequently  
    println!("\n⏳ Waiting 4 seconds to test for changes...");
    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
    
    let start = std::time::Instant::now();
    let mid_100_v2 = client.get_pending_transactions_paginated(&pool_db, chain_id, 500, 100).await?;
    let time_v2 = start.elapsed();
    
    if !mid_100.is_empty() && !mid_100_v2.is_empty() {
        let h1: HashSet<_> = mid_100.iter().map(|t| &t.hash).collect();
        let h2: HashSet<_> = mid_100_v2.iter().map(|t| &t.hash).collect();
        let changes = h1.symmetric_difference(&h2).count();
        
        println!("🔄 Second fetch (middle 100): {} txs in {:?}", mid_100_v2.len(), time_v2);
        println!("🔄 Changes in middle transactions: {} out of {}", changes, mid_100.len());
        
        if changes > 0 {
            println!("✅ Mid-range transactions ARE changing!");
        } else {
            println!("⚠️  Even mid-range transactions are stable");
        }
    }
    
    Ok(())
}