use crate::pool_db::{DexPool, PoolDatabase};
use alloy::providers::{Provider, ProviderBuilder, ReqwestProvider};
use alloy::rpc::types::eth::Filter;
use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use anyhow::Result;
use futures::future::join_all;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

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
    pub async fn fetch_uniswap_v3_pools(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
    ) -> Result<u32> {
        self.fetch_uniswap_v3_pools_with_progress(pool_db, _chain_id, None)
            .await
    }

    /// Fetch UniswapV4 pools from node by querying PoolCreated events
    /// NOTE: V4 is still in development - this will be updated with actual factory address when deployed
    /// UniswapV4 Factory: TBD (V4 not deployed yet)
    pub async fn fetch_uniswap_v4_pools(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
    ) -> Result<u32> {
        self.fetch_uniswap_v4_pools_with_progress(pool_db, _chain_id, None)
            .await
    }

    /// Fetch UniswapV3 pools with progress callback
    pub async fn fetch_uniswap_v3_pools_with_progress(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
        progress: Option<ProgressCallback>,
    ) -> Result<u32> {
        const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
        const UNISWAP_V3_DEPLOYMENT_BLOCK: u64 = 12_369_739; // V3 deployed on May 5, 2021
        const BLOCK_WINDOW: u64 = 100_000; // Query in 100k block windows for efficiency

        tracing::info!("Fetching UniswapV3 pools from node (scanning all blocks from deployment)");
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("UniswapV3: Initializing...".to_string(), 0);
        }

        // Create provider
        let provider = ProviderBuilder::new().on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);
        tracing::info!(
            "Scanning from V3 deployment block {} to current block {} ({} blocks)",
            UNISWAP_V3_DEPLOYMENT_BLOCK,
            current_block,
            current_block - UNISWAP_V3_DEPLOYMENT_BLOCK
        );

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
            let msg = format!(
                "UniswapV3: Scanning window {}/{} ({}%)",
                window_idx, total_windows, progress_pct
            );
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
                        tracing::debug!(
                            "Found {} PoolCreated events in window {}",
                            logs.len(),
                            window_idx
                        );

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
            cb.lock().unwrap()(
                format!("UniswapV3: Found {} pools, saving...", all_pools.len()),
                all_pools.len() as u32,
            );
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV3 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        tracing::info!(
            "UniswapV3 pool fetch complete: {} pools found",
            all_pools.len()
        );
        if let Some(ref cb) = progress {
            cb.lock().unwrap()(
                format!("UniswapV3: Complete! {} pools found", all_pools.len()),
                all_pools.len() as u32,
            );
        }
        Ok(all_pools.len() as u32)
    }

    /// Fetch UniswapV4 pools with progress callback
    /// NOTE: Uniswap V4 is still in development/testing phase
    /// This method will be updated when V4 is deployed to mainnet
    pub async fn fetch_uniswap_v4_pools_with_progress(
        &self,
        _pool_db: &PoolDatabase,
        _chain_id: u32,
        progress: Option<ProgressCallback>,
    ) -> Result<u32> {
        // NOTE: These are placeholder values - update when V4 is actually deployed
        const _UNISWAP_V4_FACTORY: &str = "0x0000000000000000000000000000000000000000"; // Placeholder - V4 not deployed yet
        const _UNISWAP_V4_DEPLOYMENT_BLOCK: u64 = 0; // Will be set when V4 deploys

        tracing::warn!("Uniswap V4 is not yet deployed on mainnet - skipping V4 pool scanning");
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("UniswapV4: Not deployed yet - skipping".to_string(), 0);
        }

        // TODO: Implement V4 pool scanning when deployed
        // The V4 factory will likely have a different event structure due to hooks
        // and other V4 features, so this will need to be implemented based on
        // the actual V4 factory contract when it's available

        Ok(0) // Return 0 pools found since V4 isn't deployed
    }

    /// Fetch UniswapV2 pools from node using rolling window approach
    /// Scans ALL blocks from V2 deployment (block 10,000,835) to current block
    /// Uses 100k block windows for efficiency
    /// UniswapV2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
    pub async fn fetch_uniswap_v2_pools(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
    ) -> Result<u32> {
        self.fetch_uniswap_v2_pools_with_progress(pool_db, _chain_id, None)
            .await
    }

    /// Fetch UniswapV2 pools with progress callback
    pub async fn fetch_uniswap_v2_pools_with_progress(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
        progress: Option<ProgressCallback>,
    ) -> Result<u32> {
        const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";
        const UNISWAP_V2_DEPLOYMENT_BLOCK: u64 = 10_000_835; // V2 deployed on May 5, 2020
        const BLOCK_WINDOW: u64 = 100_000; // Query in 100k block windows for efficiency

        tracing::info!("Fetching UniswapV2 pools from node (scanning all blocks from deployment)");
        if let Some(ref cb) = progress {
            cb.lock().unwrap()("UniswapV2: Initializing...".to_string(), 0);
        }

        // Create provider
        let provider = ProviderBuilder::new().on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);
        tracing::info!(
            "Scanning from V2 deployment block {} to current block {} ({} blocks)",
            UNISWAP_V2_DEPLOYMENT_BLOCK,
            current_block,
            current_block - UNISWAP_V2_DEPLOYMENT_BLOCK
        );

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
            let msg = format!(
                "UniswapV2: Scanning window {}/{} ({}%)",
                window_idx, total_windows, progress_pct
            );
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
                        tracing::debug!(
                            "Found {} PairCreated events in window {}",
                            logs.len(),
                            window_idx
                        );

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
            cb.lock().unwrap()(
                format!("UniswapV2: Found {} pools, saving...", all_pools.len()),
                all_pools.len() as u32,
            );
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV2 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        tracing::info!(
            "UniswapV2 pool fetch complete: {} pools found",
            all_pools.len()
        );
        if let Some(ref cb) = progress {
            cb.lock().unwrap()(
                format!("UniswapV2: Complete! {} pools found", all_pools.len()),
                all_pools.len() as u32,
            );
        }
        Ok(all_pools.len() as u32)
    }

    /// Parallel fetch method that scans multiple DEX pools concurrently
    /// Uses smaller chunk sizes and parallel processing for maximum speed
    /// Now supports: UniswapV2, UniswapV3, SushiSwap, PancakeSwap, ShibaSwap, FraxSwap, CurveStableswapNG, CurveTwocryptoNG
    pub async fn fetch_pools_parallel(
        &self,
        pool_db: &PoolDatabase,
        _chain_id: u32,
        progress: Option<ProgressCallback>,
    ) -> Result<u32> {
        const CHUNK_SIZE: u64 = 100_000; // Optimized for local node - 100k block windows
        const BATCH_SIZE: usize = 10; // Increased batch size for local node performance

        tracing::info!(
            "Starting parallel multi-DEX pool scanning with chunk size {} and batch size {}",
            CHUNK_SIZE,
            BATCH_SIZE
        );

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
        const SUSHISWAP_START_BLOCK: u64 = 10_794_229; // SushiSwap deployment - September 2020
        const PANCAKESWAP_START_BLOCK: u64 = 15_614_590; // PancakeSwap V2 on Ethereum - December 2022
        const SHIBASWAP_START_BLOCK: u64 = 12_771_744; // ShibaSwap deployment - July 2021
        const FRAXSWAP_START_BLOCK: u64 = 15_463_108; // FraxSwap deployment - September 2022
        const CURVE_STABLESWAP_NG_START_BLOCK: u64 = 17_000_000; // Curve Stableswap-NG deployment - May 2023
        const CURVE_TWOCRYPTO_NG_START_BLOCK: u64 = 18_000_000; // Curve Twocrypto-NG deployment - September 2023

        // Generate block ranges for each DEX protocol
        let uniswap_v2_ranges =
            generate_block_ranges(UNISWAP_V2_START_BLOCK, current_block, CHUNK_SIZE);
        let uniswap_v3_ranges =
            generate_block_ranges(UNISWAP_V3_START_BLOCK, current_block, CHUNK_SIZE);
        let sushiswap_ranges =
            generate_block_ranges(SUSHISWAP_START_BLOCK, current_block, CHUNK_SIZE);
        let pancakeswap_ranges =
            generate_block_ranges(PANCAKESWAP_START_BLOCK, current_block, CHUNK_SIZE);
        let shibaswap_ranges =
            generate_block_ranges(SHIBASWAP_START_BLOCK, current_block, CHUNK_SIZE);
        let fraxswap_ranges =
            generate_block_ranges(FRAXSWAP_START_BLOCK, current_block, CHUNK_SIZE);
        let curve_stableswap_ng_ranges =
            generate_block_ranges(CURVE_STABLESWAP_NG_START_BLOCK, current_block, CHUNK_SIZE);
        let curve_twocrypto_ng_ranges =
            generate_block_ranges(CURVE_TWOCRYPTO_NG_START_BLOCK, current_block, CHUNK_SIZE);

        let total_ranges = uniswap_v2_ranges.len()
            + uniswap_v3_ranges.len()
            + sushiswap_ranges.len()
            + pancakeswap_ranges.len()
            + shibaswap_ranges.len()
            + fraxswap_ranges.len()
            + curve_stableswap_ng_ranges.len()
            + curve_twocrypto_ng_ranges.len();
        let completed_ranges = Arc::new(AtomicU32::new(0));
        let total_pools_found = Arc::new(AtomicU32::new(0));

        tracing::info!("Generated ranges: UniV2={}, UniV3={}, Sushi={}, Pancake={}, Shiba={}, Frax={}, CurveStable={}, CurveTwocrypto={} (total={})",
                       uniswap_v2_ranges.len(), uniswap_v3_ranges.len(),
                      sushiswap_ranges.len(), pancakeswap_ranges.len(),
                      shibaswap_ranges.len(), fraxswap_ranges.len(),
                      curve_stableswap_ng_ranges.len(), curve_twocrypto_ng_ranges.len(), total_ranges);

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

        // Create ShibaSwap tasks
        for (start, end) in shibaswap_ranges {
            all_tasks.push((start, end, "ShibaSwap"));
        }

        // Create FraxSwap tasks
        for (start, end) in fraxswap_ranges {
            all_tasks.push((start, end, "FraxSwap"));
        }

        // Create Curve Stableswap-NG tasks
        for (start, end) in curve_stableswap_ng_ranges {
            all_tasks.push((start, end, "CurveStableswapNG"));
        }

        // Create Curve Twocrypto-NG tasks
        for (start, end) in curve_twocrypto_ng_ranges {
            all_tasks.push((start, end, "CurveTwocryptoNG"));
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
                        "UniswapV2" => {
                            scan_uniswap_v2_pools_range(provider_clone, start, end).await
                        }
                        "UniswapV3" => {
                            scan_uniswap_v3_pools_range(provider_clone, start, end).await
                        }
                        "SushiSwap" => scan_sushiswap_pools_range(provider_clone, start, end).await,
                        "PancakeSwap" => {
                            scan_pancakeswap_pools_range(provider_clone, start, end).await
                        }
                        "ShibaSwap" => scan_shibaswap_pools_range(provider_clone, start, end).await,
                        "FraxSwap" => scan_fraxswap_pools_range(provider_clone, start, end).await,
                        "CurveStableswapNG" => {
                            scan_curve_stableswap_ng_pools_range(provider_clone, start, end).await
                        }
                        "CurveTwocryptoNG" => {
                            scan_curve_twocrypto_ng_pools_range(provider_clone, start, end).await
                        }
                        _ => {
                            tracing::warn!("Unknown DEX type: {}", dex_type);
                            Ok(Vec::new())
                        }
                    };

                    let pools = result.unwrap_or_else(|e| {
                        tracing::warn!(
                            "Error scanning {} pools in range {}-{}: {}",
                            dex_type,
                            start,
                            end,
                            e
                        );
                        Vec::new()
                    });

                    let completed = completed_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    let found = total_found_clone.fetch_add(pools.len() as u32, Ordering::SeqCst)
                        + pools.len() as u32;

                    // Update progress
                    if let Some(ref cb) = progress_clone {
                        let progress_pct =
                            ((completed as f32 / total_ranges as f32) * 100.0) as u32;
                        let msg = format!(
                            "Multi-DEX: {}/{} ranges ({}%) - {} pools found",
                            completed, total_ranges, progress_pct, found
                        );
                        cb.lock().unwrap()(msg, found);
                    }

                    tracing::debug!(
                        "Completed {} range {}-{}: {} pools",
                        dex_type,
                        start,
                        end,
                        pools.len()
                    );
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
                cb.lock().unwrap()(
                    format!("Multi-DEX: Saving {} pools to database...", all_pools.len()),
                    all_pools.len() as u32,
                );
            }
            pool_db.add_pools(&all_pools)?;
        }

        let total_found = all_pools.len() as u32;
        tracing::info!(
            "Multi-DEX pool scanning complete: {} pools found",
            total_found
        );

        if let Some(ref cb) = progress {
            cb.lock().unwrap()(
                format!("Multi-DEX: Complete! {} pools found", total_found),
                total_found,
            );
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
    tracing::debug!(
        "V3 log data length: {}, data: {:?}",
        data.len(),
        hex::encode(data)
    );

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
            tracing::debug!(
                "Failed to decode V3 pool data: {}. Data length: {}",
                e,
                data.len()
            );
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
    let decoded: (Address, alloy_primitives::U256) =
        SolValue::abi_decode(&log.inner.data.data, false)?;
    let pair_address = decoded.0;

    Ok(DexPool {
        address: format!("{:?}", pair_address),
        protocol: protocol.to_string(),
        token0: Some(format!("{:?}", token0)),
        token1: Some(format!("{:?}", token1)),
        chain_id: 1,
    })
}

/// Parse Curve PlainPoolDeployed event
/// Event: PlainPoolDeployed(address[] coins, uint256 A, uint256 fee, address deployer)
/// Note: For proper implementation, we need to get the pool address from contract call or registry
fn parse_curve_plain_pool_deployed_log(log: &alloy::rpc::types::eth::Log) -> Result<DexPool> {
    // For now, create a deterministic pool ID based on transaction
    // In production, you'd query the Curve registry or parse transaction receipt
    let tx_hash = log.transaction_hash.unwrap_or_default();
    let log_index = log.log_index.unwrap_or_default();

    // Create a unique identifier for development/testing
    let pool_address = format!(
        "0x{:08x}{:08x}{:08x}curve",
        tx_hash.0[0] as u32, tx_hash.0[1] as u32, log_index
    );

    Ok(DexPool {
        address: pool_address,
        protocol: "CurveStableswapNG".to_string(),
        token0: None, // Curve pools can have 2-8 tokens
        token1: None,
        chain_id: 1,
    })
}

/// Parse Curve MetaPoolDeployed event
/// Event: MetaPoolDeployed(address coin, address base_pool, uint256 A, uint256 fee, address deployer)
fn parse_curve_meta_pool_deployed_log(log: &alloy::rpc::types::eth::Log) -> Result<DexPool> {
    let tx_hash = log.transaction_hash.unwrap_or_default();
    let log_index = log.log_index.unwrap_or_default();

    // Extract coin from topics if available
    let coin_address = if log.topics().len() > 1 {
        let coin_bytes = &log.topics()[1].0[12..32];
        let coin = Address::from_slice(coin_bytes);
        Some(format!("{:?}", coin))
    } else {
        None
    };

    // Create a unique identifier for development/testing
    let pool_address = format!(
        "0x{:08x}{:08x}{:08x}curve",
        tx_hash.0[0] as u32, tx_hash.0[1] as u32, log_index
    );

    Ok(DexPool {
        address: pool_address,
        protocol: "CurveStableswapNG".to_string(),
        token0: coin_address, // Primary token for metapool
        token1: None,         // Base pool reference
        chain_id: 1,
    })
}

/// Parse Curve CryptoPoolDeployed event
/// Event: CryptoPoolDeployed(address token, address[2] coins, uint256 A, uint256 gamma, ...)
fn parse_curve_crypto_pool_deployed_log(log: &alloy::rpc::types::eth::Log) -> Result<DexPool> {
    let tx_hash = log.transaction_hash.unwrap_or_default();
    let log_index = log.log_index.unwrap_or_default();

    // Extract token addresses from topics/data if available
    let (token0, token1) = if log.topics().len() >= 3 {
        // First topic after event hash should be the token
        let token_bytes = &log.topics()[1].0[12..32];
        let token0 = Address::from_slice(token_bytes);

        // For 2-token pools, we'd need to parse the address[2] from data
        // For now, just capture the first token
        (Some(format!("{:?}", token0)), None)
    } else {
        (None, None)
    };

    // Create a unique identifier for development/testing
    let pool_address = format!(
        "0x{:08x}{:08x}{:08x}curve",
        tx_hash.0[0] as u32, tx_hash.0[1] as u32, log_index
    );

    Ok(DexPool {
        address: pool_address,
        protocol: "CurveTwocryptoNG".to_string(),
        token0: token0,
        token1: token1,
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
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";

    tracing::info!("🔍 Scanning UniswapV2 blocks {}-{}", from_block, to_block);

    let factory_addr = Address::from_str(UNISWAP_V2_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");

    let mut pools = Vec::new();

    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            tracing::info!(
                "📊 UniswapV2 blocks {}-{}: found {} logs",
                from_block,
                to_block,
                logs.len()
            );
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "UniswapV2") {
                    tracing::info!("✅ UniswapV2 pool found: {}", pool.address);
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "❌ UniswapV2 error blocks {}-{}: {}",
                from_block,
                to_block,
                e
            );
            return Err(anyhow::anyhow!(
                "Failed to get UniswapV2 logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    tracing::info!(
        "✅ UniswapV2 blocks {}-{}: {} pools total",
        from_block,
        to_block,
        pools.len()
    );
    Ok(pools)
}

async fn scan_uniswap_v3_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";

    tracing::info!("🔍 Scanning UniswapV3 blocks {}-{}", from_block, to_block);

    let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PoolCreated(address,address,uint24,int24,address)");

    let mut pools = Vec::new();

    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            tracing::info!(
                "📊 UniswapV3 blocks {}-{}: found {} logs",
                from_block,
                to_block,
                logs.len()
            );
            for log in logs {
                if let Ok(pool) = parse_v3_pool_created_log(&log) {
                    tracing::info!("✅ UniswapV3 pool found: {}", pool.address);
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "❌ UniswapV3 error blocks {}-{}: {}",
                from_block,
                to_block,
                e
            );
            return Err(anyhow::anyhow!(
                "Failed to get UniswapV3 logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    tracing::info!(
        "✅ UniswapV3 blocks {}-{}: {} pools total",
        from_block,
        to_block,
        pools.len()
    );
    Ok(pools)
}

/// Scan a specific block range for SushiSwap pools
async fn scan_sushiswap_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
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
            return Err(anyhow::anyhow!(
                "Failed to get SushiSwap logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    Ok(pools)
}

/// Scan a specific block range for PancakeSwap pools
async fn scan_pancakeswap_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
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
            return Err(anyhow::anyhow!(
                "Failed to get PancakeSwap logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    Ok(pools)
}

/// Scan a specific block range for ShibaSwap pools
async fn scan_shibaswap_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const SHIBASWAP_FACTORY: &str = "0x115934131916C8b277DD010Ee02de363c09d037c";

    let factory_addr = Address::from_str(SHIBASWAP_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");

    let mut pools = Vec::new();

    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "ShibaSwap") {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to get ShibaSwap logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    Ok(pools)
}

/// Scan a specific block range for FraxSwap pools
async fn scan_fraxswap_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const FRAXSWAP_FACTORY: &str = "0x43eC799eAdd63848443E2347C49f5f52e8Fe0F6f";

    let factory_addr = Address::from_str(FRAXSWAP_FACTORY)?;
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PairCreated(address,address,address,uint256)");

    let mut pools = Vec::new();

    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_v2_pair_created_log(&log, "FraxSwap") {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to get FraxSwap logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            ));
        }
    }

    Ok(pools)
}

/// Scan a specific block range for Curve Stableswap-NG pools
/// Handles both PlainPoolDeployed and MetaPoolDeployed events
async fn scan_curve_stableswap_ng_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const CURVE_STABLESWAP_NG_FACTORY: &str = "0x6A8cbed756804B16E05E741eDaBd5cB544AE21bf";

    let factory_addr = Address::from_str(CURVE_STABLESWAP_NG_FACTORY)?;
    let mut pools = Vec::new();

    // Scan for PlainPoolDeployed events
    let plain_pool_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PlainPoolDeployed(address[],uint256,uint256,address)");

    match provider.get_logs(&plain_pool_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_curve_plain_pool_deployed_log(&log) {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "Error getting Curve PlainPoolDeployed logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            );
        }
    }

    // Scan for MetaPoolDeployed events
    let meta_pool_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("MetaPoolDeployed(address,address,uint256,uint256,address)");

    match provider.get_logs(&meta_pool_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_curve_meta_pool_deployed_log(&log) {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "Error getting Curve MetaPoolDeployed logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            );
        }
    }

    Ok(pools)
}

/// Scan a specific block range for Curve Twocrypto-NG pools
/// Handles CryptoPoolDeployed events for 2-token volatile pools
async fn scan_curve_twocrypto_ng_pools_range(
    provider: Arc<ReqwestProvider>,
    from_block: u64,
    to_block: u64,
) -> Result<Vec<DexPool>> {
    const CURVE_TWOCRYPTO_NG_FACTORY: &str = "0x98EE851a00abeE0d95D08cF4CA2BdCE32aeaAF7F";

    let factory_addr = Address::from_str(CURVE_TWOCRYPTO_NG_FACTORY)?;
    let mut pools = Vec::new();

    // CryptoPoolDeployed(address token, address[2] coins, uint256 A, uint256 gamma, ...)
    let crypto_pool_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("CryptoPoolDeployed(address,address[2],uint256,uint256,uint256,uint256,uint256,uint256,uint256,uint256,uint256,uint256,address)");

    match provider.get_logs(&crypto_pool_filter).await {
        Ok(logs) => {
            for log in logs {
                if let Ok(pool) = parse_curve_crypto_pool_deployed_log(&log) {
                    pools.push(pool);
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                "Error getting Curve CryptoPoolDeployed logs for range {}-{}: {}",
                from_block,
                to_block,
                e
            );
        }
    }

    Ok(pools)
}
