use crate::eth_client::{EthereumClient, MempoolTransaction};
use crate::pool_db::PoolDatabase;
use crate::pool_fetcher::PoolFetcher;
use crate::graph_client::GraphClient;
use anyhow::Result;
use std::collections::VecDeque;
use chrono::Local;

const MAX_TRANSACTIONS_DISPLAY: usize = 2000;

/// Filter mode for transaction display
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    All,
    DexOnly,
    TransfersSwapsOnly,
}

impl FilterMode {
    pub fn toggle(self) -> Self {
        match self {
            FilterMode::All => FilterMode::DexOnly,
            FilterMode::DexOnly => FilterMode::TransfersSwapsOnly,
            FilterMode::TransfersSwapsOnly => FilterMode::All,
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
            SortField::Default => "Default Order",
            SortField::GasPrice => "Gas Price (High→Low)",
            SortField::Value => "Value (High→Low)",
            SortField::Nonce => "Nonce (Low→High)",
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
    pub pool_fetcher: PoolFetcher,
    pub graph_client: GraphClient,
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
    pub v4_pools_found: u32,
    pub sushi_pools_found: u32,
    pub curve_pools_found: u32,
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
    // Pagination fields
    pub is_loading_more: bool,
    pub has_more_data: bool,
    pub should_load_more: bool, // flag to trigger loading more data
    // Next block tracking
    pub current_block_number: Option<u64>,
    pub next_block_number: u64,
}

impl AppState {
    /// Create a new application state
    pub async fn new(rpc_url: &str, db_path: &str, chain_id: u32) -> Result<Self> {
        let client = EthereumClient::new(rpc_url).await?;
        let pool_db = PoolDatabase::new(db_path)?;
        let pool_fetcher = PoolFetcher::new(rpc_url);
        let api_key = "79942a724597827e4cb8972667c0a355".to_string(); // The Graph API key
        let graph_client = GraphClient::new(api_key);
        let pool_count = pool_db.pool_count().unwrap_or(0);
        
         // Get initial block number
         let current_block = client.get_latest_block_number().await.unwrap_or(0);
         
         Ok(AppState {
             transactions: VecDeque::new(),
             selected_index: 0,
             scroll_offset: 0,
             client,
             pool_db,
             pool_fetcher,
             graph_client,
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
             v4_pools_found: 0,
             sushi_pools_found: 0,
             curve_pools_found: 0,
             pool_loading_progress_percent: 0,
             filter_mode: FilterMode::All,
             sort_field: SortField::Default,
             cached_filtered_count_display: 0,
             cached_title: " Pending Transactions (0) ".to_string(),
             cached_filtered_sorted: Vec::new(),
             cached_filtered_count: 0,
             cache_dirty: true,
             // Initialize pagination fields
             is_loading_more: false,
             has_more_data: true,
             should_load_more: false,
             // Initialize block tracking
             current_block_number: Some(current_block),
             next_block_number: current_block + 1,
         })
     }

    /// Update transactions from the mempool
    pub async fn update_transactions(&mut self) -> Result<()> {
        // First, check if a new block has been mined
        let latest_block = self.client.get_latest_block_number().await.unwrap_or(0);
        let block_changed = self.current_block_number.map_or(true, |current| latest_block > current);
        
        if block_changed {
            tracing::info!("New block detected! Previous: {:?}, Current: {}", 
                self.current_block_number, latest_block);
            
            // Clear transactions as they were targeting the previous block
            self.transactions.clear();
            self.current_block_number = Some(latest_block);
            self.next_block_number = latest_block + 1;
            
            tracing::info!("Cleared transactions - now targeting block #{}", self.next_block_number);
        }
        
        match self.client.get_pending_transactions(&self.pool_db, self.chain_id).await {
            Ok(new_txs) => {
                // If no new block, preserve existing transactions and add any new ones
                if !block_changed {
                    // Keep existing transactions and add new ones
                    for tx in new_txs {
                        // Check if this transaction is already in our list
                        if !self.transactions.iter().any(|existing| existing.hash == tx.hash) {
                            self.transactions.push_back(tx);
                        }
                    }
                } else {
                    // New block - replace all transactions
                    for tx in new_txs {
                        self.transactions.push_back(tx);
                    }
                }
                
                // Limit total transactions
                while self.transactions.len() > MAX_TRANSACTIONS_DISPLAY {
                    self.transactions.pop_front();
                }
                
                let status_msg = if block_changed {
                    format!("{} transactions targeting block #{}", 
                        self.transactions.len(), self.next_block_number)
                } else {
                    format!("{} pending transactions targeting block #{}", 
                        self.transactions.len(), self.next_block_number)
                };
                
                self.status = status_msg;
                self.last_update = Local::now().format("%H:%M:%S").to_string();
                self.connection_healthy = true;
                self.cache_dirty = true; // Mark cache as dirty when transactions update
                self.cached_title = format!(" Targeting Block #{} ({} txs) ", 
                    self.next_block_number, self.transactions.len()); // Update cached title
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

    /// Sync DEX pools directly from Ethereum node
    /// Comprehensive pool sync using The Graph Protocol (recommended)
    pub async fn sync_pools_comprehensive(&mut self) -> Result<()> {
        let existing_pool_count = self.pool_db.pool_count().unwrap_or(0);
        
        if existing_pool_count > 100_000 {
            tracing::info!("Pool count already high ({}), skipping sync", existing_pool_count);
            self.pools_loading_progress = format!("Pool database complete: {} pools", existing_pool_count);
            self.pool_count = existing_pool_count;
            return Ok(());
        } else {
            tracing::info!("Pool count low ({}), syncing pools from The Graph Protocol", existing_pool_count);
        }
        
        self.is_loading_pools = true;
        self.pools_found_count = 0;
        self.needs_redraw = true;
        
        // Use The Graph Protocol for comprehensive collection
        self.pools_loading_progress = "Collecting pools from The Graph Protocol...".to_string();
        self.needs_redraw = true;
        
        let sushiswap_subgraph_id = "2tGWMrDha4164KkFAfkU3rDCtuxGb4q1emXmFdLLzJ8x";
        let curve_subgraph_id = "3fy93eAT56UJsRCEht8iFhfi6wjHWXtZ9dnnbQmvFopF";
        
        match self.graph_client.populate_database_comprehensive(&self.pool_db, Some(sushiswap_subgraph_id), Some(curve_subgraph_id)).await {
            Ok((v2_count, v3_count, v4_count, sushi_count, curve_count)) => {
                let total = v2_count + v3_count + v4_count + sushi_count + curve_count;
                tracing::info!("✅ Comprehensive sync complete: {} total pools", total);
                self.pools_found_count = total;
                self.v2_pools_found = v2_count;
                self.v3_pools_found = v3_count; 
                self.v4_pools_found = v4_count;
                self.sushi_pools_found = sushi_count;
                self.curve_pools_found = curve_count;
                self.pool_count = total;
                self.pools_loading_progress = format!("✅ Complete: {} pools (V2:{} V3:{} V4:{} Sushi:{} Curve:{})", 
                    total, v2_count, v3_count, v4_count, sushi_count, curve_count);
                self.last_pool_sync = Local::now().format("%H:%M:%S").to_string();
                self.needs_redraw = true;
            }
            Err(e) => {
                tracing::error!("Failed to sync pools comprehensively: {}", e);
                self.pools_loading_progress = format!("❌ Sync failed: {}", e);
                self.needs_redraw = true;
            }
        }
        
        self.is_loading_pools = false;
        Ok(())
    }

    /// Legacy pool sync using RPC calls (slower, limited coverage)
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

        // Fetch UniswapV4 pools from node
        tracing::info!("Fetching UniswapV4 pools from node");
        self.pools_loading_progress = "UniswapV4: Checking deployment...".to_string();
        self.needs_redraw = true;
        
        match self.pool_fetcher.fetch_uniswap_v4_pools(&self.pool_db, self.chain_id).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Synced {} UniswapV4 pools from node", count);
                    total += count;
                    self.pools_found_count = total;
                    self.pools_loading_progress = format!("UniswapV4: Complete! {} pools found", count);
                } else {
                    tracing::info!("UniswapV4 not yet deployed - skipping");
                    self.pools_loading_progress = "UniswapV4: Not deployed yet".to_string();
                }
                self.needs_redraw = true;
            }
            Err(e) => {
                tracing::warn!("Failed to check UniswapV4 pools: {}", e);
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
        let new_scroll_offset = (self.scroll_offset + amount).min(max_scroll);
        
        // Check if we're near the bottom (within 10 rows) and should load more
        let threshold = 10; // Load more when within 10 rows of bottom
        if !self.is_loading_more && 
           self.has_more_data && 
           new_scroll_offset + max_rows + threshold >= filtered_count {
            self.should_load_more = true;
        }
        
        self.scroll_offset = new_scroll_offset;
        self.needs_redraw = true;
    }

    /// Scroll up in the transaction list
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        self.needs_redraw = true;
    }

    /// Select next transaction and auto-scroll to keep it visible
    pub fn select_next(&mut self, max_rows: usize) {
        if self.transactions.is_empty() {
            return;
        }
        
        let filtered_count = self.get_filtered_transaction_count();
        if filtered_count > 0 && self.selected_index < filtered_count - 1 {
            self.selected_index += 1;
            self.ensure_selection_visible(max_rows);
            self.needs_redraw = true;
        }
    }

    /// Select previous transaction and auto-scroll to keep it visible  
    pub fn select_previous(&mut self, max_rows: usize) {
        if self.transactions.is_empty() {
            return;
        }
        
        if self.selected_index > 0 {
            self.selected_index -= 1;
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

    /// Get the count of transactions that match current filter (immutable, for UI)
    pub fn get_filtered_transaction_count_display(&self) -> usize {
        self.cached_filtered_count_display
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
                    FilterMode::TransfersSwapsOnly => {
                        tx.swap_info.is_some()
                    }
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

    /// Load more pending transactions (for pagination)
    pub async fn load_more_transactions(&mut self) -> Result<()> {
        if self.is_loading_more || !self.has_more_data {
            return Ok(());
        }

        self.is_loading_more = true;
        self.should_load_more = false;

        // Calculate offset based on current transaction count
        let current_count = self.transactions.len();
        let batch_size = 500; // Load 500 more pending transactions at a time

        tracing::info!("Loading more pending transactions (offset: {}, batch: {})", current_count, batch_size);

        // Fetch more pending transactions with pagination
        match self.client.get_pending_transactions_paginated(&self.pool_db, self.chain_id, current_count, batch_size).await {
            Ok(more_pending_txs) => {
                if more_pending_txs.is_empty() {
                    self.has_more_data = false;
                    tracing::info!("No more pending transactions available for pagination");
                } else {
                    let tx_count = more_pending_txs.len();
                    tracing::info!("Found {} more pending transactions", tx_count);
                    
                    // Add pending transactions to the end of the deque
                    for tx in more_pending_txs {
                        self.transactions.push_back(tx);
                    }
                    
                    // Limit total transactions to prevent memory issues
                    let original_len = self.transactions.len();
                    while self.transactions.len() > 5000 {
                        self.transactions.pop_front();
                    }
                    if original_len > 5000 {
                        tracing::info!("Limited transactions from {} to {} to prevent memory issues", 
                            original_len, self.transactions.len());
                    }
                    
                    // If we got fewer transactions than requested, we've reached the end
                    if tx_count < batch_size {
                        self.has_more_data = false;
                        tracing::info!("Reached end of pending transactions (got {} < {})", tx_count, batch_size);
                    }
                    
                    self.mark_cache_dirty();
                    self.needs_redraw = true;
                    tracing::info!("Loaded {} more pending transactions, total: {}", 
                        tx_count, self.transactions.len());
                }
            }
            Err(e) => {
                tracing::error!("Failed to load more pending transactions: {}", e);
                // Don't set has_more_data to false on error, might be temporary
            }
        }

        self.is_loading_more = false;
        Ok(())
    }

    /// Check if we should load more data (for external polling)
    pub fn should_load_more(&self) -> bool {
        self.should_load_more && !self.is_loading_more && self.has_more_data
    }
}
