use crate::eth_client::{EthereumClient, MempoolTransaction};
use crate::pool_db::PoolDatabase;
use crate::coingecko::CoinGeckoClient;
use crate::pool_fetcher::PoolFetcher;
use anyhow::Result;
use std::collections::VecDeque;
use chrono::Local;

const MAX_TRANSACTIONS_DISPLAY: usize = 1000;

/// Filter mode for transaction display
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    All,
    DexOnly,
}

impl FilterMode {
    pub fn toggle(self) -> Self {
        match self {
            FilterMode::All => FilterMode::DexOnly,
            FilterMode::DexOnly => FilterMode::All,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FilterMode::All => "All Transactions",
            FilterMode::DexOnly => "DEX Only",
        }
    }
}

/// Sort field for transactions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Default,
    GasPrice,
    Value,
    Nonce,
}

impl SortField {
    pub fn toggle(self) -> Self {
        match self {
            SortField::Default => SortField::GasPrice,
            SortField::GasPrice => SortField::Value,
            SortField::Value => SortField::Nonce,
            SortField::Nonce => SortField::Default,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SortField::Default => "Default",
            SortField::GasPrice => "Gas Price",
            SortField::Value => "Value",
            SortField::Nonce => "Nonce",
        }
    }
}

/// Application state containing transaction data and UI state
pub struct AppState {
    pub transactions: VecDeque<MempoolTransaction>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub client: EthereumClient,
    pub pool_db: PoolDatabase,
    pub coingecko_client: CoinGeckoClient,
    pub pool_fetcher: PoolFetcher,
    pub chain_id: u32,
    pub status: String,
    pub is_running: bool,
    pub last_update: String,
    pub last_pool_sync: String,
    pub pool_count: u32,
    pub connection_healthy: bool,
    pub needs_redraw: bool,
    pub is_loading_pools: bool,
    pub pools_loading_progress: String,
    pub pools_found_count: u32,
    pub v2_pools_found: u32,
    pub v3_pools_found: u32,
    pub pool_loading_progress_percent: u32,
    pub filter_mode: FilterMode,
    pub sort_field: SortField,
    pub cached_filtered_count_display: usize, // for UI display without needing mutable access
    pub cached_title: String, // cached title string to avoid repeated format! calls
    // Cache for filtered/sorted transactions to avoid recomputing on every frame
    #[allow(dead_code)]
    pub cached_filtered_sorted: Vec<usize>, // indices into transactions deque
    cached_filtered_count: usize, // cached count to avoid O(n) on every scroll
    cache_dirty: bool,
}

impl AppState {
    /// Create a new application state
    pub async fn new(rpc_url: &str, db_path: &str, chain_id: u32) -> Result<Self> {
        let client = EthereumClient::new(rpc_url).await?;
        let pool_db = PoolDatabase::new(db_path)?;
        let coingecko_client = CoinGeckoClient::new();
        let pool_fetcher = PoolFetcher::new(rpc_url);
        let pool_count = pool_db.pool_count().unwrap_or(0);
        
         Ok(AppState {
             transactions: VecDeque::new(),
             selected_index: 0,
             scroll_offset: 0,
             client,
             pool_db,
             coingecko_client,
             pool_fetcher,
             chain_id,
             status: "Starting transaction monitor...".to_string(),
             is_running: true,
             last_update: "Never".to_string(),
             last_pool_sync: "Never".to_string(),
             pool_count,
             connection_healthy: true,
             needs_redraw: true,
             is_loading_pools: false,
             pools_loading_progress: String::new(),
             pools_found_count: 0,
             v2_pools_found: 0,
             v3_pools_found: 0,
             pool_loading_progress_percent: 0,
             filter_mode: FilterMode::All,
             sort_field: SortField::Default,
             cached_filtered_count_display: 0,
             cached_title: " Pending Transactions (0) ".to_string(),
             cached_filtered_sorted: Vec::new(),
             cached_filtered_count: 0,
             cache_dirty: true,
         })
     }

    /// Update transactions from the mempool
    pub async fn update_transactions(&mut self) -> Result<()> {
        match self.client.get_pending_transactions(&self.pool_db, self.chain_id).await {
            Ok(new_txs) => {
                // Clear and add new transactions
                self.transactions.clear();
                for tx in new_txs {
                    self.transactions.push_back(tx);
                    if self.transactions.len() > MAX_TRANSACTIONS_DISPLAY {
                        self.transactions.pop_front();
                    }
                }
                self.status = format!("{} pending transactions found", self.transactions.len());
                self.last_update = Local::now().format("%H:%M:%S").to_string();
                self.connection_healthy = true;
                self.cache_dirty = true; // Mark cache as dirty when transactions update
                self.cached_title = format!(" Pending Transactions ({}) ", self.transactions.len()); // Update cached title
                self.needs_redraw = true;
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
                self.connection_healthy = false;
                self.needs_redraw = true;
            }
        }
        Ok(())
    }

    /// Sync DEX pools from CoinGecko
    pub async fn sync_dex_pools(&mut self) -> Result<()> {
        // Fetch pools from major DEXes on Ethereum
        let dexes = vec![
            "uniswap_v3",
            "uniswap_v2",
            "sushiswap",
            "curve",
            "balancer",
        ];

        match self.coingecko_client.fetch_all_pools("eth", &dexes, 3).await {
            Ok(pools) => {
                if !pools.is_empty() {
                    // Clear old pools and add new ones
                    self.pool_db.clear_pools()?;
                    self.pool_db.add_pools(&pools)?;
                    self.pool_count = self.pool_db.pool_count()?;
                    self.last_pool_sync = Local::now().format("%H:%M:%S").to_string();
                    self.needs_redraw = true;
                }
            }
            Err(e) => {
                eprintln!("Error syncing DEX pools: {}", e);
            }
        }
        Ok(())
    }

    /// Sync DEX pools directly from Ethereum node
    pub async fn sync_pools_from_node(&mut self) -> Result<()> {
        // Check if we should force a complete pool refresh
        let force_refresh = std::env::var("FORCE_POOL_REFRESH").is_ok();
        let existing_pool_count = self.pool_db.pool_count().unwrap_or(0);
        
        if !force_refresh && existing_pool_count >= 1000 {
            tracing::info!("Sufficient pools already in database ({}), skipping pool sync", existing_pool_count);
            tracing::info!("Use FORCE_POOL_REFRESH=1 to force a complete refresh");
            self.pool_count = existing_pool_count;
            self.is_loading_pools = false;
            self.pools_loading_progress = format!("Using existing {} pools from database", existing_pool_count);
            return Ok(());
        }
        
        if force_refresh {
            tracing::info!("FORCE_POOL_REFRESH enabled - starting complete pool scan (existing: {})", existing_pool_count);
        } else {
            tracing::info!("Pool count low ({}), syncing pools from Ethereum node", existing_pool_count);
        }
        
        // Clear existing pools only if we're doing a full reload
        self.pool_db.clear_pools()?;
        
        let mut total = 0;
        self.is_loading_pools = true;
        self.pools_found_count = 0;
        self.needs_redraw = true;

        // V2 Progress: Update app state directly via mutable reference pattern
        // We'll collect the state and update after each batch
        
        // Fetch recent V2 pools from node
        tracing::info!("Fetching UniswapV2 pools from node");
        self.pools_loading_progress = "UniswapV2: Initializing...".to_string();
        self.needs_redraw = true;
        
        match self.pool_fetcher.fetch_uniswap_v2_pools(&self.pool_db, self.chain_id).await {
            Ok(count) => {
                tracing::info!("Synced {} UniswapV2 pools from node", count);
                total += count;
                self.pools_found_count = total;
                self.pools_loading_progress = format!("UniswapV2: Complete! {} pools found", count);
                self.needs_redraw = true;
            }
            Err(e) => {
                tracing::warn!("Failed to sync UniswapV2 pools: {}", e);
            }
        }

        // Fetch recent V3 pools from node
        tracing::info!("Fetching UniswapV3 pools from node");
        self.pools_loading_progress = "UniswapV3: Initializing...".to_string();
        self.needs_redraw = true;
        
        match self.pool_fetcher.fetch_uniswap_v3_pools(&self.pool_db, self.chain_id).await {
            Ok(count) => {
                tracing::info!("Synced {} UniswapV3 pools from node", count);
                total += count;
                self.pools_found_count = total;
                self.pools_loading_progress = format!("UniswapV3: Complete! {} pools found", count);
                self.needs_redraw = true;
            }
            Err(e) => {
                tracing::warn!("Failed to sync UniswapV3 pools: {}", e);
            }
        }

        // If we don't have enough pools, fall back to seeding with known DEX addresses
        if total < 100 {
            tracing::info!("Not enough pools found from node ({} < 100), seeding with known DEX addresses", total);
            self.pools_loading_progress = "Seeding with known DEX addresses...".to_string();
            self.needs_redraw = true;
            
            match self.pool_db.seed_known_dexes(self.chain_id) {
                Ok(count) => {
                    tracing::info!("Seeded {} known DEX addresses", count);
                    total += count;
                    self.pools_found_count = total;
                }
                Err(e) => {
                    tracing::error!("Failed to seed known DEX addresses: {}", e);
                }
            }
        }

        self.pool_count = self.pool_db.pool_count()?;
        self.last_pool_sync = Local::now().format("%H:%M:%S").to_string();
        self.is_loading_pools = false;
        self.pools_loading_progress = format!("Loaded {} pools", self.pool_count);
        self.pools_found_count = self.pool_count;
        self.needs_redraw = true;

        tracing::info!("Pool sync complete. Total pools: {}", self.pool_count);
        Ok(())
    }

    /// Scroll down in the transaction list
    pub fn scroll_down(&mut self, amount: usize, max_rows: usize) {
        let filtered_count = self.get_filtered_transaction_count();
        let max_scroll = filtered_count.saturating_sub(max_rows);
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
        self.needs_redraw = true;
    }

    /// Scroll up in the transaction list
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.needs_redraw = true;
    }

    /// Select next transaction and auto-scroll to keep it visible
    pub fn select_next(&mut self, max_rows: usize) {
        // Only get count if we actually need it (cache makes this fast anyway)
        if self.transactions.is_empty() {
            return;
        }
        
        let filtered_count = self.get_filtered_transaction_count();
        if filtered_count > 0 {
            self.selected_index = (self.selected_index + 1) % filtered_count;
            self.ensure_selection_visible(max_rows);
            self.needs_redraw = true;
        }
    }

    /// Select previous transaction and auto-scroll to keep it visible  
    pub fn select_previous(&mut self, max_rows: usize) {
        // Only get count if we actually need it (cache makes this fast anyway)
        if self.transactions.is_empty() {
            return;
        }
        
        let filtered_count = self.get_filtered_transaction_count();
        if filtered_count > 0 {
            self.selected_index = if self.selected_index == 0 {
                filtered_count - 1
            } else {
                self.selected_index - 1
            };
            self.ensure_selection_visible(max_rows);
            self.needs_redraw = true;
        }
    }

    /// Ensure the selected transaction is visible, auto-scrolling if needed
    fn ensure_selection_visible(&mut self, max_rows: usize) {
        let selected_pos = self.selected_index;
        let viewport_start = self.scroll_offset;
        let viewport_end = viewport_start + max_rows;

        if selected_pos < viewport_start {
            // Selection is above viewport, scroll up
            self.scroll_offset = selected_pos;
        } else if selected_pos >= viewport_end {
            // Selection is below viewport, scroll down
            self.scroll_offset = selected_pos.saturating_sub(max_rows - 1);
        }
    }

    /// Get filtered transactions based on current filter mode
    pub fn get_filtered_transactions(&self) -> Vec<&MempoolTransaction> {
        match self.filter_mode {
            FilterMode::All => self.transactions.iter().collect(),
            FilterMode::DexOnly => {
                self.transactions
                    .iter()
                    .filter(|tx| tx.is_dex)
                    .collect()
            }
        }
    }

    /// Toggle the filter mode between All and DexOnly
    pub fn toggle_filter(&mut self) {
        self.filter_mode = self.filter_mode.toggle();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.cache_dirty = true;
        self.needs_redraw = true;
    }

    /// Get the count of transactions that match current filter (cached)
    pub fn get_filtered_transaction_count(&mut self) -> usize {
        self.ensure_cache_valid();
        self.cached_filtered_count
    }

    /// Build and cache filtered/sorted transaction indices if needed
    fn ensure_cache_valid(&mut self) {
        if !self.cache_dirty {
            return;
        }

        // Build list of indices for filtered transactions
        let mut indices: Vec<usize> = (0..self.transactions.len())
            .filter(|&i| {
                let tx = &self.transactions[i];
                match self.filter_mode {
                    FilterMode::All => true,
                    FilterMode::DexOnly => tx.is_dex,
                }
            })
            .collect();

        // Cache the count before sorting
        self.cached_filtered_count = indices.len();
        self.cached_filtered_count_display = indices.len(); // Update display cache too

        // Sort indices based on sort field
        match self.sort_field {
            SortField::Default => {
                // Keep insertion order
            }
            SortField::GasPrice => {
                indices.sort_by(|&a, &b| {
                    let a_price = self.transactions[a].gas_price_f64; // Use pre-computed value!
                    let b_price = self.transactions[b].gas_price_f64; // Use pre-computed value!
                    b_price.partial_cmp(&a_price).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortField::Value => {
                indices.sort_by(|&a, &b| {
                    let a_val = self.transactions[a].value_f64; // Use pre-computed value!
                    let b_val = self.transactions[b].value_f64; // Use pre-computed value!
                    b_val.partial_cmp(&a_val).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            SortField::Nonce => {
                indices.sort_by_key(|&i| self.transactions[i].nonce);
            }
        }

        self.cached_filtered_sorted = indices;
        self.cache_dirty = false;
    }

    /// Get filtered and sorted transactions (uses cache)
    pub fn get_filtered_sorted_transactions(&mut self) -> Vec<&MempoolTransaction> {
        self.ensure_cache_valid();
        self.cached_filtered_sorted
            .iter()
            .map(|&i| &self.transactions[i])
            .collect()
    }

    /// Toggle the sort field
    pub fn toggle_sort(&mut self) {
        self.sort_field = self.sort_field.toggle();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.cache_dirty = true;
        self.needs_redraw = true;
    }

    /// Mark cache as dirty (for external updates like new transactions)
    pub fn mark_cache_dirty(&mut self) {
        self.cache_dirty = true;
    }
}
