use anyhow::Result;
use crate::pool_fetcher::PoolFetcher;
use crate::pool_db::PoolDatabase;
use tokio::sync::mpsc;

/// Messages sent from the background pool loader to the main app
#[derive(Clone, Debug)]
pub enum PoolLoaderMessage {
    /// Progress update with (message, v2_pools, v3_pools, progress_percent)
    Progress(String, u32, u32, u32),
    /// Pool loading completed with (v2_count, v3_count, total_count)
    Complete(u32, u32, u32),
    /// Pool loading failed with error message
    Error(String),
}

/// Background task that loads pools from the blockchain
pub struct BackgroundPoolLoader {
    tx: mpsc::UnboundedSender<PoolLoaderMessage>,
}

impl BackgroundPoolLoader {
    /// Create a new background pool loader and spawn the task
    pub fn spawn(
        rpc_url: String,
        db_path: String,
        chain_id: u32,
    ) -> (Self, mpsc::UnboundedReceiver<PoolLoaderMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();
        let tx_clone_error = tx.clone();

        // Spawn the background task
        tokio::spawn(async move {
            if let Err(e) = Self::load_pools(&rpc_url, &db_path, chain_id, tx_clone).await {
                let _ = tx_clone_error.send(PoolLoaderMessage::Error(e.to_string()));
            }
        });

        (Self { tx }, rx)
    }

    /// Load pools from the blockchain
    async fn load_pools(
        rpc_url: &str,
        db_path: &str,
        chain_id: u32,
        tx: mpsc::UnboundedSender<PoolLoaderMessage>,
    ) -> Result<()> {
        let pool_db = PoolDatabase::new(db_path)?;
        let pool_fetcher = PoolFetcher::new(rpc_url);

        // Check if we should force a complete pool refresh
        let force_refresh = std::env::var("FORCE_POOL_REFRESH").is_ok();
        let existing_pool_count = pool_db.pool_count().unwrap_or(0);
        
        if !force_refresh && existing_pool_count >= 1000 {
            tracing::info!("Sufficient pools already in database ({}), skipping pool loading", existing_pool_count);
            tracing::info!("Use FORCE_POOL_REFRESH=1 to force a complete refresh");
            let _ = tx.send(PoolLoaderMessage::Complete(0, 0, existing_pool_count));
            return Ok(());
        }
        
        if force_refresh {
            tracing::info!("FORCE_POOL_REFRESH enabled - starting complete pool scan (existing: {})", existing_pool_count);
        } else {
            tracing::info!("Pool count low ({}), starting comprehensive pool scan", existing_pool_count);
        }

        // Clear existing pools only if we're doing a full reload
        pool_db.clear_pools()?;

        let mut v2_count = 0;
        let mut v3_count = 0;

        // Send initial progress
        let _ = tx.send(PoolLoaderMessage::Progress(
            "UniswapV2: Initializing...".to_string(),
            0,
            0,
            0,
        ));

        // Fetch V2 pools
        tracing::info!("Background loader: Fetching UniswapV2 pools");
        match pool_fetcher.fetch_uniswap_v2_pools(&pool_db, chain_id).await {
            Ok(count) => {
                tracing::info!("Background loader: Found {} V2 pools", count);
                v2_count = count;
                let _ = tx.send(PoolLoaderMessage::Progress(
                    format!("UniswapV2: Complete! {} pools found", count),
                    v2_count,
                    v3_count,
                    33, // Arbitrary progress point for V2 completion
                ));
            }
            Err(e) => {
                tracing::warn!("Background loader: Failed to fetch V2 pools: {}", e);
                let _ = tx.send(PoolLoaderMessage::Error(format!(
                    "Failed to fetch V2 pools: {}",
                    e
                )));
                return Ok(());
            }
        }

        // Send V3 progress
        let _ = tx.send(PoolLoaderMessage::Progress(
            "UniswapV3: Initializing...".to_string(),
            v2_count,
            v3_count,
            33,
        ));

        // Fetch V3 pools
        tracing::info!("Background loader: Fetching UniswapV3 pools");
        match pool_fetcher.fetch_uniswap_v3_pools(&pool_db, chain_id).await {
            Ok(count) => {
                tracing::info!("Background loader: Found {} V3 pools", count);
                v3_count = count;
                let _ = tx.send(PoolLoaderMessage::Progress(
                    format!("UniswapV3: Complete! {} pools found", count),
                    v2_count,
                    v3_count,
                    66, // Arbitrary progress point for V3 completion
                ));
            }
            Err(e) => {
                tracing::warn!("Background loader: Failed to fetch V3 pools: {}", e);
                let _ = tx.send(PoolLoaderMessage::Error(format!(
                    "Failed to fetch V3 pools: {}",
                    e
                )));
                return Ok(());
            }
        }

        // Seed known DEX addresses if we don't have enough pools
        let mut total = v2_count + v3_count;
        if total < 100 {
            tracing::info!("Background loader: Seeding with known DEX addresses");
            let _ = tx.send(PoolLoaderMessage::Progress(
                "Seeding with known DEX addresses...".to_string(),
                v2_count,
                v3_count,
                80,
            ));

            match pool_db.seed_known_dexes(chain_id) {
                Ok(count) => {
                    tracing::info!("Background loader: Seeded {} DEX addresses", count);
                    total += count;
                }
                Err(e) => {
                    tracing::warn!("Background loader: Failed to seed DEX addresses: {}", e);
                }
            }
        }

        // Get final pool count
        match pool_db.pool_count() {
            Ok(final_count) => {
                tracing::info!("Background loader: Complete! {} total pools", final_count);
                let _ = tx.send(PoolLoaderMessage::Complete(v2_count, v3_count, final_count));
            }
            Err(e) => {
                tracing::error!("Background loader: Failed to get pool count: {}", e);
                let _ = tx.send(PoolLoaderMessage::Error(format!(
                    "Failed to get pool count: {}",
                    e
                )));
            }
        }

        Ok(())
    }
}
