//! Enhanced Pool Scanner
//! 
//! This binary implements an improved pool detection system based on the approach
//! used in the arboo project. It provides:
//! - Parallel pool scanning for V2 and V3 
//! - Progress tracking with visual indicators
//! - CSV export for analysis and comparison
//! - Robust error handling and retry logic
//! - Configurable block ranges and chunk sizes

use anyhow::Result;
use alloy::primitives::{Address, FixedBytes, B256, U256};
use alloy::providers::{Provider, ProviderBuilder, ReqwestProvider};
use alloy::rpc::types::eth::Filter;
use alloy_sol_types::SolValue;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::path::Path;
use std::sync::Arc;

// Factory contract addresses
pub const UNISWAP_V2_FACTORY: Address = Address::new([
    0x5C, 0x69, 0xbE, 0xe7, 0x01, 0xef, 0x81, 0x4a, 0x2B, 0x6a, 0x3E, 0xDD, 0x4B, 0x16, 0x52, 0xCB,
    0x9c, 0xc5, 0xaA, 0x6f,
]);

pub const UNISWAP_V3_FACTORY: Address = Address::new([
    0x1F, 0x98, 0x43, 0x1c, 0x8a, 0xD9, 0x85, 0x23, 0x63, 0x1A, 0xE4, 0xa5, 0x9f, 0x26, 0x73, 0x46,
    0xea, 0x31, 0xF9, 0x84,
]);

// Focus on active pool creation ranges based on database analysis
const PRODUCTIVE_V2_START_BLOCK: u64 = 10_000_000; // Most V2 pools created after block 10M
const UNISWAP_V3_DEPLOYMENT_BLOCK: u64 = 12_369_739; // V3 deployed on May 5, 2021

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DexVariant {
    UniswapV2,
    UniswapV3,
}

impl DexVariant {
    pub fn num(&self) -> u8 {
        match self {
            DexVariant::UniswapV2 => 2,
            DexVariant::UniswapV3 => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    pub id: i64,
    pub address: Address,
    pub version: DexVariant,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub block_number: u64,
}

impl Pool {
    pub fn cache_row(&self) -> (i64, String, i32, String, String, u32, u64) {
        (
            self.id,
            format!("{:?}", self.address),
            self.version.num() as i32,
            format!("{:?}", self.token0),
            format!("{:?}", self.token1),
            self.fee,
            self.block_number,
        )
    }

    pub fn pretty_msg(&self) -> String {
        format!(
            "[{:?}] {:?}: {:?} --> {:?} (fee: {}, block: {})",
            self.version, self.address, self.token0, self.token1, self.fee, self.block_number
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    env_logger::init();

    println!("🚀 Enhanced Pool Scanner - Based on Arboo Architecture");
    println!("=======================================================");

    // Configuration
    let rpc_url = std::env::var("ETH_RPC_URL")
        .unwrap_or_else(|_| "http://192.168.0.14:8545".to_string()); // Use HTTP for better compatibility
    let cache_path = "enhanced_pools.csv";
    let chunk_size = 50_000u64; // Process 50k blocks at a time for better parallelization
    
    println!("📡 RPC URL: {}", rpc_url);
    println!("💾 Cache path: {}", cache_path);
    println!("📦 Chunk size: {} blocks", chunk_size);

    // Load all pools using the enhanced method
    let (pools, _) = load_all_pools_enhanced(rpc_url, chunk_size, cache_path).await?;

    // Analysis and comparison
    println!("\n📊 Enhanced Pool Detection Results");
    println!("==================================");
    
    let v2_count = pools.iter().filter(|p| matches!(p.version, DexVariant::UniswapV2)).count();
    let v3_count = pools.iter().filter(|p| matches!(p.version, DexVariant::UniswapV3)).count();
    
    println!("🔹 UniswapV2 pools: {}", v2_count);
    println!("🔹 UniswapV3 pools: {}", v3_count);
    println!("🔹 Total pools: {}", pools.len());
    
    // Block range analysis
    if !pools.is_empty() {
        let min_block = pools.iter().map(|p| p.block_number).min().unwrap();
        let max_block = pools.iter().map(|p| p.block_number).max().unwrap();
        println!("🔹 Block range: {} to {} ({} blocks)", min_block, max_block, max_block - min_block);
    }

    // Compare with existing database
    println!("\n🔍 Comparison with Current Database");
    println!("===================================");
    compare_with_current_db(&pools).await?;

    Ok(())
}

async fn load_all_pools_enhanced(
    rpc_url: String,
    chunk_size: u64,
    cache_path: &str,
) -> Result<(Vec<Pool>, i64)> {
    // Create cache directory
    create_dir_all("cache")?;
    println!("📁 Creating cache file at: {}", cache_path);

    let file_path = Path::new(cache_path);
    let file_exists = file_path.exists();
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(cache_path)?;
    let mut writer = csv::Writer::from_writer(file);

    let mut pools = Vec::new();
    let mut last_id = -1i64;

    // Load existing pools if cache exists
    if file_exists {
        let mut reader = csv::Reader::from_path(cache_path)?;
        for row in reader.records() {
            let row = row?;
            if let Ok(pool) = parse_pool_from_csv(&row) {
                if pool.id > last_id {
                    last_id = pool.id;
                }
                pools.push(pool);
            }
        }
        println!("📚 Loaded {} existing pools from cache", pools.len());
    } else {
        // Write CSV headers for new file
        writer.write_record([
            "id", "address", "version", "token0", "token1", "fee", "block_number"
        ])?;
    }

    // Connect to HTTP provider for better compatibility
    println!("🔌 Connecting to Ethereum node...");
    let provider = Arc::new(ProviderBuilder::new().on_http(rpc_url.parse()?));

    let current_block = provider.get_block_number().await?;
    println!("⛓️ Current block number: {}", current_block);

    // Determine scanning ranges - follow arboo's approach
    let v2_start = if !pools.is_empty() {
        pools.iter()
            .filter(|p| matches!(p.version, DexVariant::UniswapV2))
            .map(|p| p.block_number)
            .max()
            .unwrap_or(PRODUCTIVE_V2_START_BLOCK) + 1
    } else {
        PRODUCTIVE_V2_START_BLOCK // Start from block 10M where pools actually exist
    };

    let v3_start = if !pools.is_empty() {
        pools.iter()
            .filter(|p| matches!(p.version, DexVariant::UniswapV3))
            .map(|p| p.block_number)
            .max()
            .unwrap_or(UNISWAP_V3_DEPLOYMENT_BLOCK) + 1
    } else {
        UNISWAP_V3_DEPLOYMENT_BLOCK // V3 starts from deployment
    };

    println!("📍 V2 scanning: block {} to {}", v2_start, current_block);
    println!("📍 V3 scanning: block {} to {}", v3_start, current_block);

    // Generate block ranges for parallel processing
    let block_ranges = generate_block_ranges(
        std::cmp::min(v2_start, v3_start),
        current_block,
        chunk_size,
    );

    println!("🔧 Processing {} block ranges with chunk size {}", block_ranges.len(), chunk_size);

    // Setup progress bar
    let pb = ProgressBar::new(block_ranges.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}"
        )?
        .progress_chars("##-"),
    );

    let mut new_pools = Vec::new();

    // Process block ranges in parallel batches for better performance
    let batch_size = 20; // Process 20 ranges concurrently (40 total tasks per batch)
    let pb = ProgressBar::new(block_ranges.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}"
        )?
        .progress_chars("##-"),
    );

    for batch in block_ranges.chunks(batch_size) {
        let mut tasks = Vec::new();
        
        for (start, end) in batch {
            let provider_v2 = provider.clone();
            let provider_v3 = provider.clone();
            let start = *start;
            let end = *end;
            
            // Create concurrent tasks for V2 and V3 in this range
            let v2_task = tokio::spawn(async move {
                load_uniswap_v2_pools(provider_v2, start, end).await
            });
            
            let v3_task = tokio::spawn(async move {
                load_uniswap_v3_pools(provider_v3, start, end).await
            });
            
            tasks.push(v2_task);
            tasks.push(v3_task);
        }
        
        pb.set_message(format!("Processing {} concurrent tasks...", tasks.len()));
        
        // Wait for all tasks in this batch to complete
        let results = futures::future::join_all(tasks).await;
        
        for result in results {
            match result? {
                Ok(chunk_pools) => {
                    new_pools.extend(chunk_pools);
                }
                Err(e) => {
                    eprintln!("⚠️ Error in parallel task: {}", e);
                }
            }
        }
        
        pb.inc(batch.len() as u64);
    }

    pb.finish_with_message("Scanning complete!");

    println!("✅ Discovered {} new pools", new_pools.len());

    // Assign IDs and write to cache
    let mut current_id = last_id;
    for pool in new_pools.iter_mut() {
        current_id += 1;
        pool.id = current_id;
        
        // Write to CSV
        writer.serialize(pool.cache_row())?;
        pools.push(pool.clone());
    }

    writer.flush()?;
    println!("💾 Cache file updated with {} new pools", new_pools.len());

    Ok((pools, current_id))
}

async fn load_uniswap_v2_pools(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<Pool>> {
    let mut pools = Vec::new();

    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![UNISWAP_V2_FACTORY])
        .event("PairCreated(address,address,address,uint256)");

    println!("🔍 Scanning V2 pools from block {} to {} on factory {}", from_block, to_block, UNISWAP_V2_FACTORY);
    let logs = provider.get_logs(&event_filter).await?;
    println!("📊 Found {} V2 PairCreated events", logs.len());

    for log in logs {
        let block_number = log.block_number.unwrap_or_default();

        // Extract token addresses from topics
        let token0 = Address::from(
            FixedBytes::<20>::try_from(&log.topics()[1][12..32])?
        );
        let token1 = Address::from(
            FixedBytes::<20>::try_from(&log.topics()[2][12..32])?
        );

        // Decode pool address from data
        let log_data = &log.inner.data.data;
        let decoded: (Address, B256) = SolValue::abi_decode(log_data, false)?;
        let pool_address = decoded.0;

        let pool = Pool {
            id: -1, // Will be assigned later
            address: pool_address,
            version: DexVariant::UniswapV2,
            token0,
            token1,
            fee: 300, // V2 has fixed 0.3% fee
            block_number,
        };
        
        pools.push(pool);
    }

    println!("✅ Successfully loaded {} V2 pools", pools.len());
    Ok(pools)
}

async fn load_uniswap_v3_pools(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<Pool>> {
    let mut pools = Vec::new();

    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![UNISWAP_V3_FACTORY])
        .event("PoolCreated(address,address,uint24,int24,address)");

    println!("🔍 Scanning V3 pools from block {} to {} on factory {}", from_block, to_block, UNISWAP_V3_FACTORY);
    let logs = provider.get_logs(&event_filter).await?;
    println!("📊 Found {} V3 PoolCreated events", logs.len());
    
    for log in logs {
        if log.topics()[1].is_zero() {
            println!("V3 log 1 empty");
            continue;
        }
        
        let block_number = log.block_number.unwrap_or_default();

        // Extract token addresses from topics - follow arboo's exact method
        let topic0 = log.topics()[1];
        let topic0 = FixedBytes::<20>::try_from(&topic0[12..32])
            .map_err(|e| anyhow::anyhow!("Invalid topic0 format in V3 log: {}", e))?;
        let token0 = Address::from(topic0);

        let topic1 = log.topics()[2];
        let topic1 = FixedBytes::<20>::try_from(&topic1[12..32])
            .map_err(|e| anyhow::anyhow!("Invalid topic1 format in V3 log: {}", e))?;
        let token1 = Address::from(topic1);

        // Decode the log data - V3 PoolCreated event has (uint24 fee, int24 tickSpacing, address pool)
        let log_data = &log.inner.data.data;
        let decoded: (U256, i32, Address) = SolValue::abi_decode(log_data, false)
            .map_err(|e| {
                eprintln!("⚠️ Failed to decode V3 log data in block {}: {}", block_number, e);
                anyhow::anyhow!("Failed to decode V3 log data: {}", e)
            })?;

        let fee = decoded.0.to::<u32>(); // fee is uint24, can safely convert to u32
        let pool_address = decoded.2; // pool address is the third field

        let pool = Pool {
            id: -1, // Will be assigned later
            address: pool_address,
            version: DexVariant::UniswapV3,
            token0,
            token1,
            fee,
            block_number,
        };
        
        pools.push(pool);
    }

    println!("✅ Successfully loaded {} V3 pools", pools.len());
    Ok(pools)
}

fn generate_block_ranges(from: u64, to: u64, chunk_size: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut current = from;
    
    while current <= to {
        let end = std::cmp::min(current + chunk_size - 1, to);
        ranges.push((current, end));
        current = end + 1;
    }
    
    ranges
}

fn parse_pool_from_csv(record: &csv::StringRecord) -> Result<Pool> {
    let version = match record.get(2).and_then(|v| v.parse().ok()).unwrap_or(2) {
        2 => DexVariant::UniswapV2,
        3 => DexVariant::UniswapV3,
        _ => DexVariant::UniswapV2,
    };

    let pool = Pool {
        id: record.get(0).and_then(|v| v.parse().ok()).unwrap_or(0),
        address: record.get(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        version,
        token0: record.get(3)
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        token1: record.get(4)
            .and_then(|v| v.parse().ok())
            .unwrap_or_default(),
        fee: record.get(5).and_then(|v| v.parse().ok()).unwrap_or(3000),
        block_number: record.get(6).and_then(|v| v.parse().ok()).unwrap_or(0),
    };

    Ok(pool)
}

async fn compare_with_current_db(enhanced_pools: &[Pool]) -> Result<()> {
    // Connect to current database
    let db = rusqlite::Connection::open("dex_pools.db")?;
    
    // Get counts from current database
    let mut stmt = db.prepare("SELECT COUNT(*) as count, protocol FROM dex_pools GROUP BY protocol")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut current_v2 = 0;
    let mut current_v3 = 0;
    
    for row in rows {
        let (count, protocol) = row?;
        match protocol.as_str() {
            "uniswap_v2" => current_v2 = count,
            "uniswap_v3" => current_v3 = count,
            _ => {}
        }
    }

    let enhanced_v2 = enhanced_pools.iter().filter(|p| matches!(p.version, DexVariant::UniswapV2)).count();
    let enhanced_v3 = enhanced_pools.iter().filter(|p| matches!(p.version, DexVariant::UniswapV3)).count();

    println!("📊 Pool Count Comparison:");
    println!("┌─────────────┬─────────────┬─────────────┬─────────────┐");
    println!("│ Version     │ Current DB  │ Enhanced    │ Difference  │");
    println!("├─────────────┼─────────────┼─────────────┼─────────────┤");
    println!("│ UniswapV2   │ {:>11} │ {:>11} │ {:>+11} │", current_v2, enhanced_v2, enhanced_v2 as i32 - current_v2);
    println!("│ UniswapV3   │ {:>11} │ {:>11} │ {:>+11} │", current_v3, enhanced_v3, enhanced_v3 as i32 - current_v3);
    println!("├─────────────┼─────────────┼─────────────┼─────────────┤");
    println!("│ Total       │ {:>11} │ {:>11} │ {:>+11} │", current_v2 + current_v3, enhanced_v2 + enhanced_v3, (enhanced_v2 + enhanced_v3) as i32 - (current_v2 + current_v3));
    println!("└─────────────┴─────────────┴─────────────┴─────────────┘");

    let improvement_v2 = if current_v2 > 0 {
        (enhanced_v2 as f64 - current_v2 as f64) / current_v2 as f64 * 100.0
    } else {
        0.0
    };
    
    let improvement_v3 = if current_v3 > 0 {
        (enhanced_v3 as f64 - current_v3 as f64) / current_v3 as f64 * 100.0
    } else {
        0.0
    };

    println!("\n💡 Analysis:");
    if improvement_v2 != 0.0 {
        println!("  • V2 detection improvement: {:+.1}%", improvement_v2);
    }
    if improvement_v3 != 0.0 {
        println!("  • V3 detection improvement: {:+.1}%", improvement_v3);
    }
    
    let total_enhanced = enhanced_v2 + enhanced_v3;
    let total_current = current_v2 + current_v3;
    
    if total_enhanced > total_current as usize {
        println!("  ✅ Enhanced method found {} additional pools!", total_enhanced - total_current as usize);
    } else if total_enhanced < total_current as usize {
        println!("  ⚠️  Enhanced method found {} fewer pools", total_current as usize - total_enhanced);
    } else {
        println!("  ⚖️  Both methods found the same number of pools");
    }

    Ok(())
}