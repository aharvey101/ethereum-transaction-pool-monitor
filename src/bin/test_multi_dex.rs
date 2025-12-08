use anyhow::Result;
use ethereum_transaction_pool_monitor::{eth_client::EthereumClient, pool_db::PoolDatabase};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🎯 Testing Block-Aware Transaction Monitoring...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "dex_pools.db";
    let chain_id = 1;
    
    let client = EthereumClient::new(&rpc_url).await?;
    let pool_db = PoolDatabase::new(db_path)?;
    
    println!("📡 Connected to Ethereum node");
    
    // Get current block
    let current_block = client.get_latest_block_number().await?;
    println!("🔵 Current block: #{}", current_block);
    println!("🎯 Next block target: #{}", current_block + 1);
    
    // Get current pending transactions
    let pending_txs = client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 50).await?;
    println!("📊 Current pending transactions: {}", pending_txs.len());
    
    if !pending_txs.is_empty() {
        println!("\n🔥 Top 5 transactions by gas price (targeting block #{}):", current_block + 1);
        for (i, tx) in pending_txs.iter().take(5).enumerate() {
            println!("   {}. {} | {} | Hash: {}", 
                i + 1,
                tx.gas_price_gwei,
                tx.to.as_ref().map(|t| &t[..10]).unwrap_or("Contract"),
                &tx.hash[..16]
            );
        }
    }
    
    println!("\n⏳ Monitoring for new blocks (checking every 3 seconds)...");
    println!("💡 Press Ctrl+C to stop");
    
    let mut last_block = current_block;
    let mut check_count = 0;
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        check_count += 1;
        
        match client.get_latest_block_number().await {
            Ok(new_block) => {
                if new_block > last_block {
                    println!("\n🆕 NEW BLOCK DETECTED!");
                    println!("   📈 Block: #{} → #{}", last_block, new_block);
                    println!("   🎯 New target: #{}", new_block + 1);
                    
                    // Get new pending transactions for the new target block
                    match client.get_pending_transactions_paginated(&pool_db, chain_id, 0, 50).await {
                        Ok(new_pending) => {
                            println!("   📊 Transactions now targeting block #{}: {}", new_block + 1, new_pending.len());
                            
                            if !new_pending.is_empty() {
                                println!("   🔥 Top 3 new candidates:");
                                for (i, tx) in new_pending.iter().take(3).enumerate() {
                                    println!("      {}. {} | Hash: {}", 
                                        i + 1,
                                        tx.gas_price_gwei,
                                        &tx.hash[..16]
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            println!("   ❌ Error fetching new transactions: {}", e);
                        }
                    }
                    
                    last_block = new_block;
                } else {
                    print!("📊 Check #{}: Block #{} (no change)", check_count, new_block);
                    if check_count % 5 == 0 {
                        println!(" - Still waiting...");
                    } else {
                        print!("\r");
                    }
                }
            }
            Err(e) => {
                println!("\n❌ Error checking block: {}", e);
            }
        }
    }
}