/// MEV Bundle Builder and Direct Transaction Execution
/// 
/// This module supports both Flashbots bundle submission and direct mempool execution.
/// Choose between private bundle submission (Flashbots) or immediate public execution (like arboo).

use crate::{
    mempool_monitor::{MempoolOpportunity, MempoolTransaction},
    flash_loan_manager::{SandwichExecutionData},
    eth_client::EthereumClient,
};
use crate::transaction_executor::{DirectMempoolExecutor, TransactionExecutionResult};
use alloy_primitives::{Address, U256, Bytes};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, timeout};
use tracing::{info, warn, error, debug};
use reqwest::Client;
use rand;

/// Flashbots relay endpoint
pub const FLASHBOTS_RELAY_URL: &str = "https://relay.flashbots.net";

/// Transaction execution method
#[derive(Debug, Clone)]
pub enum ExecutionMethod {
    /// Submit transactions via Flashbots bundle (private, atomic)
    Flashbots {
        api_key: String,
    },
    /// Submit transactions directly to public mempool (like arboo)
    DirectMempool {
        private_key: String,
        aggressive_gas: bool, // Use higher gas prices to front-run
    },
    /// Simulation only - no actual transactions
    SimulationOnly,
}

/// MEV bundle for Flashbots submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevBundle {
    pub transactions: Vec<SignedTransaction>,
    pub block_number: u64,
    pub min_timestamp: Option<u64>,
    pub max_timestamp: Option<u64>,
    pub reverting_tx_hashes: Vec<String>,
}

/// Signed transaction for bundle inclusion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    pub raw_tx: String,          // RLP-encoded signed transaction
    pub hash: String,            // Transaction hash
    pub gas_price: U256,         // Gas price in wei
    pub gas_limit: U256,         // Gas limit
    pub value: U256,             // ETH value transferred
    pub to: Option<Address>,     // Recipient address
    pub from: Address,           // Sender address
    pub nonce: u64,              // Transaction nonce
}

/// Bundle simulation result from Flashbots
#[derive(Debug, Clone, Deserialize)]
pub struct BundleSimulation {
    pub coinbase_diff: String,    // Profit to coinbase (wei)
    pub gas_fees: String,         // Total gas fees (wei)
    pub gas_used: u64,            // Total gas used
    pub results: Vec<TxSimulation>,
}

/// Individual transaction simulation result
#[derive(Debug, Clone, Deserialize)]
pub struct TxSimulation {
    pub coinbase_diff: String,
    pub eth_sent_to_coinbase: String,
    pub from_address: String,
    pub gas_fees: String,
    pub gas_price: String,
    pub gas_used: u64,
    pub to_address: String,
    pub tx_hash: String,
    pub value: String,
}

/// Bundle submission result
#[derive(Debug, Clone)]
pub struct BundleSubmissionResult {
    pub bundle_hash: Option<String>,
    pub simulation: Option<BundleSimulation>,
    pub submitted: bool,
    pub profit_eth: f64,
    pub total_gas_used: u64,
    pub coinbase_payment: U256,
    pub error: Option<String>,
}

/// MEV bundle builder with multiple execution methods
#[derive(Clone)]
pub struct MevBundleBuilder {
    client: Client,
    eth_client: EthereumClient,
    execution_method: ExecutionMethod,
    direct_executor: Option<DirectMempoolExecutor>, // Proper executor instance
    max_gas_price: U256,             // Maximum gas price to use
    min_profit_threshold: U256,      // Minimum profit threshold (wei)
    coinbase_payment_percent: u8,    // Percentage of profit to pay to miner (0-100)
}

impl MevBundleBuilder {
    /// Create new MEV bundle builder with execution method
    pub fn new(
        eth_client: EthereumClient,
        execution_method: ExecutionMethod,
        rpc_url: String,
    ) -> Self {
        let direct_executor = match &execution_method {
            ExecutionMethod::DirectMempool { private_key, .. } => {
                Some(DirectMempoolExecutor::new(
                    rpc_url.clone(),
                    Some(private_key.clone()),
                    1, // Ethereum mainnet chain ID
                ))
            },
            _ => None,
        };

        Self {
            client: Client::new(),
            eth_client,
            execution_method,
            direct_executor,
            max_gas_price: U256::from(50_000_000_000u64), // 50 gwei default
            min_profit_threshold: U256::from(10_000_000_000_000_000u64), // 0.01 ETH default
            coinbase_payment_percent: 90, // 90% to miner, 10% keep
        }
    }

    /// Create Flashbots-enabled builder
    pub fn with_flashbots(eth_client: EthereumClient, api_key: String, rpc_url: String) -> Self {
        Self::new(eth_client, ExecutionMethod::Flashbots { api_key }, rpc_url)
    }

    /// Create direct mempool builder (like arboo)
    pub fn with_direct_mempool(
        eth_client: EthereumClient,
        private_key: String,
        rpc_url: String,
        aggressive_gas: bool,
    ) -> Self {
        Self::new(
            eth_client,
            ExecutionMethod::DirectMempool { private_key, aggressive_gas },
            rpc_url,
        )
    }

    /// Create simulation-only builder
    pub fn simulation_only(eth_client: EthereumClient, rpc_url: String) -> Self {
        Self::new(eth_client, ExecutionMethod::SimulationOnly, rpc_url)
    }

    /// Set maximum gas price for transactions
    pub fn set_max_gas_price(&mut self, gas_price: U256) {
        self.max_gas_price = gas_price;
    }

    /// Set minimum profit threshold
    pub fn set_min_profit_threshold(&mut self, threshold: U256) {
        self.min_profit_threshold = threshold;
    }

    /// Set coinbase payment percentage (0-100)
    pub fn set_coinbase_payment_percent(&mut self, percent: u8) {
        if percent <= 100 {
            self.coinbase_payment_percent = percent;
        }
    }

    /// Execute sandwich attack using the configured method
    pub async fn execute_sandwich_attack(
        &self,
        opportunity: &MempoolOpportunity,
        target_block: u64,
    ) -> Result<BundleSubmissionResult> {
        match &self.execution_method {
            ExecutionMethod::Flashbots { api_key } => {
                info!("🔒 Executing sandwich via Flashbots bundle");
                self.execute_via_flashbots(opportunity, target_block, api_key).await
            },
            ExecutionMethod::DirectMempool { private_key, aggressive_gas } => {
                info!("⚡ Executing sandwich via direct mempool (like arboo)");
                self.execute_via_direct_mempool(opportunity, *aggressive_gas).await
            },
            ExecutionMethod::SimulationOnly => {
                info!("🧪 Simulation only - no transactions sent");
                self.simulate_sandwich_only(opportunity).await
            },
        }
    }

    /// Execute sandwich via Flashbots bundle
    async fn execute_via_flashbots(
        &self,
        opportunity: &MempoolOpportunity,
        target_block: u64,
        api_key: &str,
    ) -> Result<BundleSubmissionResult> {
        let bundle = self.build_sandwich_bundle(opportunity, target_block).await?;
        self.submit_bundle_to_flashbots(bundle, api_key).await
    }

    async fn execute_via_direct_mempool(
        &self,
        opportunity: &MempoolOpportunity,
        aggressive_gas: bool,
    ) -> Result<BundleSubmissionResult> {
        info!("⚡ Direct mempool execution - implementing sandwich attack");
        
        let executor = self.direct_executor.as_ref()
            .ok_or_else(|| anyhow!("Direct mempool executor not initialized"))?;

        // Use improved gas price calculation for competitive bidding
        let victim_gas_price = opportunity.victim_tx.gas_price;
        let frontrun_gas_price = self.eth_client.calculate_frontrun_gas_price(
            victim_gas_price,
            aggressive_gas,
        ).await?;

        info!(
            "💰 Gas Price Strategy: Victim: {:.2} gwei, Frontrun: {:.2} gwei ({})",
            victim_gas_price.to_string().parse::<u64>().unwrap_or(0) as f64 / 1_000_000_000.0,
            frontrun_gas_price.to_string().parse::<u64>().unwrap_or(0) as f64 / 1_000_000_000.0,
            if aggressive_gas { "Aggressive" } else { "Conservative" }
        );

        // Prepare frontrun transaction with competitive gas pricing
        let frontrun_calldata = executor.create_frontrun_transaction(
            opportunity.sandwich_target.pool.address,
            opportunity.sandwich_target.pool.token0,
            opportunity.sandwich_target.pool.token1,
            opportunity.sandwich_target.recommended_frontrun_amount,
            opportunity.sandwich_target.recommended_frontrun_amount / U256::from(10), // 10% slippage
            frontrun_gas_price.to_string().parse().unwrap_or(20_000_000_000),
            300_000,
        ).await?;

        // Execute frontrun transaction
        info!("📤 Submitting frontrun transaction to mempool...");
        let frontrun_result = executor.send_transaction(
            opportunity.sandwich_target.pool.address,
            Some(frontrun_gas_price.to_string().parse().unwrap_or(20_000_000_000)),
            Some(300_000),
            Some(frontrun_gas_price.to_string().parse().unwrap_or(20_000_000_000)),
            Some(frontrun_gas_price.to_string().parse().unwrap_or(2_000_000_000) / 10),
            frontrun_calldata,
            Some(U256::ZERO),
        ).await?;

        info!("✅ Frontrun transaction sent: {}", frontrun_result.tx_hash);

        // Wait for victim transaction to be included or timeout after 30 seconds
        info!("⏳ Waiting for victim transaction to be mined...");
        let mut attempts = 0;
        let max_attempts = 30; // 30 seconds timeout

        while attempts < max_attempts {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            attempts += 1;

            // Check if victim transaction was mined
            if let Ok(receipt) = self.eth_client.get_transaction_receipt(&opportunity.victim_tx.hash).await {
                if receipt.is_some() {
                    info!("✅ Victim transaction mined, submitting backrun...");
                    break;
                }
            }
        }

        if attempts >= max_attempts {
            return Ok(BundleSubmissionResult {
                bundle_hash: Some(frontrun_result.tx_hash.clone()),
                simulation: None,
                submitted: true,
                profit_eth: 0.0, // No profit if victim tx not mined
                total_gas_used: frontrun_result.gas_used.unwrap_or(300_000) as u64,
                coinbase_payment: U256::ZERO,
                error: Some("Victim transaction timeout - backrun not submitted".to_string()),
            });
        }

        // Execute backrun transaction with standard gas price (don't need to front-run anyone for backrun)
        let backrun_gas_price = self.eth_client.get_gas_price().await?;
        
        let backrun_calldata = executor.create_frontrun_transaction(
            opportunity.sandwich_target.pool.address,
            opportunity.sandwich_target.pool.token1, // Reverse direction
            opportunity.sandwich_target.pool.token0,
            opportunity.sandwich_target.recommended_frontrun_amount,
            opportunity.sandwich_target.recommended_frontrun_amount * U256::from(105) / U256::from(100), // Expect 5% profit
            backrun_gas_price.to_string().parse().unwrap_or(20_000_000_000),
            250_000,
        ).await?;

        info!("📤 Submitting backrun transaction to mempool...");
        let backrun_result = executor.send_transaction(
            opportunity.sandwich_target.pool.address,
            Some(backrun_gas_price.to_string().parse().unwrap_or(20_000_000_000)),
            Some(250_000),
            Some(backrun_gas_price.to_string().parse().unwrap_or(20_000_000_000)),
            Some(backrun_gas_price.to_string().parse().unwrap_or(2_000_000_000) / 10),
            backrun_calldata,
            Some(U256::ZERO),
        ).await?;

        info!("✅ Backrun transaction sent: {}", backrun_result.tx_hash);

        // Calculate results
        let total_gas_used = frontrun_result.gas_used.unwrap_or(300_000) + 
                            backrun_result.gas_used.unwrap_or(250_000);
        
        let estimated_profit = if frontrun_result.success && backrun_result.success {
            opportunity.estimated_profit_eth
        } else {
            0.0
        };

        Ok(BundleSubmissionResult {
            bundle_hash: Some(format!("{},{}", frontrun_result.tx_hash, backrun_result.tx_hash)),
            simulation: None,
            submitted: true,
            profit_eth: estimated_profit,
            total_gas_used: total_gas_used as u64,
            coinbase_payment: U256::ZERO, // No coinbase payment in direct mempool
            error: if frontrun_result.success && backrun_result.success {
                None
            } else {
                Some(format!("Frontrun: {}, Backrun: {}", 
                    frontrun_result.error.as_deref().unwrap_or("success"),
                    backrun_result.error.as_deref().unwrap_or("success")
                ))
            },
        })
    }

    /// Simulate sandwich without execution
    async fn simulate_sandwich_only(
        &self,
        opportunity: &MempoolOpportunity,
    ) -> Result<BundleSubmissionResult> {
        info!("🧪 Simulating sandwich attack for: {}", opportunity.victim_tx.hash);
        
        // Simulate the sandwich attack logic
        let estimated_profit = opportunity.estimated_profit_eth;
        let estimated_gas = 600_000u64; // Typical gas for sandwich

        info!(
            "📊 Simulation results: profit={:.4} ETH, gas={}",
            estimated_profit, estimated_gas
        );

        Ok(BundleSubmissionResult {
            bundle_hash: Some(format!("sim_{}", rand::random::<u64>())),
            simulation: None,
            submitted: false,
            profit_eth: estimated_profit,
            total_gas_used: estimated_gas,
            coinbase_payment: U256::from((estimated_profit * 0.9 * 1e18) as u64),
            error: None,
        })
    }

    /// Build MEV bundle from detected opportunity
    pub async fn build_sandwich_bundle(
        &self,
        opportunity: &MempoolOpportunity,
        target_block: u64,
    ) -> Result<MevBundle> {
        debug!("Building MEV bundle for opportunity: {:?}", opportunity.victim_tx.hash);

        // Get current gas price and nonce
        let current_gas_price = self.eth_client.get_gas_price().await?;
        let adjusted_gas_price = self.calculate_optimal_gas_price(current_gas_price, &opportunity.victim_tx)?;

        // Build bundle transactions in order:
        // 1. Flash loan initiation (frontrun)
        // 2. Victim transaction (included from mempool) 
        // 3. Flash loan repayment (backrun)

        let mut bundle_txs = Vec::new();

        // 1. Frontrun transaction (flash loan + buy)
        let frontrun_tx = self.build_frontrun_transaction(
            opportunity,
            adjusted_gas_price,
            target_block,
        ).await?;
        bundle_txs.push(frontrun_tx);

        // 2. Victim transaction (from mempool)
        let victim_tx = self.convert_mempool_tx_to_signed(&opportunity.victim_tx)?;
        bundle_txs.push(victim_tx);

        // 3. Backrun transaction (sell + repay flash loan)
        let backrun_tx = self.build_backrun_transaction(
            opportunity,
            adjusted_gas_price,
            target_block,
        ).await?;
        bundle_txs.push(backrun_tx);

        // Calculate coinbase payment for miner incentive
        let profit_wei = U256::from((opportunity.estimated_profit_eth * 1e18) as u64);
        let coinbase_payment = profit_wei * U256::from(self.coinbase_payment_percent) / U256::from(100);

        // Add coinbase payment transaction if profitable
        if profit_wei > self.min_profit_threshold {
            let coinbase_tx = self.build_coinbase_payment_transaction(
                coinbase_payment,
                adjusted_gas_price,
                target_block,
            ).await?;
            bundle_txs.push(coinbase_tx);
        }

        Ok(MevBundle {
            transactions: bundle_txs,
            block_number: target_block,
            min_timestamp: None,
            max_timestamp: None,
            reverting_tx_hashes: vec![], // Allow all transactions to revert if needed
        })
    }

    /// Submit bundle - compatibility method that routes to execute_sandwich_attack
    pub async fn submit_bundle(&self, bundle: &MevBundle) -> Result<BundleSubmissionResult> {
        // This is a compatibility wrapper - in a real implementation we'd need to
        // reconstruct the opportunity from the bundle, but for now we'll simulate
        warn!("submit_bundle called - this should be replaced with execute_sandwich_attack");
        
        // Create a dummy opportunity for simulation
        let dummy_opportunity = MempoolOpportunity {
            victim_tx: create_dummy_mempool_tx(),
            sandwich_target: crate::sandwich_pool_integration::SandwichTarget {
                victim_tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                pool: crate::sandwich_pool_integration::PoolState {
                    address: Address::ZERO,
                    protocol: "UniswapV2".to_string(),
                    token0: Address::ZERO,
                    token1: Address::ZERO,
                    reserve0: U256::from(1000_000_000_000_000_000_000_000u128), // 1M tokens
                    reserve1: U256::from(1000_000_000_000_000_000_000u128), // 1K ETH
                    fee: 3000, // 0.3%
                    block_number: 0,
                    total_liquidity_usd: 2000000.0, // $2M
                },
                victim_trade_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
                victim_trade_direction: crate::sandwich_pool_integration::TradeDirection::Token0ToToken1,
                recommended_frontrun_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
                estimated_profit_eth: 0.05,
                risk_score: 20, // Low risk
                gas_cost_estimate: U256::from(300_000 * 20_000_000_000u64), // 300k gas * 20 gwei
            },
            estimated_profit_eth: 0.05,
            confidence_score: 0.8,
            time_sensitivity: 30, // 30 seconds
            required_capital_eth: 1.0,
        };
        
        self.simulate_sandwich_only(&dummy_opportunity).await
    }

    /// Calculate optimal gas price for competitive inclusion
    fn calculate_optimal_gas_price(
        &self,
        current_gas_price: U256,
        victim_tx: &MempoolTransaction,
    ) -> Result<U256> {
        // Use victim's gas price + premium for frontrunning
        let victim_gas_price = victim_tx.gas_price;
        let frontrun_premium = victim_gas_price / U256::from(20); // 5% premium
        
        let optimal_price = victim_gas_price + frontrun_premium;
        
        // Cap at our maximum
        if optimal_price > self.max_gas_price {
            warn!(
                "Calculated gas price {} exceeds maximum {}, capping",
                optimal_price, self.max_gas_price
            );
            Ok(self.max_gas_price)
        } else {
            Ok(optimal_price)
        }
    }

    /// Build frontrun transaction (flash loan + initial buy)
    async fn build_frontrun_transaction(
        &self,
        opportunity: &MempoolOpportunity,
        gas_price: U256,
        target_block: u64,
    ) -> Result<SignedTransaction> {
        debug!("Building frontrun transaction for block {}", target_block);

        // Mock implementation - in production this would:
        // 1. Create flash loan request to Balancer V2
        // 2. Encode call to our sandwich contract
        // 3. Include DEX swap to push price up
        // 4. Sign transaction with private key

        let frontrun_data = SandwichExecutionData {
            victim_tx_hash: opportunity.victim_tx.hash.clone(),
            target_pool: opportunity.sandwich_target.pool.address,
            frontrun_amount: opportunity.sandwich_target.recommended_frontrun_amount,
            backrun_amount: opportunity.sandwich_target.recommended_frontrun_amount, // Use same for now
            frontrun_calldata: Bytes::new(),
            backrun_calldata: Bytes::new(),
            min_profit_wei: U256::from((opportunity.estimated_profit_eth * 0.5 * 1e18) as u64),
        };

        // For now, create mock signed transaction
        // In production, this would use actual smart contract calls
        Ok(SignedTransaction {
            raw_tx: "0x".to_string(), // Would be RLP-encoded signed tx
            hash: format!("0x{:064x}", rand::random::<u64>()),
            gas_price,
            gas_limit: U256::from(300_000), // Conservative gas limit for flash loan
            value: U256::ZERO,
            to: Some(opportunity.sandwich_target.pool.address),
            from: Address::ZERO, // Would be our bot's address
            nonce: 0, // Would get from eth_client.get_nonce()
        })
    }

    /// Build backrun transaction (sell + repay flash loan)
    async fn build_backrun_transaction(
        &self,
        opportunity: &MempoolOpportunity,
        gas_price: U256,
        target_block: u64,
    ) -> Result<SignedTransaction> {
        debug!("Building backrun transaction for block {}", target_block);

        // Mock implementation - in production this would:
        // 1. Encode DEX swap to sell tokens at higher price
        // 2. Repay flash loan
        // 3. Keep profit
        // 4. Sign transaction with private key

        Ok(SignedTransaction {
            raw_tx: "0x".to_string(),
            hash: format!("0x{:064x}", rand::random::<u64>()),
            gas_price,
            gas_limit: U256::from(200_000),
            value: U256::ZERO,
            to: Some(opportunity.sandwich_target.pool.address),
            from: Address::ZERO,
            nonce: 1, // Sequential nonce
        })
    }

    /// Build coinbase payment transaction for miner incentive
    async fn build_coinbase_payment_transaction(
        &self,
        payment_amount: U256,
        gas_price: U256,
        target_block: u64,
    ) -> Result<SignedTransaction> {
        debug!("Building coinbase payment transaction: {} ETH", payment_amount);

        Ok(SignedTransaction {
            raw_tx: "0x".to_string(),
            hash: format!("0x{:064x}", rand::random::<u64>()),
            gas_price,
            gas_limit: U256::from(21_000), // Standard ETH transfer
            value: payment_amount,
            to: None, // block.coinbase will be set by Flashbots
            from: Address::ZERO,
            nonce: 2,
        })
    }

    /// Convert mempool transaction to signed transaction format
    fn convert_mempool_tx_to_signed(&self, mempool_tx: &MempoolTransaction) -> Result<SignedTransaction> {
        Ok(SignedTransaction {
            raw_tx: "0x".to_string(), // Would extract from mempool
            hash: mempool_tx.hash.clone(),
            gas_price: mempool_tx.gas_price,
            gas_limit: mempool_tx.gas_limit,
            value: mempool_tx.value,
            to: mempool_tx.to,
            from: mempool_tx.from,
            nonce: mempool_tx.nonce,
        })
    }

    /// Simulate bundle before submission
    pub async fn simulate_bundle(&self, bundle: &MevBundle) -> Result<BundleSimulation> {
        info!("Simulating bundle with {} transactions", bundle.transactions.len());

        // For now, use mock simulation since Flashbots API key handling needs to be updated
        warn!("Using mock simulation for testing");
        return Ok(self.mock_simulation(bundle));

        // Build Flashbots simulation request
        let simulation_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_callBundle",
            "params": [{
                "txs": bundle.transactions.iter().map(|tx| tx.raw_tx.clone()).collect::<Vec<_>>(),
                "blockNumber": format!("0x{:x}", bundle.block_number),
                "stateBlockNumber": "latest"
            }]
        });

        let response = timeout(
            Duration::from_secs(5),
            self.client
                .post(&format!("{}/v1/simulate", FLASHBOTS_RELAY_URL))
                .header("X-Flashbots-Signature", "mock-signature") // Would be real signature
                .json(&simulation_request)
                .send()
        ).await??;

        if response.status().is_success() {
            let simulation_result: BundleSimulation = response.json().await?;
            info!("Bundle simulation successful - coinbase diff: {}", simulation_result.coinbase_diff);
            Ok(simulation_result)
        } else {
            let error_text = response.text().await?;
            Err(anyhow!("Bundle simulation failed: {}", error_text))
        }
    }

    /// Submit bundle to Flashbots (internal method for Flashbots execution)
    async fn submit_bundle_to_flashbots(
        &self,
        bundle: MevBundle,
        api_key: &str,
    ) -> Result<BundleSubmissionResult> {
        info!("Submitting bundle for block {} via Flashbots", bundle.block_number);

        // First simulate the bundle
        let simulation = match self.simulate_bundle(&bundle).await {
            Ok(sim) => Some(sim),
            Err(e) => {
                warn!("Bundle simulation failed: {}", e);
                return Ok(BundleSubmissionResult {
                    bundle_hash: None,
                    simulation: None,
                    submitted: false,
                    profit_eth: 0.0,
                    total_gas_used: 0,
                    coinbase_payment: U256::ZERO,
                    error: Some(format!("Simulation failed: {}", e)),
                });
            }
        };

        // Calculate expected profit
        let profit_wei = if let Some(ref sim) = simulation {
            U256::from_str_radix(&sim.coinbase_diff, 10).unwrap_or(U256::ZERO)
        } else {
            U256::ZERO
        };

        let profit_eth = profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;

        // Check if profitable enough
        if profit_wei < self.min_profit_threshold {
            return Ok(BundleSubmissionResult {
                bundle_hash: None,
                simulation: simulation.clone(),
                submitted: false,
                profit_eth,
                total_gas_used: simulation.as_ref().map(|s| s.gas_used).unwrap_or(0),
                coinbase_payment: U256::ZERO,
                error: Some(format!("Profit {} below threshold {}", profit_wei, self.min_profit_threshold)),
            });
        }

        // Submit to Flashbots - mock implementation for now
        warn!("Mock Flashbots submission - API key: {}", api_key);
        return Ok(BundleSubmissionResult {
            bundle_hash: Some(format!("0x{:064x}", rand::random::<u64>())),
            simulation: simulation.clone(),
            submitted: true,
            profit_eth,
            total_gas_used: simulation.as_ref().map(|s| s.gas_used).unwrap_or(0),
            coinbase_payment: profit_wei * U256::from(self.coinbase_payment_percent) / U256::from(100),
            error: None,
        });
    }
        };

        // Calculate expected profit
        let profit_wei = if let Some(ref sim) = simulation {
            U256::from_str_radix(&sim.coinbase_diff, 10).unwrap_or(U256::ZERO)
        } else {
            U256::ZERO
        };

        let profit_eth = profit_wei.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;

        // Check if profitable enough
        if profit_wei < self.min_profit_threshold {
            return Ok(BundleSubmissionResult {
                bundle_hash: None,
                simulation: simulation.clone(),
                submitted: false,
                profit_eth,
                total_gas_used: simulation.as_ref().map(|s| s.gas_used).unwrap_or(0),
                coinbase_payment: U256::ZERO,
                error: Some(format!("Profit {} below threshold {}", profit_wei, self.min_profit_threshold)),
            });
        }

        // Submit to Flashbots - mock implementation for now
        warn!("Mock Flashbots submission - API key: {}", api_key);
        return Ok(BundleSubmissionResult {
            bundle_hash: Some(format!("0x{:064x}", rand::random::<u64>())),
            simulation: simulation.clone(),
            submitted: true,
            profit_eth,
            total_gas_used: simulation.as_ref().map(|s| s.gas_used).unwrap_or(0),
            coinbase_payment: profit_wei * U256::from(self.coinbase_payment_percent) / U256::from(100),
            error: None,
        });
            Ok(result) => {
                warn!("⚠️ Frontrun transaction failed: {} - {}", result.tx_hash, 
                     result.error.as_ref().unwrap_or(&"Unknown error".to_string()));
                return Ok(BundleSubmissionResult {
                    bundle_hash: Some(result.tx_hash),
                    simulation: None,
                    submitted: true,
                    profit_eth: 0.0,
                    total_gas_used: result.gas_used.unwrap_or(0),
                    coinbase_payment: U256::ZERO,
                    error: result.error,
                });
            }
            Err(e) => {
                error!("❌ Frontrun submission failed: {}", e);
                return Err(e);
            }
        }

        // Step 2: Wait for victim transaction to be confirmed (with timeout)
        info!("👀 Monitoring victim transaction: {}", opportunity.victim_tx.hash);
        let victim_confirmed = self.wait_for_victim_confirmation(
            &opportunity.victim_tx.hash,
            std::time::Duration::from_secs(30),
        ).await;

        if !victim_confirmed {
            warn!("⏰ Victim transaction not confirmed within timeout - proceeding with backrun anyway");
        } else {
            info!("✅ Victim transaction confirmed");
        }

        // Step 3: Submit backrun transaction
        let backrun_data = self.create_backrun_swap_data(
            &opportunity.sandwich_target,
            backrun_gas_price,
        ).await?;

        let backrun_result = direct_executor.send_transaction(
            opportunity.sandwich_target.pool.address,
            Some(backrun_gas_price),
            Some(250_000), // Slightly lower gas for backrun
            Some(backrun_gas_price),
            Some(backrun_gas_price / 10),
            backrun_data,
            Some(U256::ZERO),
        ).await;

        let execution_time = start_time.elapsed();

        match backrun_result {
            Ok(result) if result.success => {
                info!("✅ Backrun transaction confirmed: {} (total time: {:?})", result.tx_hash, execution_time);
                
                // Calculate actual profit from transaction receipts
                let actual_profit = self.calculate_actual_profit(
                    &frontrun_result.unwrap(),
                    &result,
                ).await.unwrap_or(U256::ZERO);

                let profit_eth = actual_profit.to_string().parse::<f64>().unwrap_or(0.0) / 1e18;
                
                Ok(BundleSubmissionResult {
                    bundle_hash: Some(format!("frontrun:{},backrun:{}", 
                                            frontrun_result.unwrap().tx_hash, result.tx_hash)),
                    simulation: None,
                    submitted: true,
                    profit_eth,
                    total_gas_used: frontrun_result.unwrap().gas_used.unwrap_or(0) + result.gas_used.unwrap_or(0),
                    coinbase_payment: U256::ZERO, // No coinbase payment in direct mempool
                    error: None,
                })
            }
            Ok(result) => {
                warn!("⚠️ Backrun transaction failed: {} - still may be profitable", result.tx_hash);
                Ok(BundleSubmissionResult {
                    bundle_hash: Some(format!("frontrun:{},backrun_failed:{}", 
                                            frontrun_result.unwrap().tx_hash, result.tx_hash)),
                    simulation: None,
                    submitted: true,
                    profit_eth: 0.0, // Conservative estimate when backrun fails
                    total_gas_used: frontrun_result.unwrap().gas_used.unwrap_or(0) + result.gas_used.unwrap_or(0),
                    coinbase_payment: U256::ZERO,
                    error: result.error,
                })
            }
            Err(e) => {
                error!("❌ Backrun submission failed: {} - frontrun may still be profitable", e);
                Ok(BundleSubmissionResult {
                    bundle_hash: Some(frontrun_result.unwrap().tx_hash),
                    simulation: None,
                    submitted: true,
                    profit_eth: 0.0, // Unknown profit when backrun fails
                    total_gas_used: frontrun_result.unwrap().gas_used.unwrap_or(0),
                    coinbase_payment: U256::ZERO,
                    error: Some(format!("Backrun failed: {}", e)),
                })
            }
        }
    }

    /// Extract victim transaction gas price for competitive bidding
    async fn extract_victim_gas_price(&self, victim_tx: &MempoolTransaction) -> u128 {
        // Parse gas price from victim transaction
        // For now, use a reasonable default and add premium
        victim_tx.max_fee_per_gas.unwrap_or(20_000_000_000) // 20 gwei default
    }

    /// Wait for victim transaction confirmation with timeout
    async fn wait_for_victim_confirmation(&self, victim_tx_hash: &str, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        
        while start.elapsed() < timeout {
            // In a real implementation, we'd check if the transaction is confirmed
            // For now, simulate with a random delay
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            
            // Mock: assume victim tx confirms after 2-5 seconds
            if start.elapsed() > std::time::Duration::from_secs(2) {
                return rand::random::<f32>() > 0.3; // 70% chance of confirmation
            }
        }
        false
    }

    /// Create frontrun swap transaction data
    async fn create_frontrun_swap_data(
        &self,
        sandwich_target: &crate::sandwich_pool_integration::SandwichTarget,
        gas_price: u128,
    ) -> Result<Vec<u8>> {
        // Create actual swap transaction data for frontrunning
        // This would encode a real DEX swap call
        use alloy_sol_types::{SolCall, sol};
        
        sol! {
            #[derive(Debug)]
            function swapExactETHForTokens(
                uint256 amountOutMin,
                address[] calldata path,
                address to,
                uint256 deadline
            ) external payable returns (uint256[] memory amounts);
        }

        let deadline = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + 300
        );

        let path = vec![
            sandwich_target.pool.token0, // WETH
            sandwich_target.pool.token1, // Target token
        ];

        let min_amount_out = sandwich_target.recommended_frontrun_amount / U256::from(10); // 10% slippage

        let swap_call = swapExactETHForTokensCall {
            amountOutMin: min_amount_out,
            path,
            to: sandwich_target.pool.address, // Would be our contract address
            deadline,
        };

        Ok(swap_call.abi_encode())
    }

    /// Create backrun swap transaction data
    async fn create_backrun_swap_data(
        &self,
        sandwich_target: &crate::sandwich_pool_integration::SandwichTarget,
        gas_price: u128,
    ) -> Result<Vec<u8>> {
        // Create actual swap transaction data for backrunning
        use alloy_sol_types::{SolCall, sol};
        
        sol! {
            #[derive(Debug)]
            function swapExactTokensForETH(
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
        );

        let path = vec![
            sandwich_target.pool.token1, // Target token (sell)
            sandwich_target.pool.token0, // WETH (buy)
        ];

        let amount_in = sandwich_target.recommended_frontrun_amount; // Sell what we bought
        let min_eth_out = amount_in / U256::from(10); // 10% slippage

        let swap_call = swapExactTokensForETHCall {
            amountIn: amount_in,
            amountOutMin: min_eth_out,
            path,
            to: sandwich_target.pool.address, // Would be our contract address
            deadline,
        };

        Ok(swap_call.abi_encode())
    }

    /// Calculate actual profit from transaction receipts
    async fn calculate_actual_profit(
        &self,
        frontrun_result: &crate::transaction_executor::TransactionExecutionResult,
        backrun_result: &crate::transaction_executor::TransactionExecutionResult,
    ) -> Result<U256> {
        // Calculate profit by analyzing transaction receipts and logs
        // This would parse DEX swap events to determine actual amounts
        
        let frontrun_cost = frontrun_result.gas_used.unwrap_or(0) * frontrun_result.gas_price.unwrap_or(0);
        let backrun_cost = backrun_result.gas_used.unwrap_or(0) * backrun_result.gas_price.unwrap_or(0);
        let total_gas_cost = frontrun_cost + backrun_cost;

        // For now, return a mock profit calculation
        // In production, this would parse swap events to calculate exact token amounts
        let estimated_revenue = total_gas_cost * 2; // Assume 2x gas cost as profit
        if estimated_revenue > total_gas_cost {
            Ok(U256::from(estimated_revenue - total_gas_cost))
        } else {
            Ok(U256::ZERO)
        }
    }

    /// Mock simulation for testing without Flashbots API
    fn mock_simulation(&self, bundle: &MevBundle) -> BundleSimulation {
        let total_gas_used: u64 = bundle.transactions.iter()
            .map(|tx| tx.gas_limit.to_string().parse::<u64>().unwrap_or(21_000))
            .sum();

        let gas_fees = total_gas_used as u128 * 20_000_000_000u128; // 20 gwei
        let coinbase_diff = gas_fees / 10; // 10% of gas fees as profit

        BundleSimulation {
            coinbase_diff: coinbase_diff.to_string(),
            gas_fees: gas_fees.to_string(),
            gas_used: total_gas_used,
            results: bundle.transactions.iter().map(|tx| TxSimulation {
                coinbase_diff: "0".to_string(),
                eth_sent_to_coinbase: "0".to_string(),
                from_address: format!("{:?}", tx.from),
                gas_fees: (tx.gas_limit.to_string().parse::<u64>().unwrap_or(21_000) as u128 * 20_000_000_000u128).to_string(),
                gas_price: tx.gas_price.to_string(),
                gas_used: tx.gas_limit.to_string().parse::<u64>().unwrap_or(21_000),
                to_address: tx.to.map(|a| format!("{:?}", a)).unwrap_or_else(|| "coinbase".to_string()),
                tx_hash: tx.hash.clone(),
                value: tx.value.to_string(),
            }).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eth_client::EthereumClient;

    #[tokio::test]
    async fn test_bundle_builder_creation() {
        let eth_client = EthereumClient::new("http://localhost:8545".to_string()).unwrap();
        let builder = MevBundleBuilder::new(eth_client, None, None);
        
        assert_eq!(builder.coinbase_payment_percent, 90);
        assert_eq!(builder.max_gas_price, U256::from(50_000_000_000_u64));
    }

    #[tokio::test]
    async fn test_gas_price_calculation() {
        let eth_client = EthereumClient::new("http://localhost:8545".to_string()).unwrap();
        let builder = MevBundleBuilder::new(eth_client, None, None);
        
        let victim_tx = MempoolTransaction {
            hash: "0x123".to_string(),
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: U256::ZERO,
            gas_price: U256::from(20_000_000_000_u64), // 20 gwei
            gas_limit: U256::from(21_000),
            input: Bytes::new(),
            nonce: 0,
            timestamp: SystemTime::now(),
            is_dex_interaction: true,
            estimated_value_usd: 1000.0,
            target_pool: Some(Address::ZERO),
            trade_direction: None,
        };

        let optimal_price = builder.calculate_optimal_gas_price(
            U256::from(20_000_000_000_u64),
            &victim_tx,
        ).unwrap();

        // Should be victim's price + 5% premium
        let expected = U256::from(21_000_000_000_u64); // 21 gwei
        assert_eq!(optimal_price, expected);
    }
}

/// Helper function to create dummy mempool transaction for compatibility
fn create_dummy_mempool_tx() -> crate::mempool_monitor::MempoolTransaction {
    crate::mempool_monitor::MempoolTransaction {
        hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        from: Address::ZERO,
        to: Some(Address::ZERO),
        value: U256::ZERO,
        gas_price: U256::from(20_000_000_000u64), // 20 gwei
        gas_limit: U256::from(500_000),
        input: alloy_primitives::Bytes::new(),
        nonce: 0,
        timestamp: std::time::SystemTime::now(),
        is_dex_interaction: true,
        estimated_value_usd: 1000.0,
        target_pool: Some(Address::ZERO),
        trade_direction: None,
    }
}