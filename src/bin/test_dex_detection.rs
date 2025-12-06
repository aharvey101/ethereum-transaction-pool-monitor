//! DEX Pool Detection Test
//! 
//! This binary tests if pending transactions are targeting known DEX pools
//! and whether the is_dex detection logic is working correctly.

use anyhow::Result;
use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🔍 DEX Pool Detection Test");
    println!("=========================");
    
    let rpc_url = env::var("ETH_RPC_URL")
        .unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let db_path = "dex_pools.db";
    
    println!("📡 RPC URL: {}", rpc_url);
    println!("💾 Database: {}", db_path);
    
    // Initialize database and client
    let pool_db = PoolDatabase::new(db_path)?;
    let client = EthereumClient::new(&rpc_url).await?;
    
    // Check pool counts in database
    let protocol_counts = pool_db.get_all_protocol_counts()?;
    println!("\n📊 Pools in Database:");
    let mut total_pools = 0;
    for (protocol, count) in &protocol_counts {
        println!("• {}: {} pools", protocol, count);
        total_pools += count;
    }
    println!("• Total: {} pools", total_pools);
    
    // Get some sample pool addresses for testing
    println!("\n🎯 Sample Pool Addresses:");
    let sample_pools = pool_db.get_sample_pools(5)?;
    for pool in &sample_pools {
        println!("• {} ({})", pool.address, pool.protocol);
    }
    
    // Test router detection function directly
    println!("\n🔧 Testing Router Detection Logic:");
    let test_routers = [
        "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D", // Uniswap V2 Router
        "0xE592427A0AEce92De3Edee1F18E0157C05861564", // Uniswap V3 Router  
        "0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F", // SushiSwap Router
        "0x1111111254EEB25477B68fb85Ed929f73A960582", // 1inch Router V5
        "0xDEF171Fe48CF0115B1d80b88dc8eAb59176FEe57", // ParaSwap Router
    ];
    
    for router in &test_routers {
        let is_detected = pool_db.is_dex_router(router);
        println!("• {} -> {}", router, if is_detected { "✅ ROUTER" } else { "❌ NOT ROUTER" });
    }
    
    // Test combined detection (router + pool)
    println!("\n🔗 Testing Combined Detection (Routers + Pools):");
    for router in &test_routers {
        let is_detected = pool_db.is_dex_pool(router, 1)?;
        println!("• {} -> {}", router, if is_detected { "✅ DEX" } else { "❌ NOT DEX" });
    }
    
    // Test pool detection on actual pools
    println!("\n🧪 Testing Pool Detection Logic:");
    for pool in &sample_pools {
        let is_detected = pool_db.is_dex_pool(&pool.address, 1)?;
        println!("• {} -> {}", pool.address, if is_detected { "✅ DEX" } else { "❌ NOT DEX" });
    }
    
    // Get pending transactions
    println!("\n🔄 Fetching Pending Transactions...");
    match client.get_pending_transactions(&pool_db, 1).await {
        Ok(transactions) => {
            println!("📥 Found {} pending transactions", transactions.len());
            
            // Count DEX transactions
            let dex_count = transactions.iter().filter(|tx| tx.is_dex).count();
            let total_count = transactions.len();
            
            println!("\n📊 Transaction Analysis:");
            println!("• Total transactions: {}", total_count);
            println!("• DEX transactions: {} ({:.1}%)", 
                    dex_count, 
                    if total_count > 0 { (dex_count as f64 / total_count as f64) * 100.0 } else { 0.0 });
            println!("• Regular transactions: {}", total_count - dex_count);
            
            // Show details of DEX transactions found
            if dex_count > 0 {
                println!("\n✅ DEX Transactions Found:");
                let mut shown = 0;
                for tx in transactions.iter().filter(|tx| tx.is_dex) {
                    if shown >= 5 { break; } // Limit to first 5
                    let to_addr = tx.to.as_deref().unwrap_or("Contract Creation");
                    println!("• {} -> {} (Value: {} ETH)", 
                            tx.from, to_addr, tx.value_eth);
                    
                    // Verify the pool exists in database
                    if let Some(to) = &tx.to {
                        match pool_db.get_pool(to, 1)? {
                            Some(pool) => println!("  └─ Pool: {} protocol", pool.protocol),
                            None => println!("  └─ ⚠️ Pool not found in database!"),
                        }
                    }
                    shown += 1;
                }
                if dex_count > 5 {
                    println!("• ... and {} more DEX transactions", dex_count - 5);
                }
            } else {
                println!("\n❌ No DEX transactions found in current mempool");
                
                // Show sample of regular transactions to debug
                println!("\n📋 Sample of Non-DEX Transactions:");
                let mut shown = 0;
                for tx in transactions.iter().take(10) {
                    if shown >= 5 { break; }
                    let to_addr = tx.to.as_deref().unwrap_or("Contract Creation");
                    println!("• {} -> {} (Value: {} ETH)", 
                            tx.from, to_addr, tx.value_eth);
                    
                    // Test if this address would be detected as DEX
                    if let Some(to) = &tx.to {
                        let is_pool = pool_db.is_dex_pool(to, 1)?;
                        if is_pool {
                            println!("  └─ ⚠️ This IS a pool but tx.is_dex = false!");
                        }
                    }
                    shown += 1;
                }
            }
            
            // Test specific pool addresses from our database against recent transactions
            println!("\n🔍 Cross-checking Recent Transactions with Known Pools...");
            let mut found_matches = 0;
            let mut checked = 0;
            for tx in transactions.iter().take(1000) { // Limit to first 1000 for speed
                checked += 1;
                if let Some(to_addr) = &tx.to {
                    if pool_db.is_dex_pool(to_addr, 1)? {
                        found_matches += 1;
                        if found_matches <= 3 { // Show first 3 matches
                            println!("• Match found: {} -> Pool", to_addr);
                        }
                    }
                }
            }
            println!("• Checked {} transactions", checked);
            
            if found_matches == 0 {
                println!("• No transactions found targeting known pool addresses");
                println!("• This suggests pools may be inactive or addresses are incorrect");
            } else {
                println!("• Found {} transactions targeting known pools", found_matches);
            }
            
        }
        Err(e) => {
            println!("❌ Failed to fetch pending transactions: {}", e);
        }
    }
    
    println!("\n✅ DEX detection test complete!");
    
    Ok(())
}