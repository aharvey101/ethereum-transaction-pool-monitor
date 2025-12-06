//! Import arboo cached pools for comparison

use anyhow::Result;
use ethereum_transaction_pool_monitor::pool_db::{DexPool, PoolDatabase};
use csv::ReaderBuilder;
use std::fs::File;

#[derive(Debug, serde::Deserialize)]
struct ArbooCachedPool {
    id: u32,
    address: String,
    version: u8,
    token0: String,
    token1: String,
    fee: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔍 Analyzing Arboo Cached Pools");
    println!("================================");
    
    let arboo_cache_path = "../arboo/cache/.cached-pools.csv";
    
    // Check if arboo cache exists
    if !std::path::Path::new(arboo_cache_path).exists() {
        println!("❌ Arboo cache not found at {}", arboo_cache_path);
        return Ok(());
    }
    
    println!("📂 Reading arboo cache: {}", arboo_cache_path);
    
    // Read arboo cache
    let file = File::open(arboo_cache_path)?;
    let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);
    
    let mut arboo_pools = Vec::new();
    let mut v2_count = 0;
    let mut v3_count = 0;
    let mut fee_300_count = 0;
    let mut fee_3000_count = 0;
    let mut other_fees = std::collections::HashMap::new();
    
    for result in reader.deserialize() {
        let pool: ArbooCachedPool = result?;
        
        // Count by version (as marked in arboo)
        if pool.version == 2 {
            v2_count += 1;
        } else if pool.version == 3 {
            v3_count += 1;
        }
        
        // Count by fee (actual indicator of V2/V3)
        match pool.fee {
            300 => fee_300_count += 1,
            3000 => fee_3000_count += 1,
            _ => {
                *other_fees.entry(pool.fee).or_insert(0) += 1;
            }
        }
        
        // Convert to our format (determine actual version by fee)
        let actual_protocol = if pool.fee == 300 {
            "UniswapV2".to_string()
        } else {
            "UniswapV3".to_string()  // 3000 and other fees are V3
        };
        
        let dex_pool = DexPool {
            address: pool.address,
            protocol: actual_protocol,
            token0: Some(pool.token0),
            token1: Some(pool.token1),
            chain_id: 1,
        };
        
        arboo_pools.push(dex_pool);
    }
    
    println!("📊 ARBOO CACHE ANALYSIS");
    println!("========================");
    println!("Total pools: {}", arboo_pools.len());
    println!("Marked as version 2: {}", v2_count);
    println!("Marked as version 3: {}", v3_count);
    println!("");
    println!("By fee structure (actual V2/V3 indicator):");
    println!("  300 fee (V2): {}", fee_300_count);
    println!("  3000 fee (V3): {}", fee_3000_count);
    if !other_fees.is_empty() {
        println!("  Other fees:");
        for (fee, count) in other_fees.iter() {
            println!("    {} fee: {}", fee, count);
        }
    }
    
    // Create database with arboo pools for comparison
    let arboo_db_path = "arboo_pools.db";
    let _ = std::fs::remove_file(arboo_db_path); // Clean existing
    let arboo_db = PoolDatabase::new(arboo_db_path)?;
    
    println!("");
    println!("💾 Importing to database for comparison...");
    arboo_db.add_pools(&arboo_pools)?;
    
    let imported_count = arboo_db.pool_count()?;
    let v2_imported = arboo_db.get_pool_count_by_protocol("UniswapV2")?;
    let v3_imported = arboo_db.get_pool_count_by_protocol("UniswapV3")?;
    
    println!("✅ Import complete:");
    println!("  Total imported: {}", imported_count);
    println!("  V2 pools: {}", v2_imported);
    println!("  V3 pools: {}", v3_imported);
    
    // Compare with our existing database if it exists
    let our_db_path = "dex_pools.db";
    if std::path::Path::new(our_db_path).exists() {
        println!("");
        println!("🔍 COMPARISON WITH OUR DATABASE");
        println!("================================");
        
        let our_db = PoolDatabase::new(our_db_path)?;
        let our_total = our_db.pool_count()?;
        let our_v2 = our_db.get_pool_count_by_protocol("UniswapV2").unwrap_or(0);
        let our_v3 = our_db.get_pool_count_by_protocol("UniswapV3").unwrap_or(0);
        
        println!("Our database:");
        println!("  Total: {}", our_total);
        println!("  V2: {}", our_v2);
        println!("  V3: {}", our_v3);
        println!("");
        println!("Arboo cache:");
        println!("  Total: {}", imported_count);
        println!("  V2: {}", v2_imported);
        println!("  V3: {}", v3_imported);
        println!("");
        println!("Difference:");
        println!("  Total: {} ({:+})", imported_count as i32 - our_total as i32, imported_count as i32 - our_total as i32);
        println!("  V2: {} ({:+})", v2_imported as i32 - our_v2 as i32, v2_imported as i32 - our_v2 as i32);
        println!("  V3: {} ({:+})", v3_imported as i32 - our_v3 as i32, v3_imported as i32 - our_v3 as i32);
        
        if imported_count > our_total {
            let improvement = ((imported_count as f64 / our_total as f64) - 1.0) * 100.0;
            println!("");
            println!("🚀 Arboo found {:.1}% more pools than our scanner!", improvement);
            println!("💡 This suggests we can improve our pool detection!");
        }
    } else {
        println!("");
        println!("💡 Our database not found - run scanner first to compare");
    }
    
    println!("");
    println!("📁 Arboo pools saved to: {}", arboo_db_path);
    println!("🔬 Use this for further analysis and comparison");
    
    Ok(())
}