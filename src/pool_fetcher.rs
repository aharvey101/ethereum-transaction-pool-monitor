use anyhow::Result;
use alloy::providers::{ProviderBuilder, Provider, ReqwestProvider};
use alloy::rpc::types::eth::Filter;
use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use std::str::FromStr;
use crate::pool_db::{DexPool, PoolDatabase};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use futures::future::join_all;

/// Pool fetcher that queries the Ethereum node for pool creation events using alloy
pub struct PoolFetcher {
    rpc_url: String,
}

/// Progress callback for pool loading - takes (window_message, total_pools_found)
pub type ProgressCallback = Arc<Mutex<Box<dyn Fn(String, u32) + Send>>>;

impl PoolFetcher {
    pub fn new(rpc_url: &str) -> Self {
        PoolFetcher {
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Fetch UniswapV3 pools from node by querying PoolCreated events
    /// Scans ALL blocks from V3 deployment (block 12,369,739) to current block
    /// Uses 100k block windows for efficiency
    /// UniswapV3 Factory: 0x1F98431c8aD98523631AE4a59f267346ea31F984
    pub async fn fetch_uniswap_v3_pools(&self, pool_db: &PoolDatabase, _chain_id: u32) -> Result<u32> {
        self.fetch_uniswap_v3_pools_with_progress(pool_db, _chain_id, None).await
    }

    /// Fetch UniswapV3 pools with progress callback
    pub async fn fetch_uniswap_v3_pools_with_progress(&self, pool_db: &PoolDatabase, _chain_id: u32, progress: Option<ProgressCallback>) -> Result<u32> {
        const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
        const UNISWAP_V3_DEPLOYMENT_BLOCK: u64 = 12_369_739; // V3 deployed on May 5, 2021
        const BLOCK_WINDOW: u64 = 100_000; // Query in 100k block windows for efficiency

        tracing::info!("Fetching UniswapV3 pools from node (scanning all blocks from deployment)");
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("UniswapV3: Initializing...".to_string(), 0);
        }

        // Create provider
        let provider = ProviderBuilder::new()
            .on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);
        tracing::info!("Scanning from V3 deployment block {} to current block {} ({} blocks)", 
                      UNISWAP_V3_DEPLOYMENT_BLOCK, current_block, current_block - UNISWAP_V3_DEPLOYMENT_BLOCK);

        let total_blocks = current_block - UNISWAP_V3_DEPLOYMENT_BLOCK;
        let total_windows = (total_blocks + BLOCK_WINDOW - 1) / BLOCK_WINDOW;

        let mut all_pools = Vec::new();

        // Query from V3 deployment block to current block in windows
        let mut to_block = current_block;
        let mut window_idx = 0;
        while to_block >= UNISWAP_V3_DEPLOYMENT_BLOCK {
            let from_block = if to_block > BLOCK_WINDOW {
                to_block - BLOCK_WINDOW
            } else {
                UNISWAP_V3_DEPLOYMENT_BLOCK
            };

            let progress_pct = ((window_idx as f32 / total_windows as f32) * 100.0) as u32;
            let msg = format!("UniswapV3: Scanning window {}/{} ({}%)", window_idx, total_windows, progress_pct);
            tracing::info!("{} - blocks {}-{}", msg, from_block, to_block);
            if let Some(ref cb) = progress {
                cb.lock().unwrap()(msg, all_pools.len() as u32);
            }

            let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
            
            let event_filter = Filter::new()
                .from_block(from_block)
                .to_block(to_block)
                .address(vec![factory_addr])
                .event("PoolCreated(address,address,uint24,int24,address)");

            match provider.get_logs(&event_filter).await {
                Ok(logs) => {
                    if !logs.is_empty() {
                        tracing::debug!("Found {} PoolCreated events in window {}", logs.len(), window_idx);

                        for log in logs {
                            if let Ok(pool) = parse_v3_pool_created_log(&log) {
                                tracing::debug!("Found UniswapV3 pool: {}", pool.address);
                                all_pools.push(pool);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error querying V3 logs in window {}: {}", window_idx, e);
                    // Don't return immediately - continue with next window
                    // The error might be from unconfirmed blocks or other temporary issues
                }
            }

            if from_block <= UNISWAP_V3_DEPLOYMENT_BLOCK {
                break;
            }
            to_block = from_block - 1;
            window_idx += 1;
        }

        if let Some(ref cb) = progress {
            cb.lock().unwrap()(format!("UniswapV3: Found {} pools, saving...", all_pools.len()), all_pools.len() as u32);
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV3 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        tracing::info!("UniswapV3 pool fetch complete: {} pools found", all_pools.len());
        if let Some(ref cb) = progress {
            cb.lock().unwrap()(format!("UniswapV3: Complete! {} pools found", all_pools.len()), all_pools.len() as u32);
        }
        Ok(all_pools.len() as u32)
    }


    /// Fetch UniswapV2 pools from node using rolling window approach
    /// Scans ALL blocks from V2 deployment (block 10,000,835) to current block
    /// Uses 100k block windows for efficiency
    /// UniswapV2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
    pub async fn fetch_uniswap_v2_pools(&self, pool_db: &PoolDatabase, _chain_id: u32) -> Result<u32> {
        self.fetch_uniswap_v2_pools_with_progress(pool_db, _chain_id, None).await
    }

    /// Fetch UniswapV2 pools with progress callback
    pub async fn fetch_uniswap_v2_pools_with_progress(&self, pool_db: &PoolDatabase, _chain_id: u32, progress: Option<ProgressCallback>) -> Result<u32> {
        const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";
        const UNISWAP_V2_DEPLOYMENT_BLOCK: u64 = 10_000_835; // V2 deployed on May 5, 2020
        const BLOCK_WINDOW: u64 = 100_000; // Query in 100k block windows for efficiency

        tracing::info!("Fetching UniswapV2 pools from node (scanning all blocks from deployment)");
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("UniswapV2: Initializing...".to_string(), 0);
        }

        // Create provider
        let provider = ProviderBuilder::new()
            .on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);
        tracing::info!("Scanning from V2 deployment block {} to current block {} ({} blocks)", 
                      UNISWAP_V2_DEPLOYMENT_BLOCK, current_block, current_block - UNISWAP_V2_DEPLOYMENT_BLOCK);

        let total_blocks = current_block - UNISWAP_V2_DEPLOYMENT_BLOCK;
        let total_windows = (total_blocks + BLOCK_WINDOW - 1) / BLOCK_WINDOW;

        let mut all_pools = Vec::new();

        // Query from V2 deployment block to current block in windows
        let mut to_block = current_block;
        let mut window_idx = 0;
        while to_block >= UNISWAP_V2_DEPLOYMENT_BLOCK {
            let from_block = if to_block > BLOCK_WINDOW {
                to_block - BLOCK_WINDOW
            } else {
                UNISWAP_V2_DEPLOYMENT_BLOCK
            };

            let progress_pct = ((window_idx as f32 / total_windows as f32) * 100.0) as u32;
            let msg = format!("UniswapV2: Scanning window {}/{} ({}%)", window_idx, total_windows, progress_pct);
            tracing::info!("{} - blocks {}-{}", msg, from_block, to_block);
            if let Some(ref cb) = progress {
                cb.lock().unwrap()(msg, all_pools.len() as u32);
            }

            let factory_addr = Address::from_str(UNISWAP_V2_FACTORY)?;
            let event_filter = Filter::new()
                .from_block(from_block)
                .to_block(to_block)
                .address(vec![factory_addr])
                .event("PairCreated(address,address,address,uint256)");

            match provider.get_logs(&event_filter).await {
                Ok(logs) => {
                    if !logs.is_empty() {
                        tracing::debug!("Found {} PairCreated events in window {}", logs.len(), window_idx);

                        for log in logs {
                            if let Ok(pool) = parse_v2_pair_created_log(&log, "UniswapV2") {
                                tracing::debug!("Found UniswapV2 pool: {}", pool.address);
                                all_pools.push(pool);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error querying V2 logs in window {}: {}", window_idx, e);
                }
            }

            if from_block <= UNISWAP_V2_DEPLOYMENT_BLOCK {
                break;
            }
            to_block = from_block - 1;
            window_idx += 1;
        }

        if let Some(ref cb) = progress {
            cb.lock().unwrap()(format!("UniswapV2: Found {} pools, saving...", all_pools.len()), all_pools.len() as u32);
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV2 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        tracing::info!("UniswapV2 pool fetch complete: {} pools found", all_pools.len());
        if let Some(ref cb) = progress {
            cb.lock().unwrap()(format!("UniswapV2: Complete! {} pools found", all_pools.len()), all_pools.len() as u32);
        }
        Ok(all_pools.len() as u32)
    }

    /// Parallel fetch method that scans multiple DEX pools concurrently
    /// Uses smaller chunk sizes and parallel processing for maximum speed
    /// Now supports: UniswapV2, UniswapV3, SushiSwap, PancakeSwap
    pub async fn fetch_pools_parallel(&self, pool_db: &PoolDatabase, _chain_id: u32, progress: Option<ProgressCallback>) -> Result<u32> {
        const CHUNK_SIZE: u64 = 50_000; // Smaller chunks for better parallelization
        const BATCH_SIZE: usize = 20; // Number of concurrent tasks per batch
        
        tracing::info!("Starting parallel multi-DEX pool scanning with chunk size {} and batch size {}", CHUNK_SIZE, BATCH_SIZE);
        
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("Multi-DEX scanning: Initializing...".to_string(), 0);
        }

        // Create provider
        let provider = Arc::new(ProviderBuilder::new().on_http(self.rpc_url.parse()?));
        let current_block = provider.get_block_number().await?;
        
        tracing::info!("Current block: {}", current_block);
        
        // Define starting blocks for different protocols
        const UNISWAP_V2_START_BLOCK: u64 = 10_000_835; // V2 deployment - May 2020
        const UNISWAP_V3_START_BLOCK: u64 = 12_369_739; // V3 deployment - May 2021
        const SUSHISWAP_START_BLOCK: u64 = 10_794_229;  // SushiSwap deployment - September 2020
        const PANCAKESWAP_START_BLOCK: u64 = 15_614_590; // PancakeSwap V2 on Ethereum - December 2022
        
        // Generate block ranges for each DEX protocol
        let uniswap_v2_ranges = generate_block_ranges(UNISWAP_V2_START_BLOCK, current_block, CHUNK_SIZE);
        let uniswap_v3_ranges = generate_block_ranges(UNISWAP_V3_START_BLOCK, current_block, CHUNK_SIZE);
        let sushiswap_ranges = generate_block_ranges(SUSHISWAP_START_BLOCK, current_block, CHUNK_SIZE);
        let pancakeswap_ranges = generate_block_ranges(PANCAKESWAP_START_BLOCK, current_block, CHUNK_SIZE);
        
        let total_ranges = uniswap_v2_ranges.len() + uniswap_v3_ranges.len() + 
                          sushiswap_ranges.len() + pancakeswap_ranges.len();
        let completed_ranges = Arc::new(AtomicU32::new(0));
        let total_pools_found = Arc::new(AtomicU32::new(0));
        
        tracing::info!("Generated ranges: UniV2={}, UniV3={}, Sushi={}, Pancake={} (total={})", 
                      uniswap_v2_ranges.len(), uniswap_v3_ranges.len(), 
                      sushiswap_ranges.len(), pancakeswap_ranges.len(), total_ranges);
        
        // Combine all ranges with their types for unified processing
        let mut all_tasks = Vec::new();
        
        // Create UniswapV2 tasks
        for (start, end) in uniswap_v2_ranges {
            all_tasks.push((start, end, "UniswapV2"));
        }
        
        // Create UniswapV3 tasks  
        for (start, end) in uniswap_v3_ranges {
            all_tasks.push((start, end, "UniswapV3"));
        }
        
        // Create SushiSwap tasks
        for (start, end) in sushiswap_ranges {
            all_tasks.push((start, end, "SushiSwap"));
        }
        
        // Create PancakeSwap tasks
        for (start, end) in pancakeswap_ranges {
            all_tasks.push((start, end, "PancakeSwap"));
        }
        
        let mut all_pools = Vec::new();
        
        // Process tasks in batches for controlled concurrency
        for batch in all_tasks.chunks(BATCH_SIZE) {
            let mut batch_tasks = Vec::new();
            
            for (start, end, dex_type) in batch {
                let provider_clone = Arc::clone(&provider);
                let completed_clone = Arc::clone(&completed_ranges);
                let total_found_clone = Arc::clone(&total_pools_found);
                let progress_clone = progress.clone();
                let start = *start;
                let end = *end;
                let dex_type = *dex_type;
                
                let task = tokio::spawn(async move {
                    let result = match dex_type {
                        "UniswapV2" => scan_uniswap_v2_pools_range(provider_clone, start, end).await,
                        "UniswapV3" => scan_uniswap_v3_pools_range(provider_clone, start, end).await,
                        "SushiSwap" => scan_sushiswap_pools_range(provider_clone, start, end).await,
                        "PancakeSwap" => scan_pancakeswap_pools_range(provider_clone, start, end).await,
                        _ => {
                            tracing::warn!("Unknown DEX type: {}", dex_type);
                            Ok(Vec::new())
                        }
                    };
                    
                    let pools = result.unwrap_or_else(|e| {
                        tracing::warn!("Error scanning {} pools in range {}-{}: {}", dex_type, start, end, e);
                        Vec::new()
                    });
                    
                    let completed = completed_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    let found = total_found_clone.fetch_add(pools.len() as u32, Ordering::SeqCst) + pools.len() as u32;
                    
                    // Update progress
                    if let Some(ref cb) = progress_clone {
                        let progress_pct = ((completed as f32 / total_ranges as f32) * 100.0) as u32;
                        let msg = format!("Multi-DEX: {}/{} ranges ({}%) - {} pools found", 
                                        completed, total_ranges, progress_pct, found);
                        cb.lock().unwrap()(msg, found);
                    }
                    
                    tracing::debug!("Completed {} range {}-{}: {} pools", dex_type, start, end, pools.len());
                    pools
                });
                
                batch_tasks.push(task);
            }
            
            // Wait for batch to complete and collect results
            let batch_results = join_all(batch_tasks).await;
            for task_result in batch_results {
                if let Ok(pools) = task_result {
                    all_pools.extend(pools);
                }
            }
        }
        
        // Save all pools to database
        if !all_pools.is_empty() {
            tracing::info!("Adding {} pools to database", all_pools.len());
            if let Some(ref cb) = progress {
                cb.lock().unwrap()(format!("Multi-DEX: Saving {} pools to database...", all_pools.len()), 
                                 all_pools.len() as u32);
            }
            pool_db.add_pools(&all_pools)?;
        }
        
        let total_found = all_pools.len() as u32;
        tracing::info!("Multi-DEX pool scanning complete: {} pools found", total_found);
        
        if let Some(ref cb) = progress {
            cb.lock().unwrap()(format!("Multi-DEX: Complete! {} pools found", total_found), total_found);
        }
        
        Ok(total_found)
    }
}

/// Parse UniswapV3 PoolCreated event using alloy
/// Event: PoolCreated(address indexed token0, address indexed token1, uint24 indexed fee, int24 tickSpacing, address pool)
fn parse_v3_pool_created_log(log: &alloy::rpc::types::eth::Log) -> Result<DexPool> {
    // Topics: [hash, token0, token1, fee]
    // Data: tickSpacing, pool address
    
    if log.topics().len() < 4 {
        return Err(anyhow::anyhow!("Invalid V3 log: not enough topics"));
    }

    // Extract addresses from topics (they're padded to 32 bytes, last 20 bytes are the address)
    let token0_bytes = &log.topics()[1].0[12..32];
    let token1_bytes = &log.topics()[2].0[12..32];
    
    let token0 = Address::from_slice(token0_bytes);
    let token1 = Address::from_slice(token1_bytes);
    
    // Decode log data: (int24 tickSpacing, address pool)
    // Try to decode with proper error handling
    let data = &log.inner.data.data;
    tracing::debug!("V3 log data length: {}, data: {:?}", data.len(), hex::encode(data));
    
    match SolValue::abi_decode(data, false) {
        Ok(decoded) => {
            let (_, pool_address): (i32, Address) = decoded;
            Ok(DexPool {
                address: format!("{:?}", pool_address),
                protocol: "UniswapV3".to_string(),
                token0: Some(format!("{:?}", token0)),
                token1: Some(format!("{:?}", token1)),
                chain_id: 1,
            })
        }
        Err(e) => {
            tracing::debug!("Failed to decode V3 pool data: {}. Data length: {}", e, data.len());
            Err(anyhow::anyhow!("Failed to decode V3 pool: {}", e))
        }
    }
}

/// Parse V2-style PairCreated event using alloy
/// Event: PairCreated(address indexed token0, address indexed token1, address pair, uint)
/// Used by UniswapV2, SushiSwap, PancakeSwap
fn parse_v2_pair_created_log(log: &alloy::rpc::types::eth::Log, protocol: &str) -> Result<DexPool> {
    // Topics: [hash, token0, token1]
    // Data: pair address, count
    
    if log.topics().len() < 3 {
        return Err(anyhow::anyhow!("Invalid V2 log: not enough topics"));
    }

    // Extract addresses from topics (they're padded to 32 bytes, last 20 bytes are the address)
    let token0_bytes = &log.topics()[1].0[12..32];
    let token1_bytes = &log.topics()[2].0[12..32];
    
    let token0 = Address::from_slice(token0_bytes);
    let token1 = Address::from_slice(token1_bytes);
    
    // Decode log data: (address pair, uint count)
    let decoded: (Address, alloy_primitives::U256) = SolValue::abi_decode(&log.inner.data.data, false)?;
    let pair_address = decoded.0;

    Ok(DexPool {
        address: format!("{:?}", pair_address),
        protocol: protocol.to_string(),
        token0: Some(format!("{:?}", token0)),
        token1: Some(format!("{:?}", token1)),
        chain_id: 1,
    })
}

/// Generate block ranges for parallel processing
fn generate_block_ranges(start_block: u64, end_block: u64, chunk_size: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    let mut current = start_block;
    
    while current < end_block {
        let end = std::cmp::min(current + chunk_size - 1, end_block);
        ranges.push((current, end));
        current = end + 1;
    }
    
    ranges
}

/// Scan a specific block range for UniswapV2 pools
async fn scan_uniswap_v2_pools_range(
    provider: Arc<ReqwestProvider>, 
    from_block: u64, 
    to_block: u64
) -> Result<Vec<DexPool>> {
    const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";
    
    let factory_addr = Address::from_str(UNISWAP_V2_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");
    
    let mut pools = Vec::new();
    
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "UniswapV2") {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get UniswapV2 logs for range {}-{}: {}", from_block, to_block, e));
        }
    }
    
    Ok(pools)
}

/// Scan a specific block range for UniswapV3 pools
async fn scan_uniswap_v3_pools_range(
    provider: Arc<ReqwestProvider>, 
    from_block: u64, 
    to_block: u64
) -> Result<Vec<DexPool>> {
    const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
    
    let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PoolCreated(address,address,uint24,int24,address)");
    
    let mut pools = Vec::new();
    
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v3_pool_created_log(&log) {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get UniswapV3 logs for range {}-{}: {}", from_block, to_block, e));
        }
    }
    
    Ok(pools)
}

/// Scan a specific block range for SushiSwap pools
async fn scan_sushiswap_pools_range(
    provider: Arc<ReqwestProvider>, 
    from_block: u64, 
    to_block: u64
) -> Result<Vec<DexPool>> {
    const SUSHISWAP_FACTORY: &str = "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac";
    
    let factory_addr = Address::from_str(SUSHISWAP_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");
    
    let mut pools = Vec::new();
    
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "SushiSwap") {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get SushiSwap logs for range {}-{}: {}", from_block, to_block, e));
        }
    }
    
    Ok(pools)
}

/// Scan a specific block range for PancakeSwap pools
async fn scan_pancakeswap_pools_range(
    provider: Arc<ReqwestProvider>, 
    from_block: u64, 
    to_block: u64
) -> Result<Vec<DexPool>> {
    const PANCAKESWAP_FACTORY: &str = "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362";
    
    let factory_addr = Address::from_str(PANCAKESWAP_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");
    
    let mut pools = Vec::new();
    
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "PancakeSwap") {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to get PancakeSwap logs for range {}-{}: {}", from_block, to_block, e));
        }
    }
    
    Ok(pools)
}
