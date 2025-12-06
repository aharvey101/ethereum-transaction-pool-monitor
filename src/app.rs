use crate::eth_client::{EthereumClient, MempoolTransaction};
use crate::pool_db::PoolDatabase;
use crate::coingecko::CoinGeckoClient;
use anyhow::Result;
use std::collections::VecDeque;
use chrono::Local;

const MAX_TRANSACTIONS_DISPLAY: usize = 1000;

/// Application state containing transaction data and UI state
pub struct AppState {
    pub transactions: VecDeque<MempoolTransaction>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub client: EthereumClient,
    pub pool_db: PoolDatabase,
    pub coingecko_client: CoinGeckoClient,
    pub chain_id: u32,
    pub status: String,
    pub is_running: bool,
    pub last_update: String,
    pub last_pool_sync: String,
    pub pool_count: u32,
    pub connection_healthy: bool,
    pub needs_redraw: bool,
}

impl AppState {
    /// Create a new application state
    pub async fn new(rpc_url: &str, db_path: &str, chain_id: u32) -> Result<Self> {
        let client = EthereumClient::new(rpc_url).await?;
        let pool_db = PoolDatabase::new(db_path)?;
        let coingecko_client = CoinGeckoClient::new();
        let pool_count = pool_db.pool_count().unwrap_or(0);
        
        Ok(AppState {
            transactions: VecDeque::new(),
            selected_index: 0,
            scroll_offset: 0,
            client,
            pool_db,
            coingecko_client,
            chain_id,
            status: "Initializing...".to_string(),
            is_running: true,
            last_update: "Never".to_string(),
            last_pool_sync: "Never".to_string(),
            pool_count,
            connection_healthy: true,
            needs_redraw: true,
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

    /// Scroll down in the transaction list
    pub fn scroll_down(&mut self, amount: usize, max_rows: usize) {
        let max_scroll = self.transactions.len().saturating_sub(max_rows);
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
        if !self.transactions.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.transactions.len();
            self.ensure_selection_visible(max_rows);
            self.needs_redraw = true;
        }
    }

    /// Select previous transaction and auto-scroll to keep it visible
    pub fn select_previous(&mut self, max_rows: usize) {
        if !self.transactions.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.transactions.len() - 1
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

    /// Get the currently selected transaction
    pub fn get_selected_transaction(&self) -> Option<&MempoolTransaction> {
        self.transactions.get(self.selected_index)
    }

    /// Get visible transactions for rendering (fits available terminal height)
    pub fn get_visible_transactions(&self, max_rows: usize) -> Vec<&MempoolTransaction> {
        self.transactions
            .iter()
            .skip(self.scroll_offset)
            .take(max_rows)
            .collect()
    }
}
