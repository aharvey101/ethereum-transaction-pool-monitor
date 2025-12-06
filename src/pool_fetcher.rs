use anyhow::Result;
use alloy::providers::{ProviderBuilder, Provider};
use alloy::rpc::types::eth::Filter;
use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use std::str::FromStr;
use crate::pool_db::{DexPool, PoolDatabase};

/// Pool fetcher that queries the Ethereum node for pool creation events using alloy
pub struct PoolFetcher {
    rpc_url: String,
}

impl PoolFetcher {
    pub fn new(rpc_url: &str) -> Self {
        PoolFetcher {
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Fetch UniswapV3 pools from node by querying PoolCreated events
    /// Fetches pools from the last RECENT_BLOCK_WINDOW blocks for fast startup
    /// UniswapV3 Factory: 0x1F98431c8aD98523631AE4a59f267346ea313100
    pub async fn fetch_uniswap_v3_pools(&self, pool_db: &PoolDatabase, _chain_id: u32) -> Result<u32> {
        const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea313100";
        const BLOCK_WINDOW: u64 = 5000;
        const MAX_WINDOWS: u64 = 40;

        tracing::info!("Fetching UniswapV3 pools from node");

        // Create provider
        let provider = ProviderBuilder::new()
            .on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);

        let mut all_pools = Vec::new();

        // Query from current block backwards
        let mut to_block = current_block;
        for window_idx in 0..MAX_WINDOWS {
            let from_block = if to_block > BLOCK_WINDOW {
                to_block - BLOCK_WINDOW
            } else {
                break;
            };

            tracing::debug!("Fetching UniswapV3 window {} (blocks {}-{})", window_idx, from_block, to_block);

            let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
            tracing::debug!("Factory address parsed: {:?}", factory_addr);
            
            tracing::debug!("Creating V3 filter with event signature: 'PoolCreated(address,address,uint24,int24,address)'");
            let event_filter = Filter::new()
                .from_block(from_block)
                .to_block(to_block)
                .address(vec![factory_addr])
                .event("PoolCreated(address,address,uint24,int24,address)");

            tracing::debug!("V3 filter created successfully");
            tracing::debug!("Calling provider.get_logs()...");

            match provider.get_logs(&event_filter).await {
                Ok(logs) => {
                    tracing::debug!("V3 get_logs succeeded: {} logs found", logs.len());
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
                    // Log the error with more details
                    tracing::warn!("Error details: {:?}", e);
                    // Don't return immediately - continue with next window
                    // The error might be from unconfirmed blocks or other temporary issues
                }
            }

            to_block = from_block;
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV3 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        tracing::info!("UniswapV3 pool fetch complete: {} pools found", all_pools.len());
        Ok(all_pools.len() as u32)
    }

    /// Fetch UniswapV2 pools from node using rolling window approach
    /// UniswapV2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
    pub async fn fetch_uniswap_v2_pools(&self, pool_db: &PoolDatabase, _chain_id: u32) -> Result<u32> {
        const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";
        const BLOCK_WINDOW: u64 = 5000;
        const MAX_WINDOWS: u64 = 40;

        tracing::info!("Fetching UniswapV2 pools from node");

        // Create provider
        let provider = ProviderBuilder::new()
            .on_http(self.rpc_url.parse()?);

        // Get current block
        let current_block = provider.get_block_number().await?;
        tracing::info!("Current block: {}", current_block);

        let mut all_pools = Vec::new();

        // Query from current block backwards
        let mut to_block = current_block;
        for window_idx in 0..MAX_WINDOWS {
            let from_block = if to_block > BLOCK_WINDOW {
                to_block - BLOCK_WINDOW
            } else {
                break;
            };

            tracing::debug!("Fetching UniswapV2 window {} (blocks {}-{})", window_idx, from_block, to_block);

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
                            if let Ok(pool) = parse_v2_pair_created_log(&log) {
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

            to_block = from_block;
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV2 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        Ok(all_pools.len() as u32)
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

/// Parse UniswapV2 PairCreated event using alloy
/// Event: PairCreated(address indexed token0, address indexed token1, address pair, uint)
fn parse_v2_pair_created_log(log: &alloy::rpc::types::eth::Log) -> Result<DexPool> {
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
        protocol: "UniswapV2".to_string(),
        token0: Some(format!("{:?}", token0)),
        token1: Some(format!("{:?}", token1)),
        chain_id: 1,
    })
}
