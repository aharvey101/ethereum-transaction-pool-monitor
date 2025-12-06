use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use crate::pool_db::{DexPool, PoolDatabase};

/// Pool fetcher that queries the Ethereum node for pool creation events
pub struct PoolFetcher {
    rpc_url: String,
    http_client: Arc<reqwest::Client>,
}

impl PoolFetcher {
    pub fn new(rpc_url: &str) -> Self {
        PoolFetcher {
            rpc_url: rpc_url.to_string(),
            http_client: Arc::new(reqwest::Client::new()),
        }
    }

    /// Fetch UniswapV3 pools from node by querying PoolCreated events
    /// UniswapV3 Factory: 0x1F98431c8aD98523631AE4a59f267346ea3113F
    /// Event: PoolCreated(indexed uint24 fee, indexed int24 tickSpacing, indexed address pool, uint256 count)
    pub async fn fetch_uniswap_v3_pools(&self, pool_db: &PoolDatabase, chain_id: u32) -> Result<u32> {
        const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea3113F";
        // PoolCreated event signature: keccak256("PoolCreated(uint24,int24,address,uint256)")
        const POOL_CREATED_TOPIC: &str = "0x783cca1c0412dd0d695e784568c96da80eb3cc59c1b71e2e53d63fe50b912e4f";
        const BLOCK_CHUNK: u64 = 50000; // Query in 50k block chunks

        tracing::info!("Fetching UniswapV3 pools from node for chain_id: {}", chain_id);

        let mut all_pools = Vec::new();
        let mut from_block = 12369739u64; // Uniswap V3 launch block
        let mut chunk_count = 0;

        loop {
            let to_block = from_block + BLOCK_CHUNK;
            let from_hex = format!("0x{:x}", from_block);
            let to_hex = format!("0x{:x}", to_block);

            chunk_count += 1;
            tracing::debug!("Fetching chunk {} (blocks {}-{})", chunk_count, from_block, to_block);

            let response = self.query_logs(
                POOL_CREATED_TOPIC,
                Some(UNISWAP_V3_FACTORY),
                &from_hex,
                &to_hex,
            ).await?;

            if !response.is_empty() {
                tracing::debug!("Found {} PoolCreated events in chunk {}", response.len(), chunk_count);

                for log in response.iter() {
                    if let Ok(pool) = parse_pool_created_event(log) {
                        tracing::debug!("Found UniswapV3 pool: {}", pool.address);
                        all_pools.push(pool);
                    }
                }
            }

            // Stop after 20 chunks or if we've gone past current block
            if chunk_count >= 20 {
                tracing::info!("Reached chunk limit for UniswapV3");
                break;
            }

            from_block = to_block + 1;
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV3 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        Ok(all_pools.len() as u32)
    }

    /// Fetch UniswapV2 pools from node
    /// UniswapV2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
    pub async fn fetch_uniswap_v2_pools(&self, pool_db: &PoolDatabase, chain_id: u32) -> Result<u32> {
        const UNISWAP_V2_FACTORY: &str = "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f";
        // PairCreated event signature
        const PAIR_CREATED_TOPIC: &str = "0x0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9";
        const BLOCK_CHUNK: u64 = 50000; // Query in 50k block chunks

        tracing::info!("Fetching UniswapV2 pools from node for chain_id: {}", chain_id);

        let mut all_pools = Vec::new();
        let mut from_block = 10000835u64; // Uniswap V2 launch block
        let mut chunk_count = 0;

        loop {
            let to_block = from_block + BLOCK_CHUNK;
            let from_hex = format!("0x{:x}", from_block);
            let to_hex = format!("0x{:x}", to_block);

            chunk_count += 1;
            tracing::debug!("Fetching chunk {} (blocks {}-{})", chunk_count, from_block, to_block);

            let response = self.query_logs(
                PAIR_CREATED_TOPIC,
                Some(UNISWAP_V2_FACTORY),
                &from_hex,
                &to_hex,
            ).await?;

            if !response.is_empty() {
                tracing::debug!("Found {} PairCreated events in chunk {}", response.len(), chunk_count);

                for log in response.iter() {
                    if let Ok(pool) = parse_uniswap_v2_event(log) {
                        tracing::debug!("Found UniswapV2 pool: {}", pool.address);
                        all_pools.push(pool);
                    }
                }
            }

            // Stop after 20 chunks
            if chunk_count >= 20 {
                tracing::info!("Reached chunk limit for UniswapV2");
                break;
            }

            from_block = to_block + 1;
        }

        if !all_pools.is_empty() {
            tracing::info!("Adding {} UniswapV2 pools to database", all_pools.len());
            pool_db.add_pools(&all_pools)?;
        }

        Ok(all_pools.len() as u32)
    }

    /// Query logs from the node
    async fn query_logs(
        &self,
        topic: &str,
        address: Option<&str>,
        from_block: &str,
        to_block: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let mut filter_obj = serde_json::Map::new();
        
        if let Some(addr) = address {
            filter_obj.insert("address".to_string(), json!(addr));
        }
        
        filter_obj.insert("topics".to_string(), json!([topic]));
        filter_obj.insert("fromBlock".to_string(), json!(from_block));
        filter_obj.insert("toBlock".to_string(), json!(to_block));

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "eth_getLogs",
            "params": [serde_json::Value::Object(filter_obj)],
            "id": 1,
        });

        tracing::debug!("Querying logs: topic={}, address={:?}, blocks=[{}, {}]",
            topic, address, from_block, to_block);

        let response = self.http_client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;

        if let Some(logs) = result.get("result").and_then(|r| r.as_array()) {
            tracing::debug!("Got {} logs from node", logs.len());
            Ok(logs.clone())
        } else if let Some(error) = result.get("error") {
            tracing::warn!("RPC error: {}", error);
            Ok(Vec::new())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Parse UniswapV3 PoolCreated event
/// Event: PoolCreated(indexed uint24 fee, indexed int24 tickSpacing, indexed address pool, uint256 count)
fn parse_pool_created_event(log: &serde_json::Value) -> Result<DexPool> {
    // The pool address is in the indexed topics (topics[3])
    let pool_address = log
        .get("topics")
        .and_then(|t| t.get(3))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing pool address in event"))?;

    // Remove 0x prefix and pad to address if needed
    let pool_addr = if pool_address.len() > 42 {
        format!("0x{}", &pool_address[26..]) // Last 20 bytes
    } else {
        pool_address.to_string()
    };

    Ok(DexPool {
        address: pool_addr,
        protocol: "UniswapV3".to_string(),
        token0: None,
        token1: None,
        chain_id: 1,
    })
}

/// Parse UniswapV2 PairCreated event
/// Event: PairCreated(indexed address token0, indexed address token1, address pair, uint)
fn parse_uniswap_v2_event(log: &serde_json::Value) -> Result<DexPool> {
    // The pair address is in the data field (bytes 0-32)
    let data = log
        .get("data")
        .and_then(|d| d.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing data in event"))?;

    // data format: 0x{pair_address (20 bytes)}...
    // pair is at offset 24-64 (after 0x and 32 byte padding)
    if data.len() >= 66 {
        let pair_addr = format!("0x{}", &data[26..66]);
        
        Ok(DexPool {
            address: pair_addr,
            protocol: "UniswapV2".to_string(),
            token0: None,
            token1: None,
            chain_id: 1,
        })
    } else {
        Err(anyhow::anyhow!("Invalid event data length"))
    }
}
