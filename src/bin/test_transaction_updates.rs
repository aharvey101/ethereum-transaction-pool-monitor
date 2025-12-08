use anyhow::Result;
use ethereum_transaction_pool_monitor::{eth_client::EthereumClient, pool_db::PoolDatabase};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🔄 Testing Transaction Updates and Changes...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "dex_pools.db";
    let chain_id = 1;
    
    println!("📡 Connecting to Ethereum node: {}", rpc_url);
    
    let client = EthereumClient::new(&rpc_url).await?;
    let pool_db = PoolDatabase::new(db_path)?;
    
    println!("✅ Connected successfully!\n");
    
    let mut previous_hashes: Option<HashSet<String>> = None;
    
    for i in 1..=5 {
        println!("🔄 Update #{}: Fetching pending transactions...", i);
        
        match client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 100).await {
            Ok(transactions) => {
                println!("✅ Got {} transactions", transactions.len());
                
                if !transactions.is_empty() {
                    // Collect transaction hashes
                    let current_hashes: HashSet<String> = transactions.iter().map(|tx| tx.hash.clone()).collect();
                    
                    // Show sample transactions with gas prices
                    println!("📊 Top 5 transactions by gas price:");
                    for (idx, tx) in transactions.iter().take(5).enumerate() {
                        println!("   {}. {} | Gas: {} | Hash: {}", 
                            idx + 1, 
                            tx.to.as_ref().map(|t| &t[..10]).unwrap_or("None"),
                            tx.gas_price_gwei, 
                            &tx.hash[..10]
                        );
                    }
                    
                    // Compare with previous update
                    if let Some(prev_hashes) = &previous_hashes {
                        let new_txs = current_hashes.difference(prev_hashes).count();
                        let removed_txs = prev_hashes.difference(&current_hashes).count();
                        let unchanged_txs = current_hashes.intersection(prev_hashes).count();
                        
                        println!("🔄 Changes since last update:");
                        println!("   • New transactions: {}", new_txs);
                        println!("   • Removed transactions: {}", removed_txs);  
                        println!("   • Unchanged transactions: {}", unchanged_txs);
                        
                        if new_txs > 0 || removed_txs > 0 {
                            println!("✅ Mempool is actively changing!");
                        } else {
                            println!("⚠️  No changes detected (mempool might be stable)");
                        }
                    } else {
                        println!("ℹ️  First update - no comparison available");
                    }
                    
                    previous_hashes = Some(current_hashes);
                } else {
                    println!("⚠️  No pending transactions found");
                }
            }
            Err(e) => {
                println!("❌ Error fetching transactions: {}", e);
            }
        }
        
        println!(); // Empty line
        
        if i < 5 {
            println!("⏳ Waiting 8 seconds...\n");
            tokio::time::sleep(tokio::time::Duration::from_secs(8)).await;
        }
    }
    
    println!("🏁 Test completed!");
    println!("\n💡 Key insights:");
    println!("   • If you see 'No changes detected' repeatedly, the top 100 highest gas price");
    println!("     transactions might be very stable (common during low activity)");  
    println!("   • Active changes indicate the mempool is dynamic");
    println!("   • Higher gas price transactions have priority and change less frequently");
    
    Ok(())
}