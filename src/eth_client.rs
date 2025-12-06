use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use crate::pool_db::PoolDatabase;

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
    pub is_dex: bool,
}

impl MempoolTransaction {
    pub fn from_value(value: &serde_json::Value, pool_db: &PoolDatabase, chain_id: u32) -> Result<Self> {
        let from = value["from"].as_str().unwrap_or("N/A").to_string();
        let to_opt = value["to"].as_str().map(|s| s.to_string());
        let value_hex = value["value"].as_str().unwrap_or("0x0").to_string();
        let gas_price_hex = value["gasPrice"].as_str().unwrap_or("0x0").to_string();
        
        // Parse nonce from hex to u64
        let nonce_hex = value["nonce"].as_str().unwrap_or("0x0");
        let nonce = parse_hex_u64(nonce_hex).unwrap_or(0);

        let value_eth = format_value(&value_hex);
        let gas_price_gwei = format_gas_price(&gas_price_hex);
        
        // Check if the "to" address is a known DEX pool from database
        let is_dex = to_opt.as_ref().map_or(false, |addr| {
            let result = pool_db.is_dex_pool(addr, chain_id).unwrap_or(false);
            if result {
                tracing::debug!("DEX pool detected - Address: {}, Value: {}, Gas: {}", addr, value_eth, gas_price_gwei);
            }
            result
        });

        Ok(MempoolTransaction {
            hash: value["hash"].as_str().unwrap_or("N/A").to_string(),
            from,
            to: to_opt,
            value: value_hex,
            gas: value["gas"].as_str().unwrap_or("0x0").to_string(),
            gas_price: gas_price_hex,
            nonce,
            data: value["input"].as_str().unwrap_or("0x").to_string(),
            block_hash: value["blockHash"].as_str().map(|s| s.to_string()),
            block_number: value["blockNumber"].as_str().map(|s| s.to_string()),
            transaction_index: value["transactionIndex"].as_str().map(|s| s.to_string()),
            value_eth,
            gas_price_gwei,
            is_dex,
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
    pub async fn get_pending_transactions(&self, pool_db: &PoolDatabase, chain_id: u32) -> Result<Vec<MempoolTransaction>> {
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
    async fn get_pending_from_txpool(&self, pool_db: &PoolDatabase, chain_id: u32) -> Result<Vec<MempoolTransaction>> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "txpool_content",
            "params": [],
            "id": 1
        });

        let response = self.http_client
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
    async fn get_pending_from_filter(&self, pool_db: &PoolDatabase, chain_id: u32) -> Result<Vec<MempoolTransaction>> {
        // Create filter
        let filter_request = json!({
            "jsonrpc": "2.0",
            "method": "eth_newPendingTransactionFilter",
            "params": [],
            "id": 1
        });

        let filter_response = self.http_client
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

        let changes_response = self.http_client
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
    async fn get_transaction_by_hash(&self, hash: &str, pool_db: &PoolDatabase, chain_id: u32) -> Result<MempoolTransaction> {
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionByHash",
            "params": [hash],
            "id": 1
        });

        let response = self.http_client
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
}
