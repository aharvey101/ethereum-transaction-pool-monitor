use anyhow::Result;
use rusqlite::{Connection, params, OptionalExtension};
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
        let db = PoolDatabase { conn: Mutex::new(conn) };
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
            CREATE INDEX IF NOT EXISTS idx_chain_id ON pools(chain_id);"
        )?;
        Ok(())
    }

    /// Check if an address is a known DEX pool
    pub fn is_dex_pool(&self, address: &str, chain_id: u32) -> Result<bool> {
        let normalized = address.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let result: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pools WHERE LOWER(address) = ?1 AND chain_id = ?2)",
            params![&normalized, chain_id],
            |row| row.get(0),
        )?;
        Ok(result)
    }

    /// Get pool details by address
    #[allow(dead_code)]
    pub fn get_pool(&self, address: &str, chain_id: u32) -> Result<Option<DexPool>> {
        let normalized = address.to_lowercase();
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
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
        ).optional()?;
        Ok(result)
    }

    /// Add a new pool to the database
    #[allow(dead_code)]
    pub fn add_pool(&self, pool: &DexPool) -> Result<()> {
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
             WHERE protocol = ?1 AND chain_id = ?2"
        )?;
        let pools = stmt.query_map(params![protocol, chain_id], |row| {
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
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM pools",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Clear all pools (useful for updates)
    pub fn clear_pools(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pools", [])?;
        Ok(())
    }

    /// Seed database with known DEX router addresses for testing
    pub fn seed_known_dexes(&self, chain_id: u32) -> Result<u32> {
        let known_dexes = vec![
            // Uniswap V3
            ("0x1F98431c8aD98523631AE4a59f267346ea3113F", "Uniswap V3 Router"),
            ("0xE592427A0AEce92De3Edee1F18E0157C05861564", "Uniswap V3 SwapRouter"),
            ("0x68b3465833fb72B5A828cCEDA3187CF6cc380C86", "Uniswap V3 SwapRouter02"),
            
            // Uniswap V2
            ("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D", "Uniswap V2 Router"),
            ("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f", "Uniswap V2 Factory"),
            
            // Curve Finance
            ("0x99a58482BD7490Cf8E3bfcA92e2A6b5F7e36c009", "Curve StableSwap"),
            ("0xDC24316b9AE028E5614BFa16D19dC5c08421f535", "Curve StableSwap2"),
            
            // SushiSwap
            ("0xd9e1cE17f2641f24aE9f7FFe6ff87D78ef7B26C1", "SushiSwap Router"),
            ("0xC0AEe478e3B480f1DFF3EA3199A02A6aA7Fa05eA", "SushiSwap Factory"),
            
            // Balancer
            ("0xBA12222222228d8Ba445958a75a0704d566BF2C8", "Balancer Vault"),
            
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

        db.add_pool(&pool)?;
        assert!(db.is_dex_pool("0x1F98431c8aD98523631AE4a59f267346ea3113F", 1)?);
        assert!(db.is_dex_pool("0x1f98431c8ad98523631ae4a59f267346ea3113f", 1)?);
        assert!(!db.is_dex_pool("0x0000000000000000000000000000000000000000", 1)?);

        let retrieved = db.get_pool("0x1F98431c8aD98523631AE4a59f267346ea3113F", 1)?;
        assert!(retrieved.is_some());

        Ok(())
    }
}
