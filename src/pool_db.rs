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
