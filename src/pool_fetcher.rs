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
    /// Uses a rolling window from recent blocks backwards
    /// UniswapV3 Factory: 0x1F98431c8aD98523631AE4a59f267346ea3113F
    pub async fn fetch_uniswap_v3_pools(&self, pool_db: &PoolDatabase, _chain_id: u32) -> Result<u32> {
        const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea3113F";
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
                }
            }

            to_block = from_block;
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV3 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

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

    /// Fetch pools from The Graph subgraph (fallback for historical data)
    /// Uses the Uniswap V2 subgraph to get all known pairs
    pub async fn fetch_from_subgraph(&self, pool_db: &PoolDatabase) -> Result<u32> {
        const SUBGRAPH_URL: &str = "https://api.thegraph.com/subgraphs/name/uniswap/uniswap-v2";
        
        tracing::info!("Fetching UniswapV2 pairs from The Graph subgraph");
        
        let query = r#"
        query {
            pairs(first: 1000, orderBy: txCount, orderDirection: desc) {
                id
            }
        }
        "#;
        
        let client = reqwest::Client::new();
        let response = client
            .post(SUBGRAPH_URL)
            .json(&serde_json::json!({
                "query": query
            }))
            .send()
            .await?;
        
        let result: serde_json::Value = response.json().await?;
        
        let mut all_pools = Vec::new();
        
        if let Some(pairs) = result
            .get("data")
            .and_then(|d| d.get("pairs"))
            .and_then(|p| p.as_array())
        {
            tracing::info!("Fetched {} pairs from subgraph", pairs.len());
            
            for pair in pairs {
                if let Some(address) = pair.get("id").and_then(|id| id.as_str()) {
                    all_pools.push(DexPool {
                        address: address.to_string(),
                        protocol: "UniswapV2".to_string(),
                        token0: None,
                        token1: None,
                        chain_id: 1,
                    });
                }
            }
            
            if !all_pools.is_empty() {
                tracing::info!("Adding {} pairs from subgraph to database", all_pools.len());
                pool_db.add_pools(&all_pools)?;
            }
        } else {
            tracing::warn!("Failed to parse subgraph response: {:?}", result);
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
    let decoded: (i32, Address) = SolValue::abi_decode(&log.inner.data.data, false)?;
    let pool_address = decoded.1;

    Ok(DexPool {
        address: format!("{:?}", pool_address),
        protocol: "UniswapV3".to_string(),
        token0: Some(format!("{:?}", token0)),
        token1: Some(format!("{:?}", token1)),
        chain_id: 1,
    })
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
