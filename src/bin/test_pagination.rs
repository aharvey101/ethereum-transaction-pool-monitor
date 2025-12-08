use anyhow::Result;
use ethereum_transaction_pool_monitor::{eth_client::EthereumClient, pool_db::PoolDatabase};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🧪 Testing Pending-Only Pagination Functionality...\n");
    
    // Test scroll-to-bottom detection logic first (no network required)
    println!("🖱️  Testing scroll detection logic...");
    
    // Simulate different scroll scenarios
    let test_cases = vec![
        (100, 20, 75, "Near bottom - should trigger loading"),  // scroll=75, max_rows=20, total=100
        (100, 20, 50, "Middle - should not trigger"),           // scroll=50, max_rows=20, total=100  
        (100, 20, 10, "Top - should not trigger"),              // scroll=10, max_rows=20, total=100
        (50, 20, 25, "At bottom - should trigger"),             // scroll=25, max_rows=20, total=50
        (1000, 30, 960, "Near bottom large list - should trigger"), // scroll=960, max_rows=30, total=1000
    ];
    
    for (total_count, max_rows, scroll_offset, description) in test_cases {
        let threshold = 10;
        let should_load = scroll_offset + max_rows + threshold >= total_count;
        
        println!("• {} → {}", 
            description, 
            if should_load { "✅ LOAD" } else { "❌ NO LOAD" }
        );
        println!("  (scroll={}, visible={}, total={}, threshold={})", 
            scroll_offset, max_rows, total_count, threshold);
    }
    
    // Test network functionality if we have a valid RPC URL
    let rpc_url = std::env::var("RPC_URL").unwrap_or_default();
    
    if rpc_url.is_empty() || rpc_url.contains("demo") {
        println!("\n⚠️  Skipping network tests (no custom RPC_URL provided)");
        println!("💡 To test network functionality:");
        println!("   export RPC_URL='your-actual-rpc-endpoint'");
        println!("   cargo run --bin test_pagination");
    } else {
        println!("\n📡 Testing network functionality with custom RPC...");
        
        let db_path = "dex_pools.db";
        let chain_id = 1;
        
        // Initialize client and database
        match EthereumClient::new(&rpc_url).await {
            Ok(client) => {
                match PoolDatabase::new(db_path) {
                    Ok(pool_db) => {
                        println!("✅ Connected to Ethereum node and database");
                        
                        // Test getting pending transactions
                        println!("🔄 Testing pending transaction fetch...");
                        match client.get_pending_transactions(&pool_db, chain_id).await {
                            Ok(pending_txs) => {
                                println!("✅ Found {} pending transactions", pending_txs.len());
                                
                                if !pending_txs.is_empty() {
                                    println!("\n📊 Sample pending transactions:");
                                    for (i, tx) in pending_txs.iter().take(3).enumerate() {
                                        let activity_type = match &tx.to {
                                            Some(to_addr) => {
                                                match pool_db.get_defi_activity_type(to_addr, chain_id) {
                                                    Ok(activity) => format!("{:?}", activity),
                                                    Err(_) => "Unknown".to_string(),
                                                }
                                            }
                                            None => "Contract Creation".to_string(),
                                        };
                                        
                                        let swap_info = match &tx.swap_info {
                                            Some(info) => format!(" | {}", info.function_name),
                                            None => String::new(),
                                        };
                                        
                                        println!("{}. {} → {} | Gas: {} | {}{}", 
                                            i + 1,
                                            &tx.from[..8],
                                            tx.to.as_ref().map(|t| &t[..8]).unwrap_or("None"),
                                            tx.gas_price_gwei,
                                            activity_type,
                                            swap_info
                                        );
                                    }
                                    
                                    // Test paginated fetch
                                    println!("\n🔢 Testing paginated pending transaction fetch...");
                                    let offset = 10;
                                    let limit = 5;
                                    match client.get_pending_transactions_paginated(&pool_db, chain_id, offset, limit).await {
                                        Ok(paginated_txs) => {
                                            println!("✅ Paginated fetch (offset: {}, limit: {}) returned {} transactions", 
                                                offset, limit, paginated_txs.len());
                                            
                                            // Verify gas price sorting (should be descending)
                                            if paginated_txs.len() > 1 {
                                                let is_sorted = paginated_txs.windows(2).all(|w| w[0].gas_price_f64 >= w[1].gas_price_f64);
                                                if is_sorted {
                                                    println!("✅ Transactions are properly sorted by gas price (high to low)");
                                                } else {
                                                    println!("⚠️  Transactions may not be sorted by gas price");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            println!("❌ Failed to fetch paginated pending transactions: {}", e);
                                        }
                                    }
                                } else {
                                    println!("ℹ️  No pending transactions found (normal during low mempool activity)");
                                }
                            }
                            Err(e) => {
                                println!("❌ Failed to fetch pending transactions: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to initialize database: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to connect to Ethereum node: {}", e);
            }
        }
    }
    
    println!("\n✅ Pending-only pagination test completed!");
    println!("💡 To test in the main application:");
    println!("   1. Run: cargo run");
    println!("   2. Wait for pending transactions to load"); 
    println!("   3. Scroll down to the bottom using PageDown or arrow keys");
    println!("   4. Watch for 'Loading more...' indicator in the status bar");
    println!("   5. More pending transactions sorted by gas price should be loaded");
    println!("   6. Higher gas price transactions should appear at the top");
    
    Ok(())
}