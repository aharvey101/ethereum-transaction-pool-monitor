/// MEV Bundle Builder - Production Implementation
///
/// Handles both Flashbots bundle submission and direct mempool execution
/// for sandwich attacks and MEV extraction.
use crate::{
    flash_loan_manager::{FlashLoanManager, FlashLoanRequest},
    mempool_monitor::{MempoolOpportunity, MempoolTransaction},
    transaction_executor::DirectMempoolExecutor,
};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{sol, SolCall};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct MevBundleBuilder {
    pub min_profit_threshold: U256,
    pub max_gas_price: u128,
    pub coinbase_payment_percent: u8,
    pub flashbots_relay_url: String,
}

#[derive(Debug, Clone)]
pub enum ExecutionMethod {
    Flashbots {
        api_key: String,
    },
    DirectMempool {
        private_key: String,
        aggressive_gas: bool,
    },
    SimulationOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSubmissionResult {
    pub bundle_hash: Option<String>,
    pub simulation: Option<BundleSimulation>,
    pub submitted: bool,
    pub profit_eth: f64,
    pub total_gas_used: u64,
    pub coinbase_payment: U256,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSimulation {
    pub coinbase_diff: String,
    pub gas_fees: String,
    pub gas_used: u64,
    pub success: bool,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MevBundle {
    pub transactions: Vec<SignedTransaction>,
    pub block_number: u64,
    pub min_timestamp: Option<u64>,
    pub max_timestamp: Option<u64>,
    pub reverting_tx_hashes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SignedTransaction {
    pub raw_tx: String,
    pub hash: String,
    pub gas_price: u128,
    pub gas_limit: U256,
    pub value: U256,
    pub to: Option<Address>,
    pub from: Address,
    pub data: Bytes,
    pub nonce: u64,
}

impl MevBundleBuilder {
    pub fn new() -> Self {
        Self {
            min_profit_threshold: U256::from(5_000_000_000_000_000u64), // 0.005 ETH
            max_gas_price: 5_000_000_000,                               // 5 gwei reasonable ceiling
            coinbase_payment_percent: 10,                               // 10% to miner
            flashbots_relay_url: "https://relay.flashbots.net".to_string(),
        }
    }

    /// Execute sandwich attack using the specified method
    pub async fn execute_sandwich_attack(
        &self,
        opportunity: &MempoolOpportunity,
        execution_method: ExecutionMethod,
    ) -> Result<BundleSubmissionResult> {
        info!(
            "🥪 Executing sandwich attack for victim tx: {}",
            opportunity.victim_tx.hash
        );

        match execution_method {
            ExecutionMethod::DirectMempool {
                private_key,
                aggressive_gas,
            } => {
                info!(
                    "🎯 Using direct mempool execution (aggressive: {})",
                    aggressive_gas
                );

                // Create direct executor
                let executor = DirectMempoolExecutor::new(
                    "http://192.168.0.14:8545".to_string(), // Target local node
                    Some(private_key),
                    1, // Mainnet
                );

                self.execute_direct_mempool(&opportunity, &executor, aggressive_gas)
                    .await
            }
            ExecutionMethod::Flashbots { api_key } => {
                info!("🏛️ Using Flashbots bundle submission");
                self.execute_flashbots_bundle(opportunity, &api_key).await
            }
            ExecutionMethod::SimulationOnly => {
                info!("🧪 Simulation mode - no actual execution");
                self.simulate_sandwich_attack(opportunity).await
            }
        }
    }

    /// Execute sandwich via direct mempool (bypasses Flashbots)
    async fn execute_direct_mempool(
        &self,
        opportunity: &MempoolOpportunity,
        executor: &DirectMempoolExecutor,
        aggressive_gas: bool,
    ) -> Result<BundleSubmissionResult> {
        // Validate opportunity before execution
        if let Err(e) = self.validate_opportunity(opportunity).await {
            return Ok(BundleSubmissionResult {
                bundle_hash: None,
                simulation: None,
                submitted: false,
                profit_eth: 0.0,
                total_gas_used: 0,
                coinbase_payment: U256::ZERO,
                error: Some(format!("Validation failed: {}", e)),
            });
        }

        // Get current network gas prices for competitive bidding
        let network_gas_price = executor.get_gas_price().await.unwrap_or(10_000_000_000); // 10 gwei fallback
        let (_base_fee, _priority_fee) = executor.get_fee_history().await.unwrap_or((8_000_000_000, 2_000_000_000)); // 8 + 2 gwei fallback
        
        // Extract victim gas price (handle U256)
        let victim_gas_price = opportunity
            .victim_tx
            .gas_price
            .to_string()
            .parse::<u128>()
            .map_err(|_| {
                anyhow::anyhow!(
                    "Invalid victim transaction gas price: {}",
                    opportunity.victim_tx.gas_price
                )
            })?;

        // Calculate competitive frontrun gas price
        let gas_premium = if aggressive_gas { 20 } else { 10 }; // 20% vs 10% premium
        let minimum_gas_price = 20_000_000_000; // 20 gwei minimum for mainnet
        
        let frontrun_gas_price = if victim_gas_price == 0 {
            // For EIP-1559 transactions, use network gas price + premium
            std::cmp::max(network_gas_price + (network_gas_price * gas_premium / 100), minimum_gas_price)
        } else {
            // For legacy transactions, outbid victim + premium but ensure minimum
            let calculated = victim_gas_price + (victim_gas_price * gas_premium / 100);
            std::cmp::max(calculated, minimum_gas_price)
        };

        info!(
            "💰 Gas strategy: Network {} gwei, Victim {} gwei → Frontrun {} gwei ({}% premium, min 20 gwei)",
            network_gas_price / 1_000_000_000,
            victim_gas_price / 1_000_000_000,
            frontrun_gas_price / 1_000_000_000,
            gas_premium
        );

        // Check account balance and cap frontrun amount (reserve gas costs)
        let account_balance = executor.get_balance().await.unwrap_or(U256::ZERO);
        let estimated_gas_cost = U256::from(frontrun_gas_price) * U256::from(300_000); // 300k gas limit
        let available_balance = if account_balance > estimated_gas_cost {
            (account_balance - estimated_gas_cost) * U256::from(85) / U256::from(100) // Use 85% of balance after gas for safety
        } else {
            U256::ZERO
        };
        
        info!(
            "💰 Account balance: {} ETH, Gas cost: {} ETH, Available for MEV: {} ETH", 
            account_balance.to::<u64>() as f64 / 1e18,
            estimated_gas_cost.to::<u64>() as f64 / 1e18,
            available_balance.to::<u64>() as f64 / 1e18
        );

        // Cap frontrun amount to available balance
        let frontrun_amount = if opportunity.simulation_result.frontrun_amount > available_balance {
            warn!(
                "⚠️ Capping frontrun amount: {} ETH → {} ETH (limited by balance)",
                opportunity.simulation_result.frontrun_amount.to::<u64>() as f64 / 1e18,
                available_balance.to::<u64>() as f64 / 1e18
            );
            available_balance
        } else {
            opportunity.simulation_result.frontrun_amount
        };

        if frontrun_amount < U256::from(1000000000000000u64) { // 0.001 ETH minimum
            return Ok(BundleSubmissionResult {
                bundle_hash: None,
                simulation: None,
                submitted: false,
                profit_eth: 0.0,
                total_gas_used: 0,
                coinbase_payment: U256::ZERO,
                error: Some("Insufficient balance for MEV execution".to_string()),
            });
        }

        // Create frontrun transaction data using available balance
        let frontrun_data = self
            .create_swap_data(
                opportunity.sandwich_target.pool.token0,
                opportunity.sandwich_target.pool.token1,
                frontrun_amount, // Use capped amount
                true, // is_buy
            )
            .await?;

        // Execute frontrun transaction to Uniswap V2 Router
        let uniswap_v2_router = Address::from([0x7a, 0x25, 0x0d, 0x56, 0x30, 0xb4, 0xcf, 0x53, 0x97, 0x39, 0xdf, 0x2c, 0x5d, 0xac, 0xb4, 0xc6, 0x59, 0xf2, 0x48, 0x8d]);
        let frontrun_result = executor
            .send_transaction(
                uniswap_v2_router, // Send to router, not pool
                Some(frontrun_gas_price),
                Some(300_000),
                Some(frontrun_gas_price),
                Some(frontrun_gas_price / 10),
                frontrun_data,
                Some(frontrun_amount), // Use capped amount for ETH value
            )
            .await?;

        if !frontrun_result.success {
            warn!("🔴 Frontrun transaction failed: {:?}", frontrun_result.error);
            
            // Categorize the failure for better handling
            let error_category = self.categorize_transaction_error(&frontrun_result.error);
            let error_message = format!(
                "Frontrun failed ({}): {}",
                error_category,
                frontrun_result.error.unwrap_or_else(|| "Unknown error".to_string())
            );
            
            return Ok(BundleSubmissionResult {
                bundle_hash: Some(frontrun_result.tx_hash),
                simulation: None,
                submitted: true,
                profit_eth: 0.0,
                total_gas_used: frontrun_result.gas_used.unwrap_or(0) as u64,
                coinbase_payment: U256::ZERO,
                error: Some(error_message),
            });
        }

        info!("✅ Frontrun transaction successful: {}", frontrun_result.tx_hash);

        // Wait briefly for victim tx to be mined
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Create backrun transaction data using simulation results
        let backrun_data = self
            .create_swap_data(
                opportunity.sandwich_target.pool.token1,
                opportunity.sandwich_target.pool.token0,
                opportunity.simulation_result.backrun_amount,
                false, // is_sell
            )
            .await?;

        // Calculate proper backrun gas price (same logic as frontrun but less aggressive)
        let backrun_gas_price = if victim_gas_price == 0 {
            // For EIP-1559 transactions, use network gas price + small premium  
            std::cmp::max(network_gas_price + (network_gas_price * 5 / 100), 15_000_000_000) // 5% premium, 15 gwei minimum
        } else {
            // For legacy transactions, use victim gas price + small premium but ensure minimum
            let calculated = victim_gas_price + (victim_gas_price * 5 / 100); // 5% premium for backrun
            std::cmp::max(calculated, 15_000_000_000) // 15 gwei minimum
        };

        info!(
            "⚡ Backrun gas pricing: {} gwei (victim: {} gwei, network: {} gwei)",
            backrun_gas_price / 1_000_000_000,
            victim_gas_price / 1_000_000_000,
            network_gas_price / 1_000_000_000
        );

        // Execute backrun transaction to Uniswap V2 Router  
        let backrun_result = executor
            .send_transaction(
                uniswap_v2_router, // Send to router, not pool
                Some(backrun_gas_price), // Use proper gas price for backrun
                Some(250_000),
                Some(backrun_gas_price),
                Some(backrun_gas_price / 10),
                backrun_data,
                Some(U256::ZERO), // Backrun typically doesn't send ETH value
            )
            .await?;

        // Calculate total profit
        let total_gas_used =
            frontrun_result.gas_used.unwrap_or(0) + backrun_result.gas_used.unwrap_or(0);
        let _total_gas_cost = frontrun_result.gas_price.unwrap_or(0)
            * frontrun_result.gas_used.unwrap_or(0)
            + backrun_result.gas_price.unwrap_or(0) * backrun_result.gas_used.unwrap_or(0);

        // Use profit calculation from simulation results
        let estimated_profit = if backrun_result.success {
            info!("✅ Backrun transaction successful: {}", backrun_result.tx_hash);
            opportunity.simulation_result.net_profit_eth // Use actual simulation profit
        } else {
            warn!("🔴 Backrun transaction failed: {:?}", backrun_result.error);
            
            // Even if backrun fails, we might have made profit from frontrun
            // Calculate partial profit (this would be more sophisticated in production)
            let partial_profit = opportunity.simulation_result.net_profit_eth * 0.3; // Assume 30% of expected profit
            warn!("📊 Estimated partial profit from frontrun only: {} ETH", partial_profit);
            partial_profit
        };

        Ok(BundleSubmissionResult {
            bundle_hash: Some(format!(
                "{}+{}",
                frontrun_result.tx_hash, backrun_result.tx_hash
            )),
            simulation: None,
            submitted: true,
            profit_eth: estimated_profit,
            total_gas_used: total_gas_used as u64,
            coinbase_payment: U256::ZERO,
            error: if backrun_result.success {
                None
            } else {
                backrun_result.error
            },
        })
    }

    /// Execute sandwich via Flashbots bundle
    async fn execute_flashbots_bundle(
        &self,
        _opportunity: &MempoolOpportunity,
        _api_key: &str,
    ) -> Result<BundleSubmissionResult> {
        warn!("🏛️ Flashbots execution not fully implemented - using simulation");

        // Mock Flashbots submission for now
        Ok(BundleSubmissionResult {
            bundle_hash: Some(format!("flashbots_{}", rand::random::<u64>())),
            simulation: Some(self.mock_simulation()),
            submitted: false,  // Set to false until real implementation
            profit_eth: 0.001, // Mock profit
            total_gas_used: 500_000,
            coinbase_payment: U256::from(1_000_000_000_000_000u64), // 0.001 ETH
            error: Some("Flashbots integration pending".to_string()),
        })
    }

    /// Simulate sandwich attack without execution
    async fn simulate_sandwich_attack(
        &self,
        opportunity: &MempoolOpportunity,
    ) -> Result<BundleSubmissionResult> {
        let simulation_id = format!("bundle_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
        
        info!("🧪 === MEV BUNDLE BUILDER SIMULATION STARTED ===");
        info!("📊 Simulation ID: {}", simulation_id);
        info!("🎯 OPPORTUNITY DETAILS:");
        info!("   - Victim TX Hash: {}", opportunity.victim_tx.hash);
        info!("   - Victim TX From: {}", opportunity.victim_tx.from);
        info!("   - Victim TX To: {:?}", opportunity.victim_tx.to);
        info!("   - Victim TX Value: {} wei ({} ETH)", opportunity.victim_tx.value, opportunity.victim_tx.value.to::<u128>() as f64 / 1e18);
        info!("   - Victim TX Gas Limit: {}", opportunity.victim_tx.gas_limit);
        info!("   - Victim TX Gas Price: {} wei ({} gwei)", opportunity.victim_tx.gas_price, opportunity.victim_tx.gas_price.to::<u128>() as f64 / 1e9);
        info!("   - Victim TX Nonce: {}", opportunity.victim_tx.nonce);
        
        info!("💰 SANDWICH TARGET:");
        info!("   - Pool Address: {}", opportunity.sandwich_target.pool.address);
        info!("   - Pool Protocol: {}", opportunity.sandwich_target.pool.protocol);
        info!("   - Token0: {}", opportunity.sandwich_target.pool.token0);
        info!("   - Token1: {}", opportunity.sandwich_target.pool.token1);
        info!("   - Pool Fee: {} basis points", opportunity.sandwich_target.pool.fee);
        info!("   - Pool Reserve0: {} wei", opportunity.sandwich_target.pool.reserve0);
        info!("   - Pool Reserve1: {} wei", opportunity.sandwich_target.pool.reserve1);
        
        info!("🎲 SANDWICH PARAMETERS:");
        info!("   - Recommended Frontrun Amount: {} wei ({} ETH)", opportunity.sandwich_target.recommended_frontrun_amount, opportunity.sandwich_target.recommended_frontrun_amount.to::<u128>() as f64 / 1e18);
        info!("   - Victim Trade Direction: {:?}", opportunity.sandwich_target.victim_trade_direction);
        
        info!("📊 PROFIT ESTIMATES:");
        info!("   - Estimated Profit ETH: {}", opportunity.estimated_profit_eth);
        info!("   - Confidence Score: {}", opportunity.confidence_score);
        info!("   - Time Sensitivity: {}", opportunity.time_sensitivity);
        info!("   - Required Capital ETH: {}", opportunity.required_capital_eth);

        info!("⚡ GAS CALCULATIONS:");
        let estimated_gas = 400_000u64; // More realistic sandwich attack estimate (frontrun + backrun + overhead)
        info!("   - Estimated Gas Units: {}", estimated_gas);
        
        let gas_price = opportunity
            .victim_tx
            .gas_price
            .to_string()
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("Invalid victim transaction gas price in simulation: {}", opportunity.victim_tx.gas_price))?;
            
        info!("   - Gas Price: {} wei ({} gwei)", gas_price, gas_price as f64 / 1e9);
        
        let gas_cost_eth = (estimated_gas as u128 * gas_price) as f64 / 1e18;
        info!("   - Gas Cost ETH: {} ETH", gas_cost_eth);
        
        let estimated_profit = opportunity.estimated_profit_eth - gas_cost_eth;
        info!("   - Net Profit Estimate: {} ETH (Gross: {} - Gas: {})", estimated_profit, opportunity.estimated_profit_eth, gas_cost_eth);
        
        info!("🧪 Simulating sandwich attack...");

        Ok(BundleSubmissionResult {
            bundle_hash: None,
            simulation: Some(BundleSimulation {
                coinbase_diff: ((estimated_profit * 1e18) as u64).to_string(),
                gas_fees: ((gas_cost_eth * 1e18) as u64).to_string(),
                gas_used: estimated_gas,
                success: estimated_profit > 0.0,
                logs: vec![
                    format!("Estimated profit: {} ETH", estimated_profit),
                    format!("Gas cost: {} ETH", gas_cost_eth),
                    format!("Victim tx: {}", opportunity.victim_tx.hash),
                ],
            }),
            submitted: false,
            profit_eth: estimated_profit,
            total_gas_used: estimated_gas,
            coinbase_payment: U256::ZERO,
            error: if estimated_profit <= 0.0 {
                Some("Estimated profit too low".to_string())
            } else {
                None
            },
        })
    }

    /// Create swap transaction data for DEX interactions
    async fn create_swap_data(
        &self,
        token_in: Address,
        token_out: Address,
        amount: U256,
        _is_buy: bool,
    ) -> Result<Vec<u8>> {
        // Create swap calldata for Uniswap V2 style DEX
        // TODO: This should be extended to support different protocols (V3, SushiSwap, etc.)
        
        sol! {
            #[derive(Debug)]
            function swapExactETHForTokens(
                uint256 amountOutMin,
                address[] calldata path,
                address to,
                uint256 deadline
            ) external payable returns (uint256[] memory amounts);

            #[derive(Debug)]
            function swapExactTokensForETH(
                uint256 amountIn,
                uint256 amountOutMin,
                address[] calldata path,
                address to,
                uint256 deadline
            ) external returns (uint256[] memory amounts);
            
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
                .as_secs()
                + 300, // 5 minutes from now
        );

        let path = vec![token_in, token_out];
        // Use our own EOA address as recipient (would be configurable in production)
        let to = Address::from([0x7a, 0x25, 0x0d, 0x56, 0x30, 0xb4, 0xcf, 0x53, 0x97, 0x39, 0xdf, 0x2c, 0x5d, 0xac, 0xb4, 0xc6, 0x59, 0xf2, 0x48, 0x8d]); 
        
        // Calculate minimum output with slippage protection (2% slippage)
        let min_amount_out = amount * U256::from(98) / U256::from(100); 

        // WETH address for ETH/token swaps
        let weth = Address::from([0xc0, 0x2a, 0xaa, 0x39, 0xb2, 0x23, 0xfe, 0x8d, 0x0a, 0x0e, 0x5c, 0x4f, 0x27, 0xea, 0xd9, 0x08, 0x3c, 0x75, 0x6c, 0xc2]);

        if token_in == weth {
            // Buying tokens with ETH
            let swap_call = swapExactETHForTokensCall {
                amountOutMin: min_amount_out,
                path,
                to,
                deadline,
            };
            Ok(swap_call.abi_encode())
        } else if token_out == weth {
            // Selling tokens for ETH
            let swap_call = swapExactTokensForETHCall {
                amountIn: amount,
                amountOutMin: min_amount_out,
                path,
                to,
                deadline,
            };
            Ok(swap_call.abi_encode())
        } else {
            // Token to token swap
            let swap_call = swapExactTokensForTokensCall {
                amountIn: amount,
                amountOutMin: min_amount_out,
                path,
                to,
                deadline,
            };
            Ok(swap_call.abi_encode())
        }
    }

    /// Validate opportunity before execution
    async fn validate_opportunity(&self, opportunity: &MempoolOpportunity) -> Result<()> {
        // 1. Check minimum profit threshold
        let min_threshold_eth = self.min_profit_threshold.to::<u128>() as f64 / 1e18;
        if opportunity.simulation_result.net_profit_eth < min_threshold_eth {
            return Err(anyhow::anyhow!(
                "Profit {} ETH below minimum threshold {} ETH",
                opportunity.simulation_result.net_profit_eth,
                min_threshold_eth
            ));
        }

        // 2. Check gas price limits
        let victim_gas_price = opportunity.victim_tx.gas_price.to::<u128>();
        if victim_gas_price > self.max_gas_price {
            return Err(anyhow::anyhow!(
                "Gas price {} wei exceeds maximum {} wei",
                victim_gas_price,
                self.max_gas_price
            ));
        }

        // 3. Validate simulation accuracy
        if opportunity.simulation_result.simulation_accuracy < 0.5 {
            return Err(anyhow::anyhow!(
                "Simulation accuracy {} too low for safe execution",
                opportunity.simulation_result.simulation_accuracy
            ));
        }

        // 4. Check frontrun/backrun amounts are reasonable
        if opportunity.simulation_result.frontrun_amount == U256::ZERO {
            return Err(anyhow::anyhow!("Invalid frontrun amount: zero"));
        }

        if opportunity.simulation_result.backrun_amount == U256::ZERO {
            return Err(anyhow::anyhow!("Invalid backrun amount: zero"));
        }

        // 5. Verify price impact is not excessive (safety check)
        if opportunity.simulation_result.price_impact > 0.1 { // 10% max
            return Err(anyhow::anyhow!(
                "Price impact {} too high (>10%)",
                opportunity.simulation_result.price_impact
            ));
        }

        // 6. Check confidence score
        if opportunity.confidence_score < 0.5 {
            return Err(anyhow::anyhow!(
                "Confidence score {} too low for execution",
                opportunity.confidence_score
            ));
        }

        info!("✅ Transaction validation passed");
        Ok(())
    }

    /// Categorize transaction errors for better handling and recovery
    fn categorize_transaction_error(&self, error: &Option<String>) -> &'static str {
        match error {
            Some(err) => {
                let err_lower = err.to_lowercase();
                
                if err_lower.contains("insufficient funds") || err_lower.contains("insufficient balance") {
                    "InsufficientFunds"
                } else if err_lower.contains("gas") && (err_lower.contains("too low") || err_lower.contains("underpriced")) {
                    "GasTooLow"
                } else if err_lower.contains("nonce") && err_lower.contains("too low") {
                    "NonceTooLow"
                } else if err_lower.contains("nonce") && err_lower.contains("too high") {
                    "NonceTooHigh"
                } else if err_lower.contains("timeout") || err_lower.contains("deadline") {
                    "Timeout"
                } else if err_lower.contains("revert") || err_lower.contains("execution reverted") {
                    "ExecutionReverted"
                } else if err_lower.contains("slippage") || err_lower.contains("price impact") {
                    "SlippageExceeded"
                } else if err_lower.contains("already known") || err_lower.contains("replacement underpriced") {
                    "TransactionReplacement"
                } else {
                    "Unknown"
                }
            }
            None => "NoError"
        }
    }

    /// Create mock simulation for testing
    fn mock_simulation(&self) -> BundleSimulation {
        BundleSimulation {
            coinbase_diff: "5000000000000000".to_string(), // 0.005 ETH
            gas_fees: "2000000000000000".to_string(),      // 0.002 ETH
            gas_used: 500_000,
            success: true,
            logs: vec![
                "Mock simulation successful".to_string(),
                "Estimated profit: 0.003 ETH".to_string(),
            ],
        }
    }
}

impl Default for MevBundleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Create dummy mempool transaction for testing
pub fn create_dummy_mempool_tx() -> MempoolTransaction {
    MempoolTransaction {
        hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        from: Address::ZERO,
        to: Some(Address::ZERO),
        value: U256::ZERO,
        gas_price: U256::from(20_000_000_000u64),
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
