use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Mutex;

/// Represents a DEX pool in the database
#[derive(Clone, Debug)]
pub struct DexPool {
    pub address: String,
    pub protocol: String,
    pub token0: Option<String>,
    pub token1: Option<String>,
    pub chain_id: u32,
}

/// DEX pool database for querying pools by address
pub struct PoolDatabase {
    conn: Mutex<Connection>,
}

impl PoolDatabase {
    /// Create or open a DEX pool database at the given path
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        let db = PoolDatabase {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pools (
                id INTEGER PRIMARY KEY,
                address TEXT NOT NULL UNIQUE,
                protocol TEXT NOT NULL,
                token0 TEXT,
                token1 TEXT,
                chain_id INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_address ON pools(address);
            CREATE INDEX IF NOT EXISTS idx_protocol ON pools(protocol);
            CREATE INDEX IF NOT EXISTS idx_chain_id ON pools(chain_id);",
        )?;
        Ok(())
    }

    /// Check if an address is a known DEX router (fast in-memory lookup)
    pub fn is_dex_router(&self, address: &str) -> bool {
        let normalized = address.to_lowercase();

        // Major DEX router addresses on Ethereum mainnet
        let routers = [
            // Uniswap V2 Router
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d",
            // Uniswap V3 Routers
            "0xe592427a0aece92de3edee1f18e0157c05861564", // SwapRouter
            "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45", // SwapRouter02
            // SushiSwap Router
            "0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f",
            // PancakeSwap V2 Router (Ethereum)
            "0xeff92a263d31888d860bd50809a8d171709b7b1c",
            // Curve Finance Routers
            "0xf0d4c12a5768d806021f80a262b4d39d26c58b8d", // CurveRouterV1
            "0x16c6521dff6baab339122a0fe25b9116367cc36b", // CurveRouter
            // 1inch Router V5
            "0x1111111254eeb25477b68fb85ed929f73a960582",
            // 0x Protocol
            "0xdef1c0ded9bec7f1a1670819833240f027b25eff", // ExchangeProxy
            // Balancer V2 Vault
            "0xba12222222228d8ba445958a75a0704d566bf2c8",
            // MetaMask Swap Router
            "0x881d40237659c251811cec9c364ef91dc08d300c",
            // ParaSwap Augustus V5
            "0xdef171fe48cf0115b1d80b88dc8eab59176fee57",
            // OpenOcean Router
            "0x6352a56caadc4f1e25cd6c75970fa768a3304e64",
        ];

        routers.contains(&normalized.as_str())
    }

    /// Check if an address is a stablecoin contract (subset of tokens)
    pub fn is_stablecoin(&self, address: &str) -> bool {
        let normalized = address.to_lowercase();

        let stablecoins = [
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
            "0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", // USDC
            "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
            "0x4fabb145d64652a948d72533023f6e7a623c7c53", // BUSD
            "0x853d955acef822db058eb8505911ed77f175b99e", // FRAX
            "0x5f98805a4e8be255a32880fdec7f6728c6568ba0", // LUSD
            "0x57ab1ec28d129707052df4df418d58a2d46d5f51", // sYNTH sUSD
            "0x0000000000085d4780b73119b644ae5ecd22b376", // TUSD
        ];

        stablecoins.contains(&normalized.as_str())
    }

    /// Check if an address is a major token contract (fast in-memory lookup)
    pub fn is_token_contract(&self, address: &str) -> bool {
        let normalized = address.to_lowercase();

        // Major token contracts on Ethereum mainnet
        let tokens = [
            // Stablecoins
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
            "0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", // USDC
            "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
            "0x4fabb145d64652a948d72533023f6e7a623c7c53", // BUSD
            "0x853d955acef822db058eb8505911ed77f175b99e", // FRAX
            "0x5f98805a4e8be255a32880fdec7f6728c6568ba0", // LUSD
            "0x57ab1ec28d129707052df4df418d58a2d46d5f51", // sYNTH sUSD
            "0x0000000000085d4780b73119b644ae5ecd22b376", // TUSD
            // Wrapped ETH
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // WETH
            // Major ERC-20 tokens
            "0x1f9840a85d5af5bf1d1762f925bdaddc4201f984", // UNI
            "0x7d1afa7b718fb893db30a3abc0cfc608aacfebb0", // MATIC
            "0x6b3595068778dd592e39a122f4f5a5cf09c90fe2", // SUSHI
            "0xc00e94cb662c3520282e6f5717214004a7f26888", // COMP
            "0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2", // MKR
            "0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9", // AAVE
            "0xc011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f", // SNX
            "0x0bc529c00c6401aef6d220be8c6ea1667f6ad93e", // YFI
            "0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", // WBTC
            "0x514910771af9ca656af840dff83e8264ecf986ca", // LINK
            "0xa693b19d2931d498c5b318df961919bb4aee87a5", // UST
            "0x4e3fbd56cd56c3e72c1403e103b45db9da5b9d2b", // CVX
            "0x6dea81c8171d0ba574754ef6f8b412f2ed88c54d", // LQTY
            // Liquid staking tokens
            "0xae7ab96520de3a18e5e111b5eaab095312d7fe84", // stETH (Lido)
            "0xbe9895146f7af43049ca1c1ae358b0541ea49704", // cbETH (Coinbase)
            "0xa2e3356610840701bdf5611a53974510ae27e2e1", // wBETH (Binance)
            // Meme tokens (popular for trading)
            "0x95ad61b0a150d79219dcf64e1e6cc01f0b64c4ce", // SHIB
            "0x4d224452801aced8b2f0aebe155379bb5d594381", // APE
            "0xa0246c9032bc3a600820415ae600c6388619a14d", // FARM
        ];

        tokens.contains(&normalized.as_str())
    }

    /// Determine the specific type of DeFi activity for an address
    pub fn get_defi_activity_type(
        &self,
        address: &str,
        chain_id: u32,
    ) -> Result<crate::eth_client::DefiActivityType> {
        use crate::eth_client::DefiActivityType;

        // Check in priority order (most specific first)
        if self.is_stablecoin(address) {
            return Ok(DefiActivityType::Stablecoin);
        }

        if self.is_dex_router(address) {
            return Ok(DefiActivityType::DexRouter);
        }

        if self.is_token_contract(address) {
            return Ok(DefiActivityType::TokenContract);
        }

        // Check if it's a pool (database lookup)
        let normalized = address.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let is_pool: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pools WHERE LOWER(address) = ?1 AND chain_id = ?2)",
            params![&normalized, chain_id],
            |row| row.get(0),
        )?;

        if is_pool {
            return Ok(DefiActivityType::DexPool);
        }

        Ok(DefiActivityType::None)
    }

    /// Get pool details by address
    #[allow(dead_code)]
    pub fn get_pool(&self, address: &str, chain_id: u32) -> Result<Option<DexPool>> {
        let normalized = address.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT address, protocol, token0, token1, chain_id FROM pools
             WHERE LOWER(address) = ?1 AND chain_id = ?2",
                params![&normalized, chain_id],
                |row| {
                    Ok(DexPool {
                        address: row.get(0)?,
                        protocol: row.get(1)?,
                        token0: row.get(2)?,
                        token1: row.get(3)?,
                        chain_id: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// Get all pools at a specific address (for all chains)
    pub fn get_pools_by_address(&self, address: &str) -> Result<Vec<DexPool>> {
        let normalized = address.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, protocol, token0, token1, chain_id FROM pools
             WHERE LOWER(address) = ?1",
        )?;

        let pool_iter = stmt.query_map([&normalized], |row| {
            Ok(DexPool {
                address: row.get(0)?,
                protocol: row.get(1)?,
                token0: row.get(2)?,
                token1: row.get(3)?,
                chain_id: row.get(4)?,
            })
        })?;

        let mut pools = Vec::new();
        for pool in pool_iter {
            pools.push(pool?);
        }

        Ok(pools)
    }

    /// Get total number of pools in database
    pub async fn get_total_pools(&self) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM pools", [], |row| row.get(0))?;
        Ok(count as u32)
    }

     /// Add a new pool to the database
     pub fn insert_pool(&self, pool: &DexPool) -> Result<()> {
         let conn = self.conn.lock().unwrap();
         conn.execute(
             "INSERT OR REPLACE INTO pools (address, protocol, token0, token1, chain_id)
              VALUES (?1, ?2, ?3, ?4, ?5)",
             params![
                 pool.address.to_lowercase(),
                 &pool.protocol,
                 &pool.token0,
                 &pool.token1,
                 pool.chain_id,
             ],
         )?;
         Ok(())
     }

    /// Bulk insert pools
    pub fn add_pools(&self, pools: &[DexPool]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for pool in pools {
            tx.execute(
                "INSERT OR REPLACE INTO pools (address, protocol, token0, token1, chain_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    pool.address.to_lowercase(),
                    &pool.protocol,
                    &pool.token0,
                    &pool.token1,
                    pool.chain_id,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Get all pools for a specific protocol
    #[allow(dead_code)]
    pub fn get_pools_by_protocol(&self, protocol: &str, chain_id: u32) -> Result<Vec<DexPool>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, protocol, token0, token1, chain_id FROM pools
             WHERE protocol = ?1 AND chain_id = ?2",
        )?;
        let pools = stmt
            .query_map(params![protocol, chain_id], |row| {
                Ok(DexPool {
                    address: row.get(0)?,
                    protocol: row.get(1)?,
                    token0: row.get(2)?,
                    token1: row.get(3)?,
                    chain_id: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pools)
    }

    /// Count pools in database
    pub fn pool_count(&self) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM pools", [], |row| row.get(0))?;
        Ok(count as u32)
    }

    /// Count pools by protocol
    pub fn get_pool_count_by_protocol(&self, protocol: &str) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pools WHERE protocol = ?1",
            params![protocol],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    /// Get sample pools for testing (limit number of results)
    /// Clear all pools (useful for updates)
    pub fn clear_pools(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pools", [])?;
        Ok(())
    }

    /// Get the latest (highest) pool ID for a specific protocol
    /// This allows resumable collection by finding where we left off
    pub fn get_latest_pool_id(&self, protocol: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address FROM pools
             WHERE protocol = ?1
             ORDER BY address DESC
             LIMIT 1",
        )?;

        let result = stmt
            .query_row(params![protocol], |row| Ok(row.get::<_, String>(0)?))
            .optional()?;

        Ok(result)
    }

    /// Check if collection is complete by looking for a reasonable pool count
    /// V2: Should have 80,000+ pools when complete
    /// V3: Should have 30,000+ pools when complete
    /// V4: Should have 1,000+ pools when complete (newer protocol)
    /// SushiSwap: Should have 5,000+ pools when complete
    /// Curve: Should have 1,000+ pools when complete
    pub fn is_collection_complete(&self, protocol: &str) -> Result<bool> {
        let count = self.get_pool_count_by_protocol(protocol)?;
        let threshold = match protocol {
            "UniswapV2" => 80_000, // Expect ~100k+
            "UniswapV3" => 30_000, // Expect ~50k+
            "UniswapV4" => 1_000,  // Expect ~5k+ (newer protocol)
            "SushiSwap" => 5_000,  // Expect ~10k+
            "Curve" => 1_000,      // Expect ~2k+
            _ => return Ok(false),
        };
        Ok(count >= threshold)
    }

    /// Seed database with known DEX router addresses for testing
    pub fn seed_known_dexes(&self, chain_id: u32) -> Result<u32> {
        let known_dexes = vec![
            // Uniswap V3
            (
                "0x1F98431c8aD98523631AE4a59f267346ea3113F",
                "Uniswap V3 Router",
            ),
            (
                "0xE592427A0AEce92De3Edee1F18E0157C05861564",
                "Uniswap V3 SwapRouter",
            ),
            (
                "0x68b3465833fb72B5A828cCEDA3187CF6cc380C86",
                "Uniswap V3 SwapRouter02",
            ),
            // Uniswap V2
            (
                "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D",
                "Uniswap V2 Router",
            ),
            (
                "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
                "Uniswap V2 Factory",
            ),
            // Curve Finance
            (
                "0x99a58482BD7490Cf8E3bfcA92e2A6b5F7e36c009",
                "Curve StableSwap",
            ),
            (
                "0xDC24316b9AE028E5614BFa16D19dC5c08421f535",
                "Curve StableSwap2",
            ),
            // SushiSwap
            (
                "0xd9e1cE17f2641f24aE9f7FFe6ff87D78ef7B26C1",
                "SushiSwap Router",
            ),
            (
                "0xC0AEe478e3B480f1DFF3EA3199A02A6aA7Fa05eA",
                "SushiSwap Factory",
            ),
            // Balancer
            (
                "0xBA12222222228d8Ba445958a75a0704d566BF2C8",
                "Balancer Vault",
            ),
            // 0x Protocol
            ("0xDef1C0ded9bef7B1AcB7b8f6Ce78ffe3D5B11BAa", "0x Protocol"),
            // 1inch
            ("0x1111111254fb6c44bac0bed2854e76f90643097d", "1inch Router"),
        ];

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut count = 0;

        for (address, protocol) in known_dexes {
            match tx.execute(
                "INSERT OR IGNORE INTO pools (address, protocol, chain_id) VALUES (?1, ?2, ?3)",
                params![address.to_lowercase(), protocol, chain_id],
            ) {
                Ok(rows) if rows > 0 => count += 1,
                _ => {}
            }
        }

        tx.commit()?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_database() -> Result<()> {
        let db = PoolDatabase::new(":memory:")?;

        let pool = DexPool {
            address: "0x1F98431c8aD98523631AE4a59f267346ea3113F".to_string(),
            protocol: "Uniswap V3".to_string(),
            token0: Some("WETH".to_string()),
            token1: Some("USDC".to_string()),
            chain_id: 1,
        };

         db.insert_pool(&pool)?;
        
        // Test that the pool can be retrieved
        let retrieved = db.get_pool("0x1F98431c8aD98523631AE4a59f267346ea3113F", 1)?;
        assert!(retrieved.is_some());
        
        // Test case insensitive retrieval  
        let retrieved_lowercase = db.get_pool("0x1f98431c8ad98523631ae4a59f267346ea3113f", 1)?;
        assert!(retrieved_lowercase.is_some());
        
        // Test non-existent pool
        let non_existent = db.get_pool("0x0000000000000000000000000000000000000000", 1)?;
        assert!(non_existent.is_none());

        Ok(())
    }
}

impl PoolDatabase {
    pub fn find_pool_by_tokens(&self, token0: &str, token1: &str) -> Result<Vec<DexPool>> {
        let normalized_token0 = token0.to_lowercase();
        let normalized_token1 = token1.to_lowercase();

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, protocol, token0, token1, chain_id
             FROM pools
             WHERE (LOWER(token0) = ?1 AND LOWER(token1) = ?2)
                OR (LOWER(token0) = ?2 AND LOWER(token1) = ?1)
             ORDER BY protocol ASC
             LIMIT 5",
        )?;

        let pool_iter = stmt.query_map(&[&normalized_token0, &normalized_token1], |row| {
            Ok(DexPool {
                address: row.get(0)?,
                protocol: row.get(1)?,
                token0: row.get(2).ok(),
                token1: row.get(3).ok(),
                chain_id: row.get(4)?,
            })
        })?;

        let mut pools = Vec::new();
        for pool in pool_iter {
            pools.push(pool?);
        }

        Ok(pools)
    }

    pub fn get_pool_by_address(&self, address: &str) -> Result<Option<DexPool>> {
        let normalized_address = address.to_lowercase();

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, protocol, token0, token1, chain_id
             FROM pools
             WHERE LOWER(address) = ?1
             LIMIT 1",
        )?;

        let mut pool_iter = stmt.query_map(&[&normalized_address], |row| {
            Ok(DexPool {
                address: row.get(0)?,
                protocol: row.get(1)?,
                token0: row.get(2).ok(),
                token1: row.get(3).ok(),
                chain_id: row.get(4)?,
            })
        })?;

        match pool_iter.next() {
            Some(pool) => Ok(Some(pool?)),
            None => Ok(None),
        }
    }
}
