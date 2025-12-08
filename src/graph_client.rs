use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::pool_db::{DexPool, PoolDatabase};

/// The Graph API client for querying Uniswap subgraphs
pub struct GraphClient {
    client: reqwest::Client,
    query_count: u32,
    api_key: String,
}

/// UniswapV2 Pair from The Graph
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphV2Pair {
    pub id: String,
    pub token0: GraphToken,
    pub token1: GraphToken,
}

/// UniswapV3 Pool from The Graph  
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphV3Pool {
    pub id: String,
    pub token0: GraphToken,
    pub token1: GraphToken,
    #[serde(rename = "feeTier")]
    pub fee_tier: String,
}

/// UniswapV4 Pool from The Graph
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphV4Pool {
    pub id: String,
    pub token0: GraphToken,
    pub token1: GraphToken,
}

/// Token info from The Graph
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphToken {
    pub id: String,
    pub symbol: String,
    pub name: String,
}

/// Response wrapper for GraphQL queries
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphResponse<T> {
    pub data: Option<T>,
    pub errors: Option<Vec<GraphError>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphError {
    pub message: String,
}

/// UniswapV2 pairs response
#[derive(Debug, Serialize, Deserialize)]
pub struct V2PairsResponse {
    pub pairs: Vec<GraphV2Pair>,
}

/// SushiSwap Pair from The Graph
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphSushiPair {
    pub id: String,
    pub token0: GraphToken,
    pub token1: GraphToken,
}

/// Curve Pool from The Graph
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphCurvePool {
    pub id: String,
    pub coins: Vec<GraphToken>,
    #[serde(rename = "name")]
    pub pool_name: Option<String>,
}

/// SushiSwap pairs response
#[derive(Debug, Serialize, Deserialize)]
pub struct SushiPairsResponse {
    pub pairs: Vec<GraphSushiPair>,
}

/// UniswapV3 pools response  
#[derive(Debug, Serialize, Deserialize)]
pub struct V3PoolsResponse {
    pub pools: Vec<GraphV3Pool>,
}

/// UniswapV4 pools response
#[derive(Debug, Serialize, Deserialize)]
pub struct V4PoolsResponse {
    pub pools: Vec<GraphV4Pool>,
}

/// Curve pools response  
#[derive(Debug, Serialize, Deserialize)]
pub struct CurvePoolsResponse {
    pub pools: Vec<GraphCurvePool>,
}

impl GraphClient {
    /// Create a new Graph client with API key
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            query_count: 0,
            api_key,
        }
    }

    /// Get the current query count
    pub fn query_count(&self) -> u32 {
        self.query_count
    }

    /// Fetch all UniswapV2 pairs from The Graph with progressive database writes (resumable)
    pub async fn fetch_all_v2_pairs_with_db(&mut self, pool_db: &PoolDatabase) -> Result<u32> {
        // Correct Uniswap V2 subgraph ID
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/A3Np3RQbaBA6oKJgiwDJeo5T3zrYfGHPWFYayMwtNDum",
            self.api_key
        );
        
        println!("🔍 Fetching UniswapV2 pairs from The Graph Network...");
        println!("🔗 Using correct V2 subgraph: A3Np3RQbaBA6oKJgiwDJeo5T3zrYfGHPWFYayMwtNDum");
        
        // Check if we're resuming or starting fresh
        let (mut total_pairs, mut last_id) = match pool_db.get_latest_pool_id("UniswapV2")? {
            Some(latest_id) => {
                let existing_count = pool_db.get_pool_count_by_protocol("UniswapV2")?;
                println!("📊 Resuming V2 collection from pool ID: {}", latest_id);
                println!("📊 Already collected: {} V2 pairs", existing_count);
                (existing_count, latest_id)
            }
            None => {
                println!("📊 Starting fresh V2 collection (no existing pools found)");
                (0, String::new())
            }
        };

        // Check if already complete
        if pool_db.is_collection_complete("UniswapV2")? {
            println!("✅ V2 collection appears complete ({} pools). Skipping.", total_pairs);
            return Ok(total_pairs);
        }
        
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pairs(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pairs(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            // Retry logic for batch-level errors
            let mut batch_retry_count = 0;
            const MAX_BATCH_RETRIES: u32 = 3;
            
            let pairs = loop {
                match self.execute_query_internal::<V2PairsResponse>(&endpoint, &query).await {
                    Ok(response) => break response.pairs,
                    Err(e) => {
                        batch_retry_count += 1;
                        if batch_retry_count >= MAX_BATCH_RETRIES {
                            println!("❌ Batch {} failed after {} retries: {}", batch_count, MAX_BATCH_RETRIES, e);
                            println!("⏭️  Skipping this batch and continuing...");
                            break Vec::new(); // Return empty to skip this batch
                        }
                        println!("⚠️  Batch {} error (attempt {}): {}, retrying in {}s...", 
                                 batch_count, batch_retry_count, e, batch_retry_count * 3);
                        tokio::time::sleep(tokio::time::Duration::from_secs((batch_retry_count * 3) as u64)).await;
                    }
                }
            };
            
            if pairs.is_empty() {
                if batch_retry_count >= MAX_BATCH_RETRIES {
                    println!("⏭️  Continuing to next batch after skipping failed batch...");
                    continue; // Skip this batch but continue collection
                } else {
                    println!("✅ No more V2 pairs found - collection complete!");
                    break;
                }
            }
            
            // Convert to DexPool and write to database immediately
            let mut pools = Vec::new();
            for pair in &pairs {
                if let Ok(pool) = Self::graph_v2_pair_to_dex_pool(pair) {
                    pools.push(pool);
                }
            }
            
            // Write batch to database with retry logic
            let mut db_retry_count = 0;
            loop {
                match pool_db.add_pools(&pools) {
                    Ok(_) => {
                        total_pairs += pools.len() as u32;
                        println!("📊 Batch {}: Retrieved {} V2 pairs, wrote to DB (Total: {})", 
                                 batch_count, pairs.len(), total_pairs);
                        break;
                    }
                    Err(e) => {
                        db_retry_count += 1;
                        if db_retry_count >= 3 {
                            println!("❌ Database write failed after 3 retries: {}", e);
                            return Err(e.into());
                        }
                        println!("⚠️  Database write error (attempt {}): {}, retrying...", db_retry_count, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                }
            }
            
            if let Some(last_pair) = pairs.last() {
                last_id = last_pair.id.clone();
            }
            
            // Rate limiting delay
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(total_pairs)
    }

    /// Fetch all UniswapV3 pools from The Graph with progressive database writes (resumable)
    pub async fn fetch_all_v3_pools_with_db(&mut self, pool_db: &PoolDatabase) -> Result<u32> {
        // Correct Uniswap V3 subgraph ID
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/5zvR82QoaXYFyDEKLZ9t6v9adgnptxYpKpSbxtgVENFV",
            self.api_key
        );
        
        println!("🔍 Fetching UniswapV3 pools from The Graph Network...");
        println!("🔗 Using correct V3 subgraph: 5zvR82QoaXYFyDEKLZ9t6v9adgnptxYpKpSbxtgVENFV");
        
        // Check if we're resuming or starting fresh
        let (mut total_pools, mut last_id) = match pool_db.get_latest_pool_id("UniswapV3")? {
            Some(latest_id) => {
                let existing_count = pool_db.get_pool_count_by_protocol("UniswapV3")?;
                println!("📊 Resuming V3 collection from pool ID: {}", latest_id);
                println!("📊 Already collected: {} V3 pools", existing_count);
                (existing_count, latest_id)
            }
            None => {
                println!("📊 Starting fresh V3 collection (no existing pools found)");
                (0, String::new())
            }
        };

        // Check if already complete
        if pool_db.is_collection_complete("UniswapV3")? {
            println!("✅ V3 collection appears complete ({} pools). Skipping.", total_pools);
            return Ok(total_pools);
        }
        
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pools(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                            feeTier
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pools(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                            feeTier
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            // Retry logic for batch-level errors
            let mut batch_retry_count = 0;
            const MAX_BATCH_RETRIES: u32 = 3;
            
            let pools_data = loop {
                match self.execute_query_internal::<V3PoolsResponse>(&endpoint, &query).await {
                    Ok(response) => break response.pools,
                    Err(e) => {
                        batch_retry_count += 1;
                        if batch_retry_count >= MAX_BATCH_RETRIES {
                            println!("❌ Batch {} failed after {} retries: {}", batch_count, MAX_BATCH_RETRIES, e);
                            println!("⏭️  Skipping this batch and continuing...");
                            break Vec::new(); // Return empty to skip this batch
                        }
                        println!("⚠️  Batch {} error (attempt {}): {}, retrying in {}s...", 
                                 batch_count, batch_retry_count, e, batch_retry_count * 3);
                        tokio::time::sleep(tokio::time::Duration::from_secs((batch_retry_count * 3) as u64)).await;
                    }
                }
            };
            
            if pools_data.is_empty() {
                if batch_retry_count >= MAX_BATCH_RETRIES {
                    println!("⏭️  Continuing to next batch after skipping failed batch...");
                    continue; // Skip this batch but continue collection
                } else {
                    println!("✅ No more V3 pools found - collection complete!");
                    break;
                }
            }
            
            // Convert to DexPool and write to database immediately
            let mut pools = Vec::new();
            for pool in &pools_data {
                if let Ok(pool) = Self::graph_v3_pool_to_dex_pool(pool) {
                    pools.push(pool);
                }
            }
            
            // Write batch to database with retry logic
            let mut db_retry_count = 0;
            loop {
                match pool_db.add_pools(&pools) {
                    Ok(_) => {
                        total_pools += pools.len() as u32;
                        println!("📊 Batch {}: Retrieved {} V3 pools, wrote to DB (Total: {})", 
                                 batch_count, pools_data.len(), total_pools);
                        break;
                    }
                    Err(e) => {
                        db_retry_count += 1;
                        if db_retry_count >= 3 {
                            println!("❌ Database write failed after 3 retries: {}", e);
                            return Err(e.into());
                        }
                        println!("⚠️  Database write error (attempt {}): {}, retrying...", db_retry_count, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                }
            }
            
            if let Some(last_pool) = pools_data.last() {
                last_id = last_pool.id.clone();
            }
            
            // Rate limiting delay
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(total_pools)
    }

    /// Fetch all SushiSwap pairs from The Graph with progressive database writes (resumable)
    pub async fn fetch_all_sushi_pairs_with_db(&mut self, pool_db: &PoolDatabase, subgraph_id: &str) -> Result<u32> {
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/{}",
            self.api_key, subgraph_id
        );
        
        println!("🔍 Fetching SushiSwap pairs from The Graph Network...");
        println!("🔗 Using SushiSwap subgraph: {}", subgraph_id);
        
        // Check if we're resuming or starting fresh
        let (mut total_pairs, mut last_id) = match pool_db.get_latest_pool_id("SushiSwap")? {
            Some(latest_id) => {
                let existing_count = pool_db.get_pool_count_by_protocol("SushiSwap")?;
                println!("📊 Resuming SushiSwap collection from pool ID: {}", latest_id);
                println!("📊 Already collected: {} SushiSwap pairs", existing_count);
                (existing_count, latest_id)
            }
            None => {
                println!("📊 Starting fresh SushiSwap collection (no existing pools found)");
                (0, String::new())
            }
        };

        // Check if already complete
        if pool_db.is_collection_complete("SushiSwap")? {
            println!("✅ SushiSwap collection appears complete ({} pools). Skipping.", total_pairs);
            return Ok(total_pairs);
        }
        
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pairs(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pairs(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            // Retry logic for batch-level errors
            let mut batch_retry_count = 0;
            const MAX_BATCH_RETRIES: u32 = 3;
            
            let pairs = loop {
                match self.execute_query_internal::<SushiPairsResponse>(&endpoint, &query).await {
                    Ok(response) => break response.pairs,
                    Err(e) => {
                        batch_retry_count += 1;
                        if batch_retry_count >= MAX_BATCH_RETRIES {
                            println!("❌ Batch {} failed after {} retries: {}", batch_count, MAX_BATCH_RETRIES, e);
                            println!("⏭️  Skipping this batch and continuing...");
                            break Vec::new();
                        }
                        println!("⚠️  Batch {} error (attempt {}): {}, retrying in {}s...", 
                                 batch_count, batch_retry_count, e, batch_retry_count * 3);
                        tokio::time::sleep(tokio::time::Duration::from_secs((batch_retry_count * 3) as u64)).await;
                    }
                }
            };
            
            if pairs.is_empty() {
                if batch_retry_count >= MAX_BATCH_RETRIES {
                    println!("⏭️  Continuing to next batch after skipping failed batch...");
                    continue;
                } else {
                    println!("✅ No more SushiSwap pairs found - collection complete!");
                    break;
                }
            }
            
            // Convert to DexPool and write to database immediately
            let mut pools = Vec::new();
            for pair in &pairs {
                if let Ok(pool) = Self::graph_sushi_pair_to_dex_pool(pair) {
                    pools.push(pool);
                }
            }
            
            // Write batch to database with retry logic
            let mut db_retry_count = 0;
            loop {
                match pool_db.add_pools(&pools) {
                    Ok(_) => {
                        total_pairs += pools.len() as u32;
                        println!("📊 Batch {}: Retrieved {} SushiSwap pairs, wrote to DB (Total: {})", 
                                 batch_count, pairs.len(), total_pairs);
                        break;
                    }
                    Err(e) => {
                        db_retry_count += 1;
                        if db_retry_count >= 3 {
                            println!("❌ Database write failed after 3 retries: {}", e);
                            return Err(e.into());
                        }
                        println!("⚠️  Database write error (attempt {}): {}, retrying...", db_retry_count, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                }
            }
            
            if let Some(last_pair) = pairs.last() {
                last_id = last_pair.id.clone();
            }
            
            // Rate limiting delay
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(total_pairs)
    }

    /// Fetch all Curve pools from The Graph with progressive database writes (resumable)
    pub async fn fetch_all_curve_pools_with_db(&mut self, pool_db: &PoolDatabase, subgraph_id: &str) -> Result<u32> {
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/{}",
            self.api_key, subgraph_id
        );
        
        println!("🔍 Fetching Curve pools from The Graph Network...");
        println!("🔗 Using Curve subgraph: {}", subgraph_id);
        
        // Check if we're resuming or starting fresh
        let (mut total_pools, mut last_id) = match pool_db.get_latest_pool_id("Curve")? {
            Some(latest_id) => {
                let existing_count = pool_db.get_pool_count_by_protocol("Curve")?;
                println!("📊 Resuming Curve collection from pool ID: {}", latest_id);
                println!("📊 Already collected: {} Curve pools", existing_count);
                (existing_count, latest_id)
            }
            None => {
                println!("📊 Starting fresh Curve collection (no existing pools found)");
                (0, String::new())
            }
        };

        // Check if already complete (lower threshold for Curve as it has fewer pools)
        if total_pools >= 5_000 { // Curve has fewer pools than Uniswap
            println!("✅ Curve collection appears complete ({} pools). Skipping.", total_pools);
            return Ok(total_pools);
        }
        
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pools(first: {}) {{
                            id
                            coins {{ id symbol name }}
                            name
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pools(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            coins {{ id symbol name }}
                            name
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            // Retry logic for batch-level errors
            let mut batch_retry_count = 0;
            const MAX_BATCH_RETRIES: u32 = 3;
            
            let pools_data = loop {
                match self.execute_query_internal::<CurvePoolsResponse>(&endpoint, &query).await {
                    Ok(response) => break response.pools,
                    Err(e) => {
                        batch_retry_count += 1;
                        if batch_retry_count >= MAX_BATCH_RETRIES {
                            println!("❌ Batch {} failed after {} retries: {}", batch_count, MAX_BATCH_RETRIES, e);
                            println!("⏭️  Skipping this batch and continuing...");
                            break Vec::new();
                        }
                        println!("⚠️  Batch {} error (attempt {}): {}, retrying in {}s...", 
                                 batch_count, batch_retry_count, e, batch_retry_count * 3);
                        tokio::time::sleep(tokio::time::Duration::from_secs((batch_retry_count * 3) as u64)).await;
                    }
                }
            };
            
            if pools_data.is_empty() {
                if batch_retry_count >= MAX_BATCH_RETRIES {
                    println!("⏭️  Continuing to next batch after skipping failed batch...");
                    continue;
                } else {
                    println!("✅ No more Curve pools found - collection complete!");
                    break;
                }
            }
            
            // Convert to DexPool and write to database immediately
            let mut pools = Vec::new();
            for pool in &pools_data {
                if let Ok(pool) = Self::graph_curve_pool_to_dex_pool(pool) {
                    pools.push(pool);
                }
            }
            
            // Write batch to database with retry logic
            let mut db_retry_count = 0;
            loop {
                match pool_db.add_pools(&pools) {
                    Ok(_) => {
                        total_pools += pools.len() as u32;
                        println!("📊 Batch {}: Retrieved {} Curve pools, wrote to DB (Total: {})", 
                                 batch_count, pools_data.len(), total_pools);
                        break;
                    }
                    Err(e) => {
                        db_retry_count += 1;
                        if db_retry_count >= 3 {
                            println!("❌ Database write failed after 3 retries: {}", e);
                            return Err(e.into());
                        }
                        println!("⚠️  Database write error (attempt {}): {}, retrying...", db_retry_count, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                }
            }
            
            if let Some(last_pool) = pools_data.last() {
                last_id = last_pool.id.clone();
            }
            
            // Rate limiting delay
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(total_pools)
    }

    /// Fetch all UniswapV4 pools from The Graph with progressive database writes (resumable)
    pub async fn fetch_all_v4_pools_with_db(&mut self, pool_db: &PoolDatabase) -> Result<u32> {
        // UniswapV4 subgraph ID
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/DiYPVdygkfjDWhbxGSqAQxwBKmfKnkWQojqeM2rkLb3G",
            self.api_key
        );
        
        println!("🔍 Fetching UniswapV4 pools from The Graph Network...");
        println!("🔗 Using V4 subgraph: DiYPVdygkfjDWhbxGSqAQxwBKmfKnkWQojqeM2rkLb3G");
        
        // Check if we're resuming or starting fresh
        let (mut total_pools, mut last_id) = match pool_db.get_latest_pool_id("UniswapV4")? {
            Some(latest_id) => {
                let existing_count = pool_db.get_pool_count_by_protocol("UniswapV4")?;
                println!("📊 Resuming V4 collection from pool ID: {}", latest_id);
                println!("📊 Already collected: {} V4 pools", existing_count);
                (existing_count, latest_id)
            }
            None => {
                println!("📊 Starting fresh V4 collection (no existing pools found)");
                (0, String::new())
            }
        };

        // Check if already complete
        if pool_db.is_collection_complete("UniswapV4")? {
            println!("✅ V4 collection appears complete ({} pools). Skipping.", total_pools);
            return Ok(total_pools);
        }
        
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pools(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pools(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            // Retry logic for batch-level errors
            let mut batch_retry_count = 0;
            const MAX_BATCH_RETRIES: u32 = 3;
            
            let pools_data = loop {
                match self.execute_query_internal::<V4PoolsResponse>(&endpoint, &query).await {
                    Ok(response) => break response.pools,
                    Err(e) => {
                        batch_retry_count += 1;
                        if batch_retry_count >= MAX_BATCH_RETRIES {
                            println!("❌ Batch {} failed after {} retries: {}", batch_count, MAX_BATCH_RETRIES, e);
                            println!("⏭️  Skipping this batch and continuing...");
                            break Vec::new();
                        }
                        println!("⚠️  Batch {} error (attempt {}): {}, retrying in {}s...", 
                                 batch_count, batch_retry_count, e, batch_retry_count * 3);
                        tokio::time::sleep(tokio::time::Duration::from_secs((batch_retry_count * 3) as u64)).await;
                    }
                }
            };
            
            if pools_data.is_empty() {
                if batch_retry_count >= MAX_BATCH_RETRIES {
                    println!("⏭️  Continuing to next batch after skipping failed batch...");
                    continue;
                } else {
                    println!("✅ No more V4 pools found - collection complete!");
                    break;
                }
            }
            
            // Convert to DexPool and write to database immediately
            let mut pools = Vec::new();
            for pool in &pools_data {
                if let Ok(pool) = Self::graph_v4_pool_to_dex_pool(pool) {
                    pools.push(pool);
                }
            }
            
            // Write batch to database with retry logic
            let mut db_retry_count = 0;
            loop {
                match pool_db.add_pools(&pools) {
                    Ok(_) => {
                        total_pools += pools.len() as u32;
                        println!("📊 Batch {}: Retrieved {} V4 pools, wrote to DB (Total: {})", 
                                 batch_count, pools_data.len(), total_pools);
                        break;
                    }
                    Err(e) => {
                        db_retry_count += 1;
                        if db_retry_count >= 3 {
                            println!("❌ Database write failed after 3 retries: {}", e);
                            return Err(e.into());
                        }
                        println!("⚠️  Database write error (attempt {}): {}, retrying...", db_retry_count, e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                    }
                }
            }
            
            if let Some(last_pool) = pools_data.last() {
                last_id = last_pool.id.clone();
            }
            
            // Rate limiting delay
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(total_pools)
    }

    async fn try_fetch_v2_from_endpoint(&mut self, endpoint: &str) -> Result<Vec<GraphV2Pair>> {
        
        let mut all_pairs = Vec::new();
        let mut last_id = String::new();
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pairs(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pairs(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            let response = self.execute_query_internal::<V2PairsResponse>(&endpoint, &query).await?;
            let pairs = response.pairs;
            
            println!("📊 Batch {}: Retrieved {} V2 pairs (Total: {})", 
                     batch_count, pairs.len(), all_pairs.len() + pairs.len());
            
            if pairs.is_empty() {
                break;
            }
            
            if let Some(last_pair) = pairs.last() {
                last_id = last_pair.id.clone();
            }
            
            all_pairs.extend(pairs);
            
            // Longer delay to avoid rate limiting - The Graph has strict limits
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(all_pairs)
    }

    /// Fetch all UniswapV3 pools from The Graph
    pub async fn fetch_all_v3_pools(&mut self) -> Result<Vec<GraphV3Pool>> {
        // Correct Uniswap V3 subgraph ID
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/5zvR82QoaXYFyDEKLZ9t6v9adgnptxYpKpSbxtgVENFV",
            self.api_key
        );
        
        println!("🔍 Fetching UniswapV3 pools from The Graph Network...");
        println!("🔗 Using correct V3 subgraph: 5zvR82QoaXYFyDEKLZ9t6v9adgnptxYpKpSbxtgVENFV");
        
        self.try_fetch_v3_from_endpoint(&endpoint).await
    }

    async fn try_fetch_v3_from_endpoint(&mut self, endpoint: &str) -> Result<Vec<GraphV3Pool>> {
        
        let mut all_pools = Vec::new();
        let mut last_id = String::new();
        let mut batch_count = 0;
        const BATCH_SIZE: usize = 1000;
        
        loop {
            batch_count += 1;
            
            let query = if last_id.is_empty() {
                format!(r#"
                    query {{
                        pools(first: {}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                            feeTier
                        }}
                    }}
                "#, BATCH_SIZE)
            } else {
                format!(r#"
                    query {{
                        pools(first: {}, where: {{id_gt: "{}"}}) {{
                            id
                            token0 {{ id symbol name }}
                            token1 {{ id symbol name }}
                            feeTier
                        }}
                    }}
                "#, BATCH_SIZE, last_id)
            };
            
            let response = self.execute_query_internal::<V3PoolsResponse>(&endpoint, &query).await?;
            let pools = response.pools;
            
            println!("📊 Batch {}: Retrieved {} V3 pools (Total: {})", 
                     batch_count, pools.len(), all_pools.len() + pools.len());
            
            if pools.is_empty() {
                break;
            }
            
            if let Some(last_pool) = pools.last() {
                last_id = last_pool.id.clone();
            }
            
            all_pools.extend(pools);
            
            // Longer delay to avoid rate limiting - The Graph has strict limits
            println!("⏳ Waiting 2s to respect rate limits...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
        
        Ok(all_pools)
    }

    /// Execute a GraphQL query (public for testing)
    pub async fn execute_query<T>(&mut self, url: &str, query: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.execute_query_internal(url, query).await
    }

    /// Execute a GraphQL query with retry logic
    async fn execute_query_internal<T>(&mut self, url: &str, query: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        const MAX_RETRIES: u32 = 5;
        let mut attempt = 0;
        
        loop {
            attempt += 1;
            let body = json!({ "query": query });
            
            let response = self.client
                .post(url)
                .header("Content-Type", "application/json")
                .header("User-Agent", "ethereum-pool-monitor/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .json(&body)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    self.query_count += 1;
                    
                    if resp.status().is_success() {
                        match resp.json::<GraphResponse<T>>().await {
                            Ok(graph_response) => {
                                if let Some(errors) = graph_response.errors {
                                    let error_messages: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
                                    return Err(anyhow::anyhow!("GraphQL errors: {}", error_messages.join(", ")));
                                }
                                
                                return graph_response.data
                                    .ok_or_else(|| anyhow::anyhow!("No data in GraphQL response"));
                            }
                            Err(e) => {
                                if attempt >= MAX_RETRIES {
                                    return Err(anyhow::anyhow!("JSON parse error after {} attempts: {}", MAX_RETRIES, e));
                                }
                                println!("⚠️  JSON parse error (attempt {}), retrying...", attempt);
                            }
                        }
                    } else {
                        // Handle specific HTTP error codes with longer delays for rate limiting
                        let status = resp.status();
                        if attempt >= MAX_RETRIES {
                            return Err(anyhow::anyhow!("HTTP error {} after {} attempts", status, MAX_RETRIES));
                        }
                        
                        let delay = match status.as_u16() {
                            429 | 503 => {
                                // Rate limiting or service unavailable - longer delays
                                println!("⚠️  Rate limited (HTTP {}) - attempt {}, waiting {}s...", status, attempt, attempt * 5);
                                attempt * 5 // 5s, 10s, 15s, 20s, 25s
                            }
                            _ => {
                                println!("⚠️  HTTP error {} (attempt {}), retrying in {}s...", status, attempt, attempt);
                                attempt // Normal backoff
                            }
                        };
                        
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay as u64)).await;
                    }
                }
                Err(e) => {
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow::anyhow!("Request failed after {} attempts: {}", MAX_RETRIES, e));
                    }
                    
                    println!("⚠️  Request failed (attempt {}): {}, retrying in {}s...", attempt, e, attempt * 2);
                    tokio::time::sleep(tokio::time::Duration::from_secs((attempt * 2) as u64)).await;
                }
            }
        }
    }

    /// Convert Graph V2 pair to DexPool
    pub fn graph_v2_pair_to_dex_pool(pair: &GraphV2Pair) -> Result<DexPool> {
        Ok(DexPool {
            address: pair.id.clone(),
            protocol: "UniswapV2".to_string(),
            token0: Some(pair.token0.id.clone()),
            token1: Some(pair.token1.id.clone()),
            chain_id: 1, // Ethereum mainnet
        })
    }

    /// Convert Graph V3 pool to DexPool
    pub fn graph_v3_pool_to_dex_pool(pool: &GraphV3Pool) -> Result<DexPool> {
        Ok(DexPool {
            address: pool.id.clone(),
            protocol: "UniswapV3".to_string(),
            token0: Some(pool.token0.id.clone()),
            token1: Some(pool.token1.id.clone()),
            chain_id: 1, // Ethereum mainnet
        })
    }

    /// Convert Graph V4 pool to DexPool
    pub fn graph_v4_pool_to_dex_pool(pool: &GraphV4Pool) -> Result<DexPool> {
        Ok(DexPool {
            address: pool.id.clone(),
            protocol: "UniswapV4".to_string(),
            token0: Some(pool.token0.id.clone()),
            token1: Some(pool.token1.id.clone()),
            chain_id: 1, // Ethereum mainnet
        })
    }

    /// Convert Graph SushiSwap pair to DexPool
    pub fn graph_sushi_pair_to_dex_pool(pair: &GraphSushiPair) -> Result<DexPool> {
        Ok(DexPool {
            address: pair.id.clone(),
            protocol: "SushiSwap".to_string(),
            token0: Some(pair.token0.id.clone()),
            token1: Some(pair.token1.id.clone()),
            chain_id: 1, // Ethereum mainnet
        })
    }

    /// Convert Graph Curve pool to DexPool
    pub fn graph_curve_pool_to_dex_pool(pool: &GraphCurvePool) -> Result<DexPool> {
        // Curve pools can have multiple tokens, we'll use the first two if available
        let (token0, token1) = if pool.coins.len() >= 2 {
            (Some(pool.coins[0].id.clone()), Some(pool.coins[1].id.clone()))
        } else if pool.coins.len() == 1 {
            (Some(pool.coins[0].id.clone()), None)
        } else {
            (None, None)
        };

        Ok(DexPool {
            address: pool.id.clone(),
            protocol: "Curve".to_string(),
            token0,
            token1,
            chain_id: 1, // Ethereum mainnet
        })
    }

    /// Populate database with pools from all supported DEXs (requires subgraph IDs for SushiSwap/Curve)
    pub async fn populate_database_comprehensive(&mut self, pool_db: &PoolDatabase, sushiswap_subgraph_id: Option<&str>, curve_subgraph_id: Option<&str>) -> Result<(u32, u32, u32, u32, u32)> {
        println!("🚀 Starting comprehensive DEX pool data collection from The Graph...");
        println!("📊 Free tier limit: 100,000 queries/month");
        println!("🔄 Resumable collection - will continue from where it left off");
        println!();
        
        // Fetch V2 pairs
        println!("📥 Collecting UniswapV2 pairs...");
        let v2_count = self.fetch_all_v2_pairs_with_db(pool_db).await?;
        println!("✅ V2 collection status: {} pairs in database", v2_count);
        println!();
        
        // Fetch V3 pools
        println!("📥 Collecting UniswapV3 pools...");
        let v3_count = self.fetch_all_v3_pools_with_db(pool_db).await?;
        println!("✅ V3 collection status: {} pools in database", v3_count);
        println!();
        
        // Fetch V4 pools
        println!("📥 Collecting UniswapV4 pools...");
        let v4_count = self.fetch_all_v4_pools_with_db(pool_db).await?;
        println!("✅ V4 collection status: {} pools in database", v4_count);
        println!();
        
        // Fetch SushiSwap pairs if subgraph ID provided
        let sushi_count = if let Some(subgraph_id) = sushiswap_subgraph_id {
            println!("📥 Collecting SushiSwap pairs...");
            let count = self.fetch_all_sushi_pairs_with_db(pool_db, subgraph_id).await?;
            println!("✅ SushiSwap collection status: {} pairs in database", count);
            println!();
            count
        } else {
            println!("⚠️  SushiSwap subgraph ID not provided - skipping");
            pool_db.get_pool_count_by_protocol("SushiSwap").unwrap_or(0)
        };
        
        // Fetch Curve pools if subgraph ID provided
        let curve_count = if let Some(subgraph_id) = curve_subgraph_id {
            println!("📥 Collecting Curve pools...");
            let count = self.fetch_all_curve_pools_with_db(pool_db, subgraph_id).await?;
            println!("✅ Curve collection status: {} pools in database", count);
            println!();
            count
        } else {
            println!("⚠️  Curve subgraph ID not provided - skipping");
            pool_db.get_pool_count_by_protocol("Curve").unwrap_or(0)
        };
        
        let total_queries = self.query_count();
        println!("📈 Query usage this session: {}/{} queries ({:.1}% of free tier)", 
                 total_queries, 100_000, (total_queries as f32 / 100_000.0) * 100.0);
        
        if total_queries > 100_000 {
            println!("⚠️  Warning: Exceeded free tier query limit in this session!");
        } else {
            println!("✅ Well within free tier limits!");
        }
        
        let total_pools = v2_count + v3_count + v4_count + sushi_count + curve_count;
        println!("🎉 Database collection status: {} V2 + {} V3 + {} V4 + {} Sushi + {} Curve = {} total pools", 
                 v2_count, v3_count, v4_count, sushi_count, curve_count, total_pools);
        
        Ok((v2_count, v3_count, v4_count, sushi_count, curve_count))
    }

    /// Populate database with pools from The Graph (optimized with progressive writes and resumable)
    pub async fn populate_database_from_graph_optimized(&mut self, pool_db: &PoolDatabase) -> Result<(u32, u32, u32)> {
        println!("🚀 Starting comprehensive pool data collection from The Graph...");
        println!("📊 Free tier limit: 100,000 queries/month");
        println!("🔄 Resumable collection - will continue from where it left off");
        println!();
        
        // NO clear_pools() call - we want to resume from existing data
        
        // Fetch V2 pairs with progressive database writes (resumable)
        println!("📥 Collecting V2 pairs with progressive database writes (resumable)...");
        let v2_count = self.fetch_all_v2_pairs_with_db(pool_db).await?;
        println!("✅ V2 collection status: {} pairs in database", v2_count);
        println!();
        
        // Fetch V3 pools with progressive database writes (resumable)
        println!("📥 Collecting V3 pools with progressive database writes (resumable)...");
        let v3_count = self.fetch_all_v3_pools_with_db(pool_db).await?;
        println!("✅ V3 collection status: {} pools in database", v3_count);
        println!();
        
        // Fetch V4 pools with progressive database writes (resumable)
        println!("📥 Collecting V4 pools with progressive database writes (resumable)...");
        let v4_count = self.fetch_all_v4_pools_with_db(pool_db).await?;
        println!("✅ V4 collection status: {} pools in database", v4_count);
        println!();
        
        let total_queries = self.query_count();
        println!("📈 Query usage this session: {}/{} queries ({:.1}% of free tier)", 
                 total_queries, 100_000, (total_queries as f32 / 100_000.0) * 100.0);
        
        if total_queries > 100_000 {
            println!("⚠️  Warning: Exceeded free tier query limit in this session!");
        } else {
            println!("✅ Well within free tier limits!");
        }
        
        println!("🎉 Database collection status: {} V2 pairs + {} V3 pools + {} V4 pools = {} total pools", 
                 v2_count, v3_count, v4_count, v2_count + v3_count + v4_count);
        
        Ok((v2_count, v3_count, v4_count))
    }
}