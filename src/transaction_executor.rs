/// Direct Mempool Transaction Execution
/// 
/// This module provides direct-to-mempool transaction submission similar to arboo's approach.
/// Unlike Flashbots bundles, these transactions are immediately visible in the public mempool.

use alloy::{
    primitives::{Address, U256, Bytes, TxHash, Log},
    providers::{Provider, ProviderBuilder},
    rpc::types::{TransactionRequest, TransactionInput, TransactionReceipt},
    signers::local::PrivateKeySigner,
    network::{EthereumWallet, TransactionBuilder},
    sol_types::{SolEvent, sol},
};
use alloy_primitives::TxKind;
use anyhow::Result;
use reqwest::Url;
use std::{str::FromStr, collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::{info, debug, error, warn, trace, instrument};
use serde_json::json;

// Define common DEX events for profit calculation
sol! {
    #[derive(Debug, PartialEq, Eq)]
    event Swap(
        address indexed sender,
        uint256 amount0In,
        uint256 amount1In,
        uint256 amount0Out,
        uint256 amount1Out,
        address indexed to
    );
    
    #[derive(Debug, PartialEq, Eq)]
    event Transfer(
        address indexed from,
        address indexed to,
        uint256 value
    );
}

#[derive(Debug, Clone)]
pub struct TransactionExecutionResult {
    pub tx_hash: String,
    pub gas_used: Option<u128>,
    pub gas_price: Option<u128>,
    pub actual_profit: Option<U256>,
    pub success: bool,
    pub confirmation_time: Option<std::time::Duration>,
    pub receipt: Option<TransactionReceipt>,
    pub profit_calculation: Option<ProfitCalculation>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProfitCalculation {
    pub eth_in: U256,
    pub eth_out: U256,
    pub token_in: U256,
    pub token_out: U256,
    pub gas_cost_wei: U256,
    pub net_profit_wei: U256,
    pub profit_percentage: f64,
    pub swap_events_parsed: usize,
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub gas_price_increase_percent: u8,
    pub retry_on_timeout: bool,
    pub retry_on_underpriced: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 1000,
            max_delay_ms: 10000,
            gas_price_increase_percent: 10,
            retry_on_timeout: true,
            retry_on_underpriced: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub tx_hash: TxHash,
    pub nonce: u64,
    pub gas_price: u128,
    pub submitted_at: std::time::Instant,
    pub max_wait_time: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct NonceManager {
    current_nonce: Arc<RwLock<u64>>,
    pending_transactions: Arc<RwLock<HashMap<u64, PendingTransaction>>>,
}

impl NonceManager {
    pub fn new(initial_nonce: u64) -> Self {
        Self {
            current_nonce: Arc::new(RwLock::new(initial_nonce)),
            pending_transactions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_next_nonce(&self) -> u64 {
        let mut nonce = self.current_nonce.write().await;
        let next = *nonce;
        *nonce += 1;
        next
    }

    pub async fn add_pending_transaction(&self, tx: PendingTransaction) {
        let mut pending = self.pending_transactions.write().await;
        pending.insert(tx.nonce, tx);
    }

    pub async fn mark_transaction_confirmed(&self, nonce: u64) {
        let mut pending = self.pending_transactions.write().await;
        pending.remove(&nonce);
    }

    pub async fn get_pending_count(&self) -> usize {
        let pending = self.pending_transactions.read().await;
        pending.len()
    }

    pub async fn cleanup_stale_transactions(&self, provider_url: &str) -> Result<()> {
        let mut pending = self.pending_transactions.write().await;
        let mut to_remove = Vec::new();

        // Create provider for checking transaction status
        let http_url = Url::from_str(provider_url)
            .map_err(|e| anyhow::anyhow!("Invalid RPC URL format '{}': {}", provider_url, e))?;
        let provider = ProviderBuilder::new().on_http(http_url);

        for (nonce, tx) in pending.iter() {
            if tx.submitted_at.elapsed() > tx.max_wait_time {
                // Check if transaction was confirmed using tx hash as string
                let tx_hash_str = format!("{:?}", tx.tx_hash);
                match provider.get_transaction_by_hash(tx.tx_hash).await {
                    Ok(Some(confirmed_tx)) => {
                        if confirmed_tx.block_number.is_some() {
                            info!("✅ Transaction {} confirmed after timeout", tx_hash_str);
                            to_remove.push(*nonce);
                        } else {
                            warn!("⏰ Transaction {} still pending after timeout", tx_hash_str);
                        }
                    }
                    Ok(None) => {
                        warn!("❌ Transaction {} not found, likely dropped", tx_hash_str);
                        to_remove.push(*nonce);
                    }
                    Err(e) => {
                        error!("🔍 Error checking transaction {}: {}", tx_hash_str, e);
                    }
                }
            }
        }

        for nonce in to_remove {
            pending.remove(&nonce);
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SandwichExecutionTracker {
    pub frontrun_tx: Option<TransactionExecutionResult>,
    pub victim_tx_hash: Option<String>,
    pub backrun_tx: Option<TransactionExecutionResult>,
    pub start_time: std::time::Instant,
    pub total_profit: Option<ProfitCalculation>,
    pub strategy: String,
}

#[derive(Debug, Clone)]
pub struct DirectMempoolExecutor {
    rpc_url: String,
    private_key: Option<String>,
    chain_id: u64,
    nonce_manager: Option<NonceManager>,
    max_pending_transactions: usize,
    retry_config: RetryConfig,
}

impl DirectMempoolExecutor {
    pub fn new(rpc_url: String, private_key: Option<String>, chain_id: u64) -> Self {
        Self {
            rpc_url,
            private_key,
            chain_id,
            nonce_manager: None,
            max_pending_transactions: 10,
            retry_config: RetryConfig::default(),
        }
    }

    pub fn with_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    pub async fn initialize_nonce_manager(&mut self) -> Result<()> {
        if let Some(private_key) = &self.private_key {
            let signer = PrivateKeySigner::from_str(private_key)
                .map_err(|e| anyhow::anyhow!("Invalid private key format: {}", e))?;
            
            let http_url = Url::from_str(&self.rpc_url)
                .map_err(|e| anyhow::anyhow!("Invalid RPC URL format '{}': {}", self.rpc_url, e))?;
            
            let provider = ProviderBuilder::new().on_http(http_url);
            let from_address = signer.address();
            
            let current_nonce = provider
                .get_transaction_count(from_address)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get initial nonce: {}", e))?;

            self.nonce_manager = Some(NonceManager::new(current_nonce));
            info!("🔢 Initialized nonce manager with nonce: {}", current_nonce);
        }
        Ok(())
    }

    /// Send transaction with retry logic
    pub async fn send_transaction_with_retry(
        &self,
        contract_address: Address,
        gas_price: Option<u128>,
        gas_limit: Option<u64>,
        base_fee: Option<u128>,
        priority_fee: Option<u128>,
        input_data: Vec<u8>,
        value: Option<U256>,
    ) -> Result<TransactionExecutionResult> {
        let mut current_gas_price = gas_price.unwrap_or(20_000_000_000); // 20 gwei default
        let mut attempts = 0;

        loop {
            attempts += 1;
            
            info!("🔄 Transaction attempt {} of {} (gas price: {} gwei)", 
                 attempts, self.retry_config.max_attempts, current_gas_price / 1_000_000_000);

            match self.send_transaction(
                contract_address,
                Some(current_gas_price),
                gas_limit,
                base_fee,
                priority_fee,
                input_data.clone(),
                value,
            ).await {
                Ok(result) if result.success => {
                    info!("✅ Transaction successful on attempt {}: {}", attempts, result.tx_hash);
                    return Ok(result);
                }
                Ok(result) => {
                    warn!("⚠️ Transaction failed on attempt {}: {} - {}", 
                         attempts, result.tx_hash, 
                         result.error.as_ref().unwrap_or(&"Unknown error".to_string()));
                    
                    if attempts >= self.retry_config.max_attempts {
                        return Ok(result); // Return the failed result
                    }

                    // Check if this is a retryable error
                    if let Some(ref error) = result.error {
                        if self.should_retry_error(error) {
                            current_gas_price = self.calculate_retry_gas_price(current_gas_price);
                            let delay = self.calculate_retry_delay(attempts);
                            warn!("🔄 Retrying with higher gas price {} gwei in {}ms", 
                                 current_gas_price / 1_000_000_000, delay);
                            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                            continue;
                        } else {
                            error!("❌ Non-retryable error: {}", error);
                            return Ok(result);
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Transaction submission error on attempt {}: {}", attempts, e);
                    
                    if attempts >= self.retry_config.max_attempts {
                        return Err(e);
                    }

                    // For submission errors, also retry with higher gas
                    current_gas_price = self.calculate_retry_gas_price(current_gas_price);
                    let delay = self.calculate_retry_delay(attempts);
                    warn!("🔄 Retrying transaction submission with gas price {} gwei in {}ms", 
                         current_gas_price / 1_000_000_000, delay);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }

    /// Determine if an error is retryable
    fn should_retry_error(&self, error: &str) -> bool {
        let error_lower = error.to_lowercase();
        
        // Common retryable errors
        if error_lower.contains("timeout") && self.retry_config.retry_on_timeout {
            return true;
        }
        if error_lower.contains("underpriced") && self.retry_config.retry_on_underpriced {
            return true;
        }
        if error_lower.contains("nonce too low") {
            return true; // Will require nonce adjustment
        }
        if error_lower.contains("replacement transaction underpriced") {
            return true;
        }
        if error_lower.contains("network is busy") || error_lower.contains("congested") {
            return true;
        }

        // Non-retryable errors
        if error_lower.contains("insufficient funds") {
            return false;
        }
        if error_lower.contains("execution reverted") {
            return false;
        }
        if error_lower.contains("invalid signature") {
            return false;
        }

        // Default to retryable for unknown errors
        true
    }

    /// Calculate new gas price for retry
    fn calculate_retry_gas_price(&self, current_gas_price: u128) -> u128 {
        current_gas_price + (current_gas_price * self.retry_config.gas_price_increase_percent as u128 / 100)
    }

    /// Calculate delay for retry with exponential backoff
    fn calculate_retry_delay(&self, attempt: usize) -> u64 {
        let delay = self.retry_config.base_delay_ms * (2_u64.pow((attempt - 1) as u32));
        delay.min(self.retry_config.max_delay_ms)
    }

    /// Log comprehensive transaction metrics for monitoring
    fn log_transaction_metrics(&self, result: &TransactionExecutionResult, operation: &str) {
        let metrics = json!({
            "operation": operation,
            "tx_hash": result.tx_hash,
            "success": result.success,
            "gas_used": result.gas_used,
            "gas_price_gwei": result.gas_price.map(|p| p / 1_000_000_000),
            "confirmation_time_ms": result.confirmation_time.map(|t| t.as_millis()),
            "profit_eth": result.profit_calculation.as_ref().map(|p| 
                p.net_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18),
            "profit_percentage": result.profit_calculation.as_ref().map(|p| p.profit_percentage),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        
        info!("📊 Transaction Metrics: {}", metrics);
        
        // Also log detailed breakdown for successful transactions
        if result.success && result.profit_calculation.is_some() {
            let profit = result.profit_calculation.as_ref().unwrap();
            trace!("💰 Profit breakdown - ETH in: {}, ETH out: {}, Gas cost: {} ETH, Net: {} ETH",
                  profit.eth_in.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
                  profit.eth_out.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
                  profit.gas_cost_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
                  profit.net_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18);
        }
    }

    /// Log nonce manager status for debugging
    pub async fn log_nonce_status(&self) {
        if let Some(ref nonce_manager) = self.nonce_manager {
            let current_nonce = *nonce_manager.current_nonce.read().await;
            let pending_count = nonce_manager.get_pending_count().await;
            
            debug!("🔢 Nonce Status - Current: {}, Pending: {}, Max pending: {}", 
                  current_nonce, pending_count, self.max_pending_transactions);
            
            if pending_count > 5 {
                warn!("⚠️ High number of pending transactions: {}", pending_count);
            }
        }
    }
    pub async fn send_transaction(
        &self,
        contract_address: Address,
        gas_price: Option<u128>,
        gas_limit: Option<u64>,
        base_fee: Option<u128>,
        priority_fee: Option<u128>,
        input_data: Vec<u8>,
        value: Option<U256>,
    ) -> Result<TransactionExecutionResult> {
        let private_key = self.private_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Private key required for direct mempool execution"))?;

        let signer = PrivateKeySigner::from_str(private_key)
            .map_err(|e| anyhow::anyhow!("Invalid private key format: {}", e))?;

        let http_url = Url::from_str(&self.rpc_url)
            .map_err(|e| anyhow::anyhow!("Invalid RPC URL format '{}': {}", self.rpc_url, e))?;

        // Create provider without wallet first
        let provider = ProviderBuilder::new().on_http(http_url.clone());
        let from_address = signer.address();

        // Get nonce from manager or fetch fresh
        let nonce = if let Some(ref nonce_manager) = self.nonce_manager {
            // Check pending transaction limit
            let pending_count = nonce_manager.get_pending_count().await;
            if pending_count >= self.max_pending_transactions {
                return Err(anyhow::anyhow!(
                    "Too many pending transactions: {} >= {}", 
                    pending_count, 
                    self.max_pending_transactions
                ));
            }

            // Cleanup stale transactions
            nonce_manager.cleanup_stale_transactions(&self.rpc_url).await?;

            nonce_manager.get_next_nonce().await
        } else {
            provider
                .get_transaction_count(from_address)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get transaction nonce: {}", e))?
        };

        let start_time = std::time::Instant::now();

        debug!(
            "Preparing direct mempool transaction:\n\
             contract_address: {}\n\
             from_address: {}\n\
             nonce: {}\n\
             gas_price: {:?}\n\
             gas_limit: {:?}\n\
             base_fee: {:?}\n\
             priority_fee: {:?}",
            contract_address,
            from_address,
            nonce,
            gas_price,
            gas_limit,
            base_fee,
            priority_fee
        );

        let input_bytes = Bytes::from(input_data);
        let gas_limit = gas_limit.unwrap_or(500_000);
        let max_priority_fee = priority_fee.unwrap_or(2_000_000_000);
        let max_fee_per_gas = base_fee.unwrap_or(20_000_000_000);

        // Build transaction request
        let tx_req = TransactionRequest {
            from: Some(from_address),
            to: Some(TxKind::Call(contract_address)),
            value: Some(value.unwrap_or(U256::ZERO)),
            input: TransactionInput::new(input_bytes),
            nonce: Some(nonce),
            gas: Some(gas_limit),
            max_priority_fee_per_gas: Some(max_priority_fee),
            max_fee_per_gas: Some(max_fee_per_gas),
            chain_id: Some(self.chain_id),
            ..Default::default()
        };

        debug!("Transaction request: {:?}", tx_req);

        // Create provider with wallet for signing
        let provider_with_wallet = ProviderBuilder::new()
            .wallet(EthereumWallet::from(signer))
            .on_http(http_url);

        // Send transaction to mempool
        match provider_with_wallet.send_transaction(tx_req).await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash();
                let tx_hash_str = tx_hash.to_string();
                info!("✅ Transaction sent to mempool: {} (nonce: {})", tx_hash_str, nonce);

                // Track pending transaction if nonce manager is available
                if let Some(ref nonce_manager) = self.nonce_manager {
                    let pending_tx_info = PendingTransaction {
                        tx_hash: *tx_hash,
                        nonce,
                        gas_price: max_fee_per_gas,
                        submitted_at: start_time,
                        max_wait_time: std::time::Duration::from_secs(60), // 1 minute timeout
                    };
                    nonce_manager.add_pending_transaction(pending_tx_info).await;
                }

                // Wait for confirmation with timeout
                match tokio::time::timeout(
                    std::time::Duration::from_secs(45),
                    pending_tx.get_receipt()
                ).await {
                    Ok(Ok(receipt)) => {
                        let confirmation_time = start_time.elapsed();
                        let gas_used = receipt.gas_used;
                        let effective_gas_price = receipt.effective_gas_price;
                        
                        info!("✅ Transaction confirmed: {} in {:?} (gas: {}, price: {})", 
                             tx_hash_str, confirmation_time, gas_used, effective_gas_price);

                        // Mark transaction as confirmed
                        if let Some(ref nonce_manager) = self.nonce_manager {
                            nonce_manager.mark_transaction_confirmed(nonce).await;
                        }

                        Ok(TransactionExecutionResult {
                            tx_hash: tx_hash_str,
                            gas_used: Some(gas_used),
                            gas_price: Some(effective_gas_price),
                            actual_profit: None, // Will be calculated separately
                            success: true,
                            confirmation_time: Some(confirmation_time),
                            receipt: Some(receipt.clone()),
                            profit_calculation: self.calculate_profit_from_receipt(&receipt).await.ok(),
                            error: None,
                        })
                    }
                    Ok(Err(e)) => {
                        error!("❌ Transaction failed: {:?}", e);
                        if let Some(ref nonce_manager) = self.nonce_manager {
                            nonce_manager.mark_transaction_confirmed(nonce).await;
                        }
                        Ok(TransactionExecutionResult {
                            tx_hash: tx_hash_str,
                            gas_used: None,
                            gas_price: Some(max_fee_per_gas),
                            actual_profit: None,
                            success: false,
                            confirmation_time: Some(start_time.elapsed()),
                            receipt: None,
                            profit_calculation: None,
                            error: Some(format!("Transaction execution failed: {}", e)),
                        })
                    }
                    Err(_) => {
                        warn!("⏰ Transaction confirmation timeout: {} (will continue monitoring)", tx_hash_str);
                        // Don't mark as confirmed - let cleanup handle it
                        Ok(TransactionExecutionResult {
                            tx_hash: tx_hash_str,
                            gas_used: None,
                            gas_price: Some(max_fee_per_gas),
                            actual_profit: None,
                            success: false,
                            confirmation_time: None,
                            receipt: None,
                            profit_calculation: None,
                            error: Some("Transaction confirmation timeout".to_string()),
                        })
                    }
                }
            }
            Err(e) => {
                error!("❌ Failed to send transaction: {:?}", e);
                Err(anyhow::anyhow!("Failed to send transaction to mempool: {}", e))
            }
        }
    }

    /// Create sandwich frontrun transaction
    pub async fn create_frontrun_transaction(
        &self,
        pool_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        min_amount_out: U256,
        gas_price: u128,
        gas_limit: u64,
    ) -> Result<Vec<u8>> {
        // For now, create a simple swap transaction calldata
        // This would need to be adapted for your specific MEV contract
        
        use alloy_sol_types::{SolCall, sol};
        
        sol! {
            #[derive(Debug)]
            function swapExactTokensForTokens(
                uint256 amountIn,
                uint256 amountOutMin,
                address[] calldata path,
                address to,
                uint256 deadline
            ) external returns (uint256[] memory amounts);
        }

        let deadline = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + 300
        ); // 5 minutes from now

        let path = vec![token_in, token_out];
        let to = Address::ZERO; // Would be your contract or EOA

        let function_call = swapExactTokensForTokensCall {
            amountIn: amount_in,
            amountOutMin: min_amount_out,
            path,
            to,
            deadline,
        }.abi_encode();

        Ok(function_call)
    }

    /// Execute sandwich attack using direct mempool submission
    pub async fn execute_sandwich_direct(
        &self,
        frontrun_tx: TransactionRequest,
        backrun_tx: TransactionRequest,
        gas_price: u128,
    ) -> Result<Vec<TransactionExecutionResult>> {
        info!("🥪 Executing sandwich attack via direct mempool");
        
        let mut results = Vec::new();
        
        // Submit frontrun transaction first
        info!("📤 Submitting frontrun transaction...");
        match self.send_raw_transaction(frontrun_tx, gas_price + 1_000_000_000).await {
            Ok(result) => {
                info!("✅ Frontrun submitted: {}", result.tx_hash);
                results.push(result);
            }
            Err(e) => {
                error!("❌ Frontrun failed: {}", e);
                return Err(e);
            }
        }

        // Wait a brief moment, then submit backrun
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        info!("📤 Submitting backrun transaction...");
        match self.send_raw_transaction(backrun_tx, gas_price).await {
            Ok(result) => {
                info!("✅ Backrun submitted: {}", result.tx_hash);
                results.push(result);
            }
            Err(e) => {
                error!("❌ Backrun failed: {}", e);
                // Don't return error here - frontrun might still be profitable
            }
        }

        Ok(results)
    }

    /// Send raw transaction with specific gas price
    async fn send_raw_transaction(
        &self,
        mut tx_req: TransactionRequest,
        gas_price: u128,
    ) -> Result<TransactionExecutionResult> {
        // Update gas price for competitive submission
        tx_req.max_fee_per_gas = Some(gas_price);
        tx_req.max_priority_fee_per_gas = Some(gas_price / 10); // 10% priority fee

        self.send_transaction(
            tx_req.to.unwrap().to().copied().unwrap_or(Address::ZERO),
            Some(gas_price),
            tx_req.gas,
            Some(gas_price),
            tx_req.max_priority_fee_per_gas,
            tx_req.input.into_input().unwrap_or_default().to_vec(),
            tx_req.value,
        ).await
    }

    /// Calculate profit from transaction receipt by analyzing swap events
    async fn calculate_profit_from_receipt(&self, receipt: &TransactionReceipt) -> Result<ProfitCalculation> {
        let mut eth_in = U256::ZERO;
        let mut eth_out = U256::ZERO;
        let mut token_in = U256::ZERO;
        let mut token_out = U256::ZERO;
        let mut swap_events_parsed = 0;

        // Parse swap events from transaction logs
        for log in receipt.inner.logs() {
            if let Ok(swap_event) = Swap::decode_log(log, true) {
                swap_events_parsed += 1;
                
                // Accumulate token flows
                token_in += swap_event.amount0In + swap_event.amount1In;
                token_out += swap_event.amount0Out + swap_event.amount1Out;
                
                debug!("🔍 Parsed swap event - In: {}, Out: {}", 
                      token_in, token_out);
            }
        }

        // Calculate ETH flows (simplified - in production would track specific tokens)
        if swap_events_parsed > 0 {
            // Estimate ETH equivalent values
            eth_in = token_in / U256::from(1000); // Mock conversion rate
            eth_out = token_out / U256::from(1000);
        }

        let gas_cost_wei = U256::from(receipt.gas_used) * 
                          U256::from(receipt.effective_gas_price);

        let gross_profit_wei = if eth_out > eth_in {
            eth_out - eth_in
        } else {
            U256::ZERO
        };

        let net_profit_wei = if gross_profit_wei > gas_cost_wei {
            gross_profit_wei - gas_cost_wei
        } else {
            U256::ZERO
        };

        let profit_percentage = if eth_in > U256::ZERO {
            (net_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 
             eth_in.to_string().parse::<f64>().unwrap_or(1.0)) * 100.0
        } else {
            0.0
        };

        info!("💰 Profit calculation - Gross: {} ETH, Gas: {} ETH, Net: {} ETH ({}%)",
             gross_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
             gas_cost_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
             net_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
             profit_percentage);

        Ok(ProfitCalculation {
            eth_in,
            eth_out,
            token_in,
            token_out,
            gas_cost_wei,
            net_profit_wei,
            profit_percentage,
            swap_events_parsed,
        })
    }

    /// Execute complete sandwich attack with profit tracking
    pub async fn execute_tracked_sandwich(
        &self,
        frontrun_tx: TransactionRequest,
        victim_tx_hash: String,
        backrun_tx: TransactionRequest,
        gas_price: u128,
        strategy: String,
    ) -> Result<SandwichExecutionTracker> {
        let mut tracker = SandwichExecutionTracker {
            frontrun_tx: None,
            victim_tx_hash: Some(victim_tx_hash.clone()),
            backrun_tx: None,
            start_time: std::time::Instant::now(),
            total_profit: None,
            strategy,
        };

        info!("🥪 Starting tracked sandwich execution - strategy: {}", tracker.strategy);

        // Execute frontrun
        match self.send_raw_transaction(frontrun_tx, gas_price + 1_000_000_000).await {
            Ok(result) => {
                info!("✅ Frontrun completed: {} (success: {})", result.tx_hash, result.success);
                tracker.frontrun_tx = Some(result);
            }
            Err(e) => {
                error!("❌ Frontrun failed: {}", e);
                return Err(e);
            }
        }

        // Wait briefly, then execute backrun
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        match self.send_raw_transaction(backrun_tx, gas_price).await {
            Ok(result) => {
                info!("✅ Backrun completed: {} (success: {})", result.tx_hash, result.success);
                tracker.backrun_tx = Some(result);
            }
            Err(e) => {
                warn!("⚠️ Backrun failed: {} - frontrun might still be profitable", e);
            }
        }

        // Calculate total profit
        tracker.total_profit = self.calculate_sandwich_profit(&tracker).await.ok();

        let execution_time = tracker.start_time.elapsed();
        info!("🏁 Sandwich execution completed in {:?}", execution_time);

        if let Some(ref profit) = tracker.total_profit {
            info!("📊 Final profit: {} ETH ({}% return)", 
                 profit.net_profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18,
                 profit.profit_percentage);
        }

        Ok(tracker)
    }

    /// Calculate total profit from complete sandwich execution
    async fn calculate_sandwich_profit(&self, tracker: &SandwichExecutionTracker) -> Result<ProfitCalculation> {
        let mut total_gas_cost = U256::ZERO;
        let mut total_eth_in = U256::ZERO;
        let mut total_eth_out = U256::ZERO;
        let mut total_swap_events = 0;

        // Aggregate frontrun profit calculation
        if let Some(ref frontrun) = tracker.frontrun_tx {
            if let Some(ref profit) = frontrun.profit_calculation {
                total_gas_cost += profit.gas_cost_wei;
                total_eth_in += profit.eth_in;
                total_swap_events += profit.swap_events_parsed;
            }
        }

        // Aggregate backrun profit calculation
        if let Some(ref backrun) = tracker.backrun_tx {
            if let Some(ref profit) = backrun.profit_calculation {
                total_gas_cost += profit.gas_cost_wei;
                total_eth_out += profit.eth_out;
                total_swap_events += profit.swap_events_parsed;
            }
        }

        let gross_profit = if total_eth_out > total_eth_in {
            total_eth_out - total_eth_in
        } else {
            U256::ZERO
        };

        let net_profit = if gross_profit > total_gas_cost {
            gross_profit - total_gas_cost
        } else {
            U256::ZERO
        };

        let profit_percentage = if total_eth_in > U256::ZERO {
            (net_profit.to_string().parse::<f64>().unwrap_or(0.0) / 
             total_eth_in.to_string().parse::<f64>().unwrap_or(1.0)) * 100.0
        } else {
            0.0
        };

        Ok(ProfitCalculation {
            eth_in: total_eth_in,
            eth_out: total_eth_out,
            token_in: U256::ZERO, // Not tracked at sandwich level
            token_out: U256::ZERO,
            gas_cost_wei: total_gas_cost,
            net_profit_wei: net_profit,
            profit_percentage,
            swap_events_parsed: total_swap_events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let executor = DirectMempoolExecutor::new(
            "http://localhost:8545".to_string(),
            Some("0x1234".to_string()),
            1,
        );
        assert_eq!(executor.chain_id, 1);
    }
}