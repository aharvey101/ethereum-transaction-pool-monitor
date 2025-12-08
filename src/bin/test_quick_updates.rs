use anyhow::Result;
use ethereum_transaction_pool_monitor::{eth_client::EthereumClient, pool_db::PoolDatabase};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🔄 Quick Transaction Change Test...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "dex_pools.db";
    let chain_id = 1;
    
    let client = EthereumClient::new(&rpc_url).await?;
    let pool_db = PoolDatabase::new(db_path)?;
    
    println!("📡 First fetch...");
    let tx1 = client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 20).await?;
    println!("✅ Got {} transactions", tx1.len());
    if !tx1.is_empty() {
        println!("   First tx hash: {}", &tx1[0].hash[..16]);
        println!("   First tx gas: {}", tx1[0].gas_price_gwei);
    }
    
    println!("\n⏳ Waiting 3 seconds...");
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    
    println!("📡 Second fetch...");
    let tx2 = client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 20).await?;
    println!("✅ Got {} transactions", tx2.len());
    if !tx2.is_empty() {
        println!("   First tx hash: {}", &tx2[0].hash[..16]);
        println!("   First tx gas: {}", tx2[0].gas_price_gwei);
    }
    
    if !tx1.is_empty() && !tx2.is_empty() {
        let h1: HashSet<_> = tx1.iter().map(|t| &t.hash).collect();
        let h2: HashSet<_> = tx2.iter().map(|t| &t.hash).collect();
        let changes = h1.symmetric_difference(&h2).count();
        println!("\n🔄 {} transactions changed between fetches", changes);
        
        if changes > 0 {
            println!("✅ Transactions ARE updating!");
        } else {
            println!("⚠️  Same transactions in both fetches");
        }
    }
    
    Ok(())
}