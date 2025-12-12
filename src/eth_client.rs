use crate::pool_db::PoolDatabase;
use crate::transaction_decoder::SwapInfo;
use anyhow::Result;
use serde_json::{self, json};
use std::sync::Arc;
use tracing::debug;

/// Type of DeFi activity detected in a transaction
#[derive(Clone, Debug, PartialEq)]
pub enum DefiActivityType {
    None,
    TokenContract, // USDT, USDC, WETH, etc.
    Stablecoin,    // USDT, USDC, DAI, BUSD - subset of tokens
    DexPool,       // Uniswap pools, etc.
    DexRouter,     // Uniswap router, 1inch, etc.
}

/// Represents a pending transaction from the mempool
#[derive(Clone, Debug)]
pub struct MempoolTransaction {
    #[allow(dead_code)]
    pub hash: String,
    pub from: String,
    pub to: Option<String>,
    #[allow(dead_code)]
    pub value: String,
    #[allow(dead_code)]
    pub gas: String,
    #[allow(dead_code)]
    pub gas_price: String,
    pub nonce: u64,
    #[allow(dead_code)]
    pub data: String,
    #[allow(dead_code)]
    pub block_hash: Option<String>,
    #[allow(dead_code)]
    pub block_number: Option<String>,
    #[allow(dead_code)]
    pub transaction_index: Option<String>,
    // Cached formatted values for UI rendering
    pub value_eth: String,
    pub gas_price_gwei: String,
    // Pre-computed numeric values for fast sorting (eliminates string parsing)
    pub value_f64: f64,
    pub gas_price_f64: f64,
    pub is_dex: bool,
    pub defi_activity_type: DefiActivityType,
    pub swap_info: Option<SwapInfo>,
}

impl MempoolTransaction {
    pub fn from_value(
        value: &serde_json::Value,
        pool_db: &PoolDatabase,
        chain_id: u32,
    ) -> Result<Self> {
        let from = value["from"].as_str().unwrap_or("N/A").to_string();
        let to_opt = value["to"].as_str().map(|s| s.to_string());
        let value_hex = value["value"].as_str().unwrap_or("0x0").to_string();
        let gas_price_hex = value["gasPrice"].as_str().unwrap_or("0x0").to_string();

        // Parse nonce from hex to u64
        let nonce_hex = value["nonce"].as_str().unwrap_or("0x0");
        let nonce = parse_hex_u64(nonce_hex).unwrap_or(0);

        let value_eth = format_value(&value_hex);
        let gas_price_gwei = format_gas_price(&gas_price_hex);

        // Pre-compute numeric values for fast sorting (eliminates repeated string parsing)
        let value_f64 = value_eth.parse::<f64>().unwrap_or(0.0);
        let gas_price_f64 = gas_price_gwei.parse::<f64>().unwrap_or(0.0);

        // Determine DeFi activity type and set is_dex flag
        let (is_dex, defi_activity_type) =
            to_opt
                .as_ref()
                .map_or((false, DefiActivityType::None), |addr| {
                    let activity_type = pool_db
                        .get_defi_activity_type(addr, chain_id)
                        .unwrap_or(DefiActivityType::None);
                    let is_defi = activity_type != DefiActivityType::None;
                    if is_defi {
                        tracing::debug!(
                        "DeFi transaction detected - Address: {}, Type: {:?}, Value: {}, Gas: {}",
                        addr,
                        activity_type,
                        value_eth,
                        gas_price_gwei
                    );
                    }
                    (is_defi, activity_type)
                });

        // Extract transaction data for potential decoding
        let tx_data = value["input"].as_str().unwrap_or("0x").to_string();

        // Decode transaction based on type: swaps for routers, transfers for tokens
        let swap_info = if is_dex {
            if let Some(to_addr) = &to_opt {
                let decoder = crate::transaction_decoder::TransactionDecoder::new();
                if pool_db.is_dex_router(to_addr) {
                    // Router transaction - decode as swap
                    decoder.decode_swap(to_addr, &tx_data, &value_hex)
                } else if pool_db.is_token_contract(to_addr) {
                    // Token contract - decode as transfer
                    decoder.decode_token_transfer(to_addr, &tx_data)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(MempoolTransaction {
            hash: value["hash"].as_str().unwrap_or("N/A").to_string(),
            from,
            to: to_opt,
            value: value_hex,
            gas: value["gas"].as_str().unwrap_or("0x0").to_string(),
            gas_price: gas_price_hex,
            nonce,
            data: tx_data,
            block_hash: value["blockHash"].as_str().map(|s| s.to_string()),
            block_number: value["blockNumber"].as_str().map(|s| s.to_string()),
            transaction_index: value["transactionIndex"].as_str().map(|s| s.to_string()),
            value_eth,
            gas_price_gwei,
            value_f64,
            gas_price_f64,
            is_dex,
            defi_activity_type,
            swap_info,
        })
    }
}

/// Parse hex string to u64
fn parse_hex_u64(hex: &str) -> Result<u64> {
    let trimmed = hex.trim_start_matches("0x").trim_start_matches("0X");
    if trimmed.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(trimmed, 16).map_err(|e| anyhow::anyhow!("Failed to parse hex: {}", e))
}

/// Format value in Wei to ETH representation
fn format_value(value_hex: &str) -> String {
    if value_hex == "0x0" || value_hex == "0x" {
        "0 ETH".to_string()
    } else {
        match u128::from_str_radix(value_hex.trim_start_matches("0x"), 16) {
            Ok(value) => {
                let eth = value as f64 / 1e18;
                format!("{:.4} ETH", eth)
            }
            Err(_) => value_hex.to_string(),
        }
    }
}

/// Format gas price from Wei to Gwei
fn format_gas_price(gas_price_hex: &str) -> String {
    if gas_price_hex == "0x0" || gas_price_hex == "0x" {
        "0 Gwei".to_string()
    } else {
        match u64::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16) {
            Ok(price) => {
                let gwei = price as f64 / 1e9;
                format!("{:.2} Gwei", gwei)
            }
            Err(_) => gas_price_hex.to_string(),
        }
    }
}

/// Ethereum client for connecting to a local node
#[derive(Clone)]
pub struct EthereumClient {
    rpc_url: String,
    http_client: Arc<reqwest::Client>,
}

impl EthereumClient {
    /// Create a new Ethereum client connected to the specified RPC URL
    pub async fn new(rpc_url: &str) -> Result<Self> {
        Ok(EthereumClient {
            rpc_url: rpc_url.to_string(),
            http_client: Arc::new(reqwest::Client::new()),
        })
    }

    /// Fetch pending transactions - tries txpool_content first, then filter-based approach
    pub async fn get_pending_transactions(
        &self,
        pool_db: &PoolDatabase,
        chain_id: u32,
    ) -> Result<Vec<MempoolTransaction>> {
        // Try txpool_content first (works with Reth, Geth, Erigon)
        match self.get_pending_from_txpool(pool_db, chain_id).await {
            Ok(txs) => {
                if !txs.is_empty() {
                    return Ok(txs);
                }
            }
            Err(_) => {
                // If txpool_content fails, try filter-based approach
            }
        }

        // Fallback to filter-based approach
        self.get_pending_from_filter(pool_db, chain_id).await
    }

    /// Get pending transactions from txpool_content (most direct method)
    async fn get_pending_from_txpool(
        &self,
        pool_db: &PoolDatabase,
        chain_id: u32,
    ) -> Result<Vec<MempoolTransaction>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "txpool_content",
            "params": [],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("txpool_content error: {}", error["message"]);
        }

        let result = response_body
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        let pending = result
            .get("pending")
            .ok_or_else(|| anyhow::anyhow!("No pending key in result"))?;

        let mut transactions = Vec::new();

        // pending is a map of address -> map of nonce -> tx
        if let Some(pending_map) = pending.as_object() {
            for (_address, nonce_map) in pending_map {
                if let Some(txs_by_nonce) = nonce_map.as_object() {
                    for (_nonce, tx) in txs_by_nonce {
                        if let Ok(tx_data) = MempoolTransaction::from_value(tx, pool_db, chain_id) {
                            transactions.push(tx_data);
                        }
                    }
                }
            }
        }

        Ok(transactions)
    }

    /// Fallback: Get pending transactions using filter-based approach
    async fn get_pending_from_filter(
        &self,
        pool_db: &PoolDatabase,
        chain_id: u32,
    ) -> Result<Vec<MempoolTransaction>> {
        // Create filter
        let filter_request = json!({
            "jsonrpc": "2.0",
            "method": "eth_newPendingTransactionFilter",
            "params": [],
            "id": 1
        });

        let filter_response = self
            .http_client
            .post(&self.rpc_url)
            .json(&filter_request)
            .send()
            .await?;

        let filter_body: serde_json::Value = filter_response.json().await?;

        if let Some(error) = filter_body.get("error") {
            anyhow::bail!("Filter creation error: {}", error["message"]);
        }

        let filter_id = filter_body
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| anyhow::anyhow!("No filter ID returned"))?;

        // Get filter changes
        let changes_request = json!({
            "jsonrpc": "2.0",
            "method": "eth_getFilterChanges",
            "params": [filter_id],
            "id": 1
        });

        let changes_response = self
            .http_client
            .post(&self.rpc_url)
            .json(&changes_request)
            .send()
            .await?;

        let changes_body: serde_json::Value = changes_response.json().await?;

        if let Some(error) = changes_body.get("error") {
            anyhow::bail!("Filter changes error: {}", error["message"]);
        }

        let hashes = changes_body
            .get("result")
            .and_then(|r| r.as_array())
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|h| h.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();

        // Fetch full transaction details for each hash
        let mut transactions = Vec::new();
        for hash in hashes {
            match self.get_transaction_by_hash(&hash, pool_db, chain_id).await {
                Ok(tx) => transactions.push(tx),
                Err(_) => {
                    // Skip individual transaction errors
                }
            }
        }

        Ok(transactions)
    }

    /// Fetch full transaction details by hash
    pub async fn get_transaction_by_hash(
        &self,
        hash: &str,
        pool_db: &PoolDatabase,
        chain_id: u32,
    ) -> Result<MempoolTransaction> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [hash],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let result = response_body
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;

        MempoolTransaction::from_value(result, pool_db, chain_id)
    }

    /// Get pending transactions with pagination support, sorted by likelihood of being in next block
    pub async fn get_pending_transactions_paginated(
        &self,
        pool_db: &PoolDatabase,
        chain_id: u32,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MempoolTransaction>> {
        // Get raw pending transactions without expensive processing
        let raw_txs = self.get_pending_transactions_raw().await?;

        // Sort by gas price (parsing hex directly for speed)
        let mut tx_with_gas: Vec<_> = raw_txs
            .into_iter()
            .filter_map(|tx| {
                let gas_price_hex = tx.get("gasPrice")?.as_str()?;
                let gas_price_wei =
                    u64::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16).ok()?;
                Some((tx, gas_price_wei))
            })
            .collect();

        // Sort by gas price descending (highest first)
        tx_with_gas.sort_by(|a, b| b.1.cmp(&a.1));

        // Apply pagination and then process only the transactions we need
        let start = offset;
        let end = (offset + limit).min(tx_with_gas.len());

        if start >= tx_with_gas.len() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for (tx_value, _gas_price) in &tx_with_gas[start..end] {
            if let Ok(tx) = MempoolTransaction::from_value(tx_value, pool_db, chain_id) {
                result.push(tx);
            }
        }

        Ok(result)
    }

    /// Get raw pending transaction data without expensive processing
    async fn get_pending_transactions_raw(&self) -> Result<Vec<serde_json::Value>> {
        // Try txpool_content first (works with Reth, Geth, Erigon)
        match self.get_pending_raw_from_txpool().await {
            Ok(txs) => {
                if !txs.is_empty() {
                    return Ok(txs);
                }
            }
            Err(_) => {
                // If txpool_content fails, try filter-based approach
            }
        }

        // Fallback to filter-based approach
        self.get_pending_raw_from_filter().await
    }

    /// Get raw pending transactions from txpool_content (fast, no processing)
    async fn get_pending_raw_from_txpool(&self) -> Result<Vec<serde_json::Value>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "txpool_content",
            "params": [],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("txpool_content error: {}", error["message"]);
        }

        let result = response_body
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        let pending = result
            .get("pending")
            .ok_or_else(|| anyhow::anyhow!("No pending key in result"))?;

        let mut transactions = Vec::new();

        // pending is a map of address -> map of nonce -> tx
        if let Some(pending_map) = pending.as_object() {
            for (_address, nonce_map) in pending_map {
                if let Some(txs_by_nonce) = nonce_map.as_object() {
                    for (_nonce, tx) in txs_by_nonce {
                        transactions.push(tx.clone());
                    }
                }
            }
        }

        Ok(transactions)
    }

    /// Fallback: Get raw pending transactions using filter-based approach
    async fn get_pending_raw_from_filter(&self) -> Result<Vec<serde_json::Value>> {
        // Create filter
        let filter_request = json!({
            "jsonrpc": "2.0",
            "method": "eth_newPendingTransactionFilter",
            "params": [],
            "id": 1
        });

        let filter_response = self
            .http_client
            .post(&self.rpc_url)
            .json(&filter_request)
            .send()
            .await?;

        let filter_body: serde_json::Value = filter_response.json().await?;

        if let Some(error) = filter_body.get("error") {
            anyhow::bail!("Filter creation error: {}", error["message"]);
        }

        let filter_id = filter_body
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| anyhow::anyhow!("No filter ID returned"))?;

        // Get filter changes
        let changes_request = json!({
            "jsonrpc": "2.0",
            "method": "eth_getFilterChanges",
            "params": [filter_id],
            "id": 1
        });

        let changes_response = self
            .http_client
            .post(&self.rpc_url)
            .json(&changes_request)
            .send()
            .await?;

        let changes_body: serde_json::Value = changes_response.json().await?;

        if let Some(error) = changes_body.get("error") {
            anyhow::bail!("Filter changes error: {}", error["message"]);
        }

        let hashes = changes_body
            .get("result")
            .and_then(|r| r.as_array())
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|h| h.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();

        // Fetch full transaction details for each hash
        let mut transactions = Vec::new();
        for hash in hashes {
            match self.get_transaction_raw_by_hash(&hash).await {
                Ok(tx) => transactions.push(tx),
                Err(_) => {
                    // Skip individual transaction errors
                }
            }
        }

        Ok(transactions)
    }

    /// Fetch raw transaction details by hash (no processing)
    async fn get_transaction_raw_by_hash(&self, hash: &str) -> Result<serde_json::Value> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [hash],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let result = response_body
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;

        Ok(result.clone())
    }

    /// Add missing get_gas_price method for compatibility
    pub async fn get_gas_price(&self) -> Result<alloy_primitives::U256> {
        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_gasPrice",
            "params": [],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let gas_price_hex = response_body["result"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid gas price response"))?;

        let gas_price =
            alloy_primitives::U256::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16)?;

        Ok(gas_price)
    }

    /// Get current effective gas price using EIP-1559 base fee + priority fee
    /// This is more accurate than the legacy eth_gasPrice for modern Ethereum
    pub async fn get_current_gas_price_wei(&self) -> Result<u64> {
        // Get base fee from latest block (EIP-1559)
        let base_fee_gwei = self.get_current_base_fee().await.unwrap_or(0.1); // fallback to 0.1 gwei

        // Add a small priority fee (0.1 gwei) to ensure inclusion
        let priority_fee_gwei = 0.1;
        let total_gas_price_gwei = base_fee_gwei + priority_fee_gwei;

        // Convert to wei (1 gwei = 1e9 wei)
        let gas_price_wei = (total_gas_price_gwei * 1_000_000_000.0) as u64;

        Ok(gas_price_wei)
    }

    /// Get current gas price with fallback to legacy method
    pub async fn get_current_gas_price(&self) -> Result<alloy_primitives::U256> {
        // Try EIP-1559 method first
        match self.get_current_gas_price_wei().await {
            Ok(gas_price_wei) => Ok(alloy_primitives::U256::from(gas_price_wei)),
            Err(_) => {
                // Fallback to legacy gas price method
                self.get_gas_price().await
            }
        }
    }

    /// Test and display current gas price information
    pub async fn test_gas_prices(&self) -> Result<()> {
        println!("🔍 Testing Dynamic Gas Price Fetching");

        // Test legacy gas price
        println!("\n📊 Legacy Gas Price (eth_gasPrice):");
        match self.get_gas_price().await {
            Ok(gas_price) => {
                let gas_price_gwei =
                    gas_price.to_string().parse::<u128>().unwrap_or(0) as f64 / 1e9;
                println!("   Legacy: {:.4} gwei ({} wei)", gas_price_gwei, gas_price);
            }
            Err(e) => println!("   Error: {}", e),
        }

        // Test current base fee
        println!("\n📊 Current Base Fee (EIP-1559):");
        match self.get_current_base_fee().await {
            Ok(base_fee) => {
                println!("   Base Fee: {:.4} gwei", base_fee);
            }
            Err(e) => println!("   Error: {}", e),
        }

        // Test our new combined gas price
        println!("\n📊 New Dynamic Gas Price:");
        match self.get_current_gas_price_wei().await {
            Ok(gas_price_wei) => {
                let gas_price_gwei = gas_price_wei as f64 / 1e9;
                println!(
                    "   Dynamic: {:.4} gwei ({} wei)",
                    gas_price_gwei, gas_price_wei
                );
            }
            Err(e) => println!("   Error: {}", e),
        }

        // Calculate cost estimates with different scenarios
        println!("\n💰 Gas Cost Estimates (OLD vs NEW):");
        let current_gas_price = self
            .get_current_gas_price_wei()
            .await
            .unwrap_or(500_000_000); // 0.5 gwei fallback
        let old_gas_price = 20_000_000_000u64; // 20 gwei

        let scenarios = [
            ("Simple swap", 150_000u64),
            ("Complex swap", 180_000u64),
            ("Sandwich attack", 400_000u64),
            ("OLD estimate", 550_000u64),
        ];

        for (name, gas_limit) in scenarios.iter() {
            let old_cost_wei = gas_limit * old_gas_price;
            let new_cost_wei = gas_limit * current_gas_price;
            let old_cost_eth = old_cost_wei as f64 / 1e18;
            let new_cost_eth = new_cost_wei as f64 / 1e18;
            let savings = old_cost_eth - new_cost_eth;
            let percent_savings = (savings / old_cost_eth) * 100.0;

            println!(
                "   {}: OLD {:.6} ETH vs NEW {:.6} ETH (Save {:.6} ETH, {:.1}%)",
                name, old_cost_eth, new_cost_eth, savings, percent_savings
            );
        }

        println!("\n✅ Gas price testing completed!");
        Ok(())
    }

    /// Add missing get_block_number method for compatibility
    pub async fn get_block_number(&self) -> Result<u64> {
        self.get_latest_block_number().await
    }

    /// Get transaction receipt by hash
    pub async fn get_transaction_receipt(
        &self,
        tx_hash: &str,
    ) -> Result<Option<serde_json::Value>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let result = response_body.get("result");

        match result {
            Some(receipt_data) if !receipt_data.is_null() => Ok(Some(receipt_data.clone())),
            _ => Ok(None), // Transaction not found or null (still pending)
        }
    }

    /// Subscribe to pending transactions via WebSocket for real-time mempool monitoring
    pub async fn subscribe_pending_transactions(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<String>> {
        // Convert HTTP URL to WebSocket URL and change port from 8545 to 8547
        let mut ws_url = self
            .rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        // Change port from 8545 to 8547 for WebSocket
        if ws_url.contains(":8545") {
            ws_url = ws_url.replace(":8545", ":8547");
        }

        debug!("📡 WebSocket URL: {}", ws_url);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Use tokio-tungstenite for reliable WebSocket connection
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

        let url = url::Url::parse(&ws_url)?;

        tokio::spawn(async move {
            loop {
                // Attempt WebSocket connection with retry logic
                match connect_async(&url).await {
                    Ok((ws_stream, _)) => {
                        debug!("✅ WebSocket connected successfully");
                        let (mut write, mut read) = ws_stream.split();

                        // Subscribe to pending transactions
                        let subscribe_msg = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": "eth_subscribe",
                            "params": ["newPendingTransactions"],
                            "id": 1
                        });

                        if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                            debug!("❌ Failed to send subscription: {}", e);
                            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                            continue;
                        }

                        // Process incoming messages
                        while let Some(message) = read.next().await {
                            match message {
                                Ok(Message::Text(text)) => {
                                    debug!("📨 Received WebSocket message: {}", text);
                                    
                                    if let Ok(json) =
                                        serde_json::from_str::<serde_json::Value>(&text)
                                    {
                                        // Handle subscription confirmation
                                        if json.get("result").is_some()
                                            && json.get("id") == Some(&serde_json::json!(1))
                                        {
                                            debug!("✅ Subscription confirmed");
                                            continue;
                                        }

                                        // Handle new pending transaction notifications
                                        if let Some(params) = json.get("params") {
                                            if let Some(result) = params.get("result") {
                                                if let Some(tx_hash) = result.as_str() {
                                                    debug!("📨 Got transaction hash: {}", tx_hash);
                                                    if tx.send(tx_hash.to_string()).is_err() {
                                                        debug!("📡 Receiver dropped, closing WebSocket");
                                                        break;
                                                    }
                                                } else {
                                                    debug!("❓ Result is not a string: {:?}", result);
                                                }
                                            } else {
                                                debug!("❓ No result in params: {:?}", params);
                                            }
                                        } else {
                                            debug!("❓ Unknown WebSocket message format: {:?}", json);
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) => {
                                    debug!("📡 WebSocket closed by server");
                                    break;
                                }
                                Err(e) => {
                                    debug!("❌ WebSocket error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        debug!("❌ WebSocket connection failed: {}", e);
                    }
                }

                // Wait before reconnecting
                debug!("🔄 Reconnecting WebSocket in 5 seconds...");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        Ok(rx)
    }

    /// Polling fallback for pending transactions when WebSocket doesn't broadcast events
    pub async fn poll_pending_transactions(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<String>> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let client = self.clone();
        
        tokio::spawn(async move {
            let mut known_transactions = std::collections::HashSet::new();
            
            loop {
                match client.get_pending_transaction_hashes().await {
                    Ok(tx_hashes) => {
                        // Send only new transaction hashes
                        for hash in tx_hashes {
                            if !known_transactions.contains(&hash) {
                                known_transactions.insert(hash.clone());
                                
                                // Clean up old hashes periodically (keep last 10k)
                                if known_transactions.len() > 10000 {
                                    let mut to_remove = Vec::new();
                                    for (i, h) in known_transactions.iter().enumerate() {
                                        if i > known_transactions.len() - 5000 {
                                            break;
                                        }
                                        to_remove.push(h.clone());
                                    }
                                    for h in to_remove {
                                        known_transactions.remove(&h);
                                    }
                                }
                                
                                if tx.send(hash).is_err() {
                                    debug!("📡 Receiver dropped, stopping polling");
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!("❌ Failed to poll pending transactions: {}", e);
                    }
                }
                
                // Poll every 2 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });
        
        Ok(rx)
    }

    /// Get pending transaction hashes from txpool
    async fn get_pending_transaction_hashes(&self) -> Result<Vec<String>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "txpool_content",
            "params": [],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let mut tx_hashes = Vec::new();

        // Extract pending transaction hashes from txpool_content
        if let Some(result) = response_body.get("result") {
            if let Some(pending) = result.get("pending") {
                if let Some(pending_obj) = pending.as_object() {
                    for (_, account_txs) in pending_obj {
                        if let Some(account_obj) = account_txs.as_object() {
                            for (_, tx_data) in account_obj {
                                if let Some(hash) = tx_data.get("hash") {
                                    if let Some(hash_str) = hash.as_str() {
                                        tx_hashes.push(hash_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        debug!("📊 Found {} pending transactions via polling", tx_hashes.len());
        Ok(tx_hashes)
    }

    /// Get full transaction details by hash using alloy types
    pub async fn get_transaction_details(
        &self,
        tx_hash: &str,
    ) -> Result<Option<alloy::rpc::types::Transaction>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [tx_hash],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let result = response_body.get("result");

        match result {
            Some(tx_data) if !tx_data.is_null() => {
                // Parse the transaction data into alloy Transaction type
                let tx: alloy::rpc::types::Transaction = serde_json::from_value(tx_data.clone())?;
                Ok(Some(tx))
            }
            _ => Ok(None), // Transaction not found or null
        }
    }

    /// Get the current latest block number
    pub async fn get_latest_block_number(&self) -> Result<u64> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let block_hex = response_body
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| anyhow::anyhow!("No block number returned"))?;

        let block_number = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16)?;
        Ok(block_number)
    }

    /// Get the current base fee from the latest block (for EIP-1559 transactions)
    pub async fn get_current_base_fee(&self) -> Result<f64> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["latest", false],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let block = response_body
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("Block not found"))?;

        // Parse base fee per gas (EIP-1559)
        if let Some(base_fee_hex) = block.get("baseFeePerGas").and_then(|v| v.as_str()) {
            let base_fee_wei = u64::from_str_radix(base_fee_hex.trim_start_matches("0x"), 16)?;
            let base_fee_gwei = base_fee_wei as f64 / 1_000_000_000.0;
            Ok(base_fee_gwei)
        } else {
            // Fallback for pre-EIP-1559 blocks
            Ok(0.0)
        }
    }

    /// Calculate optimal gas price for front-running a specific transaction
    pub async fn calculate_frontrun_gas_price(
        &self,
        target_tx_gas_price: alloy_primitives::U256,
        aggressive: bool,
    ) -> Result<alloy_primitives::U256> {
        let target_gwei =
            target_tx_gas_price.to_string().parse::<u64>().unwrap_or(0) as f64 / 1_000_000_000.0;

        let frontrun_gwei = if aggressive {
            // Aggressive: 20% higher than victim or current gas price + 5 gwei, whichever is higher
            let current_gas_price = self.get_gas_price().await?;
            let current_gwei =
                current_gas_price.to_string().parse::<u64>().unwrap_or(0) as f64 / 1_000_000_000.0;
            (target_gwei * 1.2).max(current_gwei + 5.0)
        } else {
            // Conservative: 5% higher than victim
            target_gwei * 1.05
        };

        let frontrun_wei = (frontrun_gwei * 1_000_000_000.0) as u64;
        Ok(alloy_primitives::U256::from(frontrun_wei))
    }

    /// Get the contract code at a given address
    pub async fn get_code(&self, address: alloy_primitives::Address) -> Result<String> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getCode",
            "params": [format!("{:#x}", address), "latest"],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        let code = response_body
            .get("result")
            .and_then(|r| r.as_str())
            .ok_or_else(|| anyhow::anyhow!("No code result"))?;

        Ok(code.to_string())
    }

    /// Get block by number with transaction hashes
    pub async fn get_block_by_number(&self, block_number: u64, include_transactions: bool) -> Result<Option<serde_json::Value>> {
        let block_param = if block_number == u64::MAX {
            "latest".to_string()
        } else {
            format!("0x{:x}", block_number)
        };

        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [block_param, include_transactions],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("JSON-RPC Error: {}", error);
        }

        Ok(response_body.get("result").cloned())
    }

    /// Get block transactions by block number
    pub async fn get_block_transactions(&self, block_number: u64) -> Result<Vec<serde_json::Value>> {
        if let Some(block) = self.get_block_by_number(block_number, true).await? {
            if let Some(transactions) = block.get("transactions").and_then(|t| t.as_array()) {
                return Ok(transactions.clone());
            }
        }
        Ok(vec![])
    }

    /// Get basic block information
    pub async fn get_block_info(&self, block_number: u64) -> Result<Option<serde_json::Value>> {
        self.get_block_by_number(block_number, false).await
    }

    /// Verify if a pool exists on-chain by checking if it has code at the address
    pub async fn verify_pool_exists_onchain(&self, pool_address: &str) -> Result<bool> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getCode",
            "params": [pool_address, "latest"],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            return Err(anyhow::anyhow!("eth_getCode error: {}", error["message"]));
        }

        let code = response_body
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("0x");

        // Pool exists if it has code beyond just "0x"
        Ok(code.len() > 2)
    }

    /// Get pool token addresses by calling the token0() and token1() functions
    pub async fn get_pool_tokens(&self, pool_address: &str) -> Result<Option<(String, String)>> {
        // First verify the pool has code
        if !self.verify_pool_exists_onchain(pool_address).await? {
            return Ok(None);
        }

        // token0() function selector: 0x0dfe1681
        let token0_call = self.call_contract_function(pool_address, "0x0dfe1681").await?;
        // token1() function selector: 0xd21220a7  
        let token1_call = self.call_contract_function(pool_address, "0xd21220a7").await?;

        if let (Some(token0_result), Some(token1_result)) = (token0_call, token1_call) {
            // Extract address from 32-byte result (last 20 bytes)
            if token0_result.len() >= 42 && token1_result.len() >= 42 {
                let token0_addr = format!("0x{}", &token0_result[token0_result.len()-40..]);
                let token1_addr = format!("0x{}", &token1_result[token1_result.len()-40..]);
                return Ok(Some((token0_addr, token1_addr)));
            }
        }

        Ok(None)
    }

    /// Call a contract function with given selector
    async fn call_contract_function(&self, contract_address: &str, data: &str) -> Result<Option<String>> {
        let request_body = json!({
            "jsonrpc": "2.0", 
            "method": "eth_call",
            "params": [{
                "to": contract_address,
                "data": data
            }, "latest"],
            "id": 1
        });

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request_body)
            .send()
            .await?;

        let response_body: serde_json::Value = response.json().await?;

        if let Some(error) = response_body.get("error") {
            // Contract call failed - pool likely doesn't exist or doesn't implement this function
            debug!("Contract call failed: {}", error["message"]);
            return Ok(None);
        }

        let result = response_body
            .get("result")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(result)
    }

    /// Get pool reserves by calling getReserves() function (Uniswap V2 style)
    pub async fn get_pool_reserves(&self, pool_address: &str) -> Result<Option<(String, String, u32)>> {
        // getReserves() function selector: 0x0902f1ac
        let reserves_call = self.call_contract_function(pool_address, "0x0902f1ac").await?;

        if let Some(result) = reserves_call {
            if result.len() >= 194 { // 0x + 3 * 64 chars = 194 chars minimum
                // Parse reserves from 3 32-byte values: reserve0, reserve1, blockTimestampLast
                let reserve0 = &result[2..66];   // First 32 bytes
                let reserve1 = &result[66..130]; // Second 32 bytes
                let timestamp = &result[130..194]; // Third 32 bytes
                
                // Convert timestamp hex to u32
                let timestamp_u32 = u32::from_str_radix(&timestamp[56..], 16).unwrap_or(0);
                
                return Ok(Some((
                    format!("0x{}", reserve0),
                    format!("0x{}", reserve1), 
                    timestamp_u32
                )));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pool_verification() {
        let eth_client = EthereumClient::new("http://192.168.0.14:8545").await.unwrap();
        
        // Test known USDC/WETH Uniswap V2 pool
        let usdc_weth_pool = "0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc";
        
        println!("Testing pool verification for: {}", usdc_weth_pool);
        
        // Test pool existence
        let exists = eth_client.verify_pool_exists_onchain(usdc_weth_pool).await.unwrap();
        println!("Pool exists: {}", exists);
        
        if exists {
            // Try to get tokens
            if let Some((token0, token1)) = eth_client.get_pool_tokens(usdc_weth_pool).await.unwrap() {
                println!("Token0: {}", token0);
                println!("Token1: {}", token1);
            }
        }
    }
}
