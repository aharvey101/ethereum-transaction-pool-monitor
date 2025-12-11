/// Real-Time Mempool Monitor for MEV Bot
///
/// This module provides WebSocket-based real-time monitoring of the Ethereum mempool,
/// filtering for DEX transactions that could be potential sandwich targets.
use crate::{
    eth_client::EthereumClient, pool_db::PoolDatabase, sandwich_pool_integration::SandwichTarget,
};
use alloy_primitives::{hex::FromHex, Address, Bytes, U256, keccak256};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::str::FromStr;

use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// Import for dynamic pool discovery
// use crate::dynamic_pool_discovery::{DynamicPoolDiscovery, DynamicDiscoveryConfig};

/// Configuration for mempool monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub min_value_usd: f64,
    pub min_gas_price_gwei: f64,
    pub max_gas_price_gwei: f64,
    pub confidence_threshold: f64,
    pub enable_websocket: bool,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            min_value_usd: 1000.0,
            min_gas_price_gwei: 5.0,
            max_gas_price_gwei: 100.0,
            confidence_threshold: 0.7,
            enable_websocket: true,
        }
    }
}

/// Live mempool transaction for MEV analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolTransaction {
    pub hash: String,
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub gas_price: U256,
    pub gas_limit: U256,
    pub input: Bytes,
    pub nonce: u64,
    pub timestamp: SystemTime,
    pub is_dex_interaction: bool,
    pub estimated_value_usd: f64,
    pub target_pool: Option<Address>,
    pub trade_direction: Option<TradeDirection>,
}

/// Direction of the DEX trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeDirection {
    Buy,  // Buying token with ETH/stable
    Sell, // Selling token for ETH/stable
}

/// MEV opportunity detected in mempool
#[derive(Debug, Clone)]
pub struct MempoolOpportunity {
    pub victim_tx: MempoolTransaction,
    pub sandwich_target: SandwichTarget,
    pub estimated_profit_eth: f64,
    pub confidence_score: f32, // 0-1, how confident we are this will be profitable
    pub time_sensitivity: u64, // seconds until opportunity expires
    pub required_capital_eth: f64,
    pub simulation_result: crate::enhanced_revm_simulator::EnhancedSandwichResult, // Full simulation data for transaction building
}

/// Configuration for mempool monitoring
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    pub min_tx_value_usd: f64,
    pub max_gas_price_gwei: f64,
    pub target_protocols: Vec<String>,
    pub min_profit_threshold_eth: f64,
    pub max_price_impact: f64,
    pub confidence_threshold: f32,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            min_tx_value_usd: 1.0, // $1 minimum transaction size (ultra low for testing)
            max_gas_price_gwei: 200.0, // 200 gwei max gas price (higher for testing)
            target_protocols: vec![
                "UniswapV2".to_string(),
                "UniswapV3".to_string(),
                "SushiSwap".to_string(),
            ],
            min_profit_threshold_eth: 0.0001, // 0.0001 ETH minimum profit (very low for testing)
            max_price_impact: 0.05,           // 5% maximum price impact
            confidence_threshold: 0.7,        // 70% minimum confidence
        }
    }
}

/// Real-time mempool monitor
pub struct MempoolMonitor {
    eth_client: std::sync::Arc<EthereumClient>,
    pool_db: std::sync::Arc<PoolDatabase>,
    config: MempoolConfig,
    known_pools: HashSet<Address>,
    opportunity_sender: mpsc::UnboundedSender<MempoolOpportunity>,
    stats: MonitorStats,
    // dynamic_discovery: Option<std::sync::Arc<DynamicPoolDiscovery>>,
}

#[derive(Debug, Default)]
struct MonitorStats {
    total_transactions_seen: u64,
    dex_transactions_found: u64,
    opportunities_detected: u64,
    profitable_opportunities: u64,
    average_confidence: f32,
}

impl MempoolMonitor {
    /// Create a new mempool monitor
    pub async fn new(
        rpc_url: &str,
        db_path: &str,
        config: MempoolConfig,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MempoolOpportunity>)> {
        info!("🚀 Initializing MEV mempool monitor...");
        debug!("🔗 Connecting to RPC: {}", rpc_url);
        debug!("🗄️  Database path: {}", db_path);

        let eth_client = EthereumClient::new(rpc_url).await?;
        info!("✅ Ethereum client connected");

        let pool_db = PoolDatabase::new(db_path)?;
        info!("✅ Pool database loaded");

        // Wrap clients in Arc for sharing with discovery service
        let eth_client_arc = std::sync::Arc::new(eth_client);
        let pool_db_arc = std::sync::Arc::new(pool_db);

        // Initialize dynamic pool discovery service (TODO: Implement)
        // let discovery_config = DynamicDiscoveryConfig::default();
        // let dynamic_discovery = DynamicPoolDiscovery::new(
        //     eth_client_arc.clone(),
        //     discovery_config,
        // ).await?;
        // let dynamic_discovery = std::sync::Arc::new(dynamic_discovery);
        info!("✅ Dynamic pool discovery service initialized (placeholder)");

        // Set the discovery service on the pool database to enable dynamic discovery (TODO: Implement)
        // let mut pool_db_mut = Arc::try_unwrap(pool_db_arc)
        //     .map_err(|_| anyhow::anyhow!("PoolDatabase Arc has multiple references"))?;
        // pool_db_mut.set_dynamic_discovery(dynamic_discovery.clone());
        // let pool_db_arc = Arc::new(pool_db_mut);
        info!("✅ Dynamic discovery integrated with PoolDatabase (placeholder)");

        // Initialize enhanced simulator (temporarily bypassed to avoid hang)
        info!("✅ Enhanced sandwich simulator ready (bypassed for now)");

        // Pre-load known pool addresses for fast filtering
        debug!("📚 Loading known pool addresses for mempool filtering...");
        let known_pools = Self::load_known_pools(&pool_db_arc).await?;
        info!(
            "✅ Loaded {} known pool addresses for mempool filtering",
            known_pools.len()
        );

        let (opportunity_sender, opportunity_receiver) = mpsc::unbounded_channel();

        let monitor = Self {
            eth_client: eth_client_arc,
            pool_db: pool_db_arc,
            config,
            known_pools,
            opportunity_sender,
            stats: MonitorStats::default(),
            // dynamic_discovery: None,
        };

        Ok((monitor, opportunity_receiver))
    }

    /// Start monitoring the mempool for MEV opportunities
    pub async fn start_monitoring(&mut self) -> Result<()> {
        info!("🔍 Starting real-time mempool monitoring for MEV opportunities");
        debug!("🔧 Debug: About to start monitoring loop");
        info!(
            "📊 Config: min_value=${}, max_gas={} gwei, min_profit={} ETH",
            self.config.min_tx_value_usd,
            self.config.max_gas_price_gwei,
            self.config.min_profit_threshold_eth
        );
        info!(
            "🎯 Monitoring {} protocols: {:?}",
            self.config.target_protocols.len(),
            self.config.target_protocols
        );

        debug!("🔧 Debug: About to start subscription loop");

        // Subscribe to mempool transactions via WebSocket with auto-reconnection
        loop {
            debug!("📡 Attempting to subscribe to pending transactions stream...");

            match self.eth_client.subscribe_pending_transactions().await {
                Ok(mut tx_stream) => {
                    info!("✅ Successfully subscribed to mempool transactions");

                    // Process transactions until connection drops
                    loop {
                        tokio::select! {
                            // Handle new mempool transactions
                            tx_result = tx_stream.recv() => {
                                match tx_result {
                                    Some(tx_hash) => {
                                        //info!("📨 Processing new pending transaction: {}", tx_hash);
                                        let tx_hash_for_debug = tx_hash.clone();

                                        if let Err(e) = self.process_mempool_transaction(tx_hash).await {
                                            info!("⚠️  Failed to process transaction {}: {}", tx_hash_for_debug, e);
                                        }
                                    }
                                    None => {
                                        warn!("📡 WebSocket subscription disconnected, attempting to reconnect...");
                                        break; // Exit inner loop to reconnect
                                    }
                                }
                            }

                            // Print stats every 30 seconds
                            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                                self.print_monitoring_stats();
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "❌ Failed to subscribe to mempool: {}. Retrying in 5 seconds...",
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }

            // Wait before attempting to reconnect
            warn!("🔄 Reconnecting to mempool in 3 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }

    /// Process a single mempool transaction for MEV opportunities
    async fn process_mempool_transaction(&mut self, tx_hash: String) -> Result<()> {
        self.stats.total_transactions_seen += 1;

        // Get full transaction details
        debug!("🔍 Fetching transaction details for: {}", tx_hash);
        let tx_details = self
            .eth_client
            .get_transaction_by_hash(&tx_hash, &self.pool_db, 1)
            .await?;

        // Quick filter: Check if transaction interacts with known pools
        let target_address = match &tx_details.to {
            Some(addr_str) => match addr_str.parse::<Address>() {
                Ok(addr) => addr,
                Err(_) => {
                    debug!("🔍 Skipping tx {} - invalid address format", tx_hash);
                    return Ok(());
                }
            },
            None => {
                debug!("🔍 Skipping tx {} - contract creation", tx_hash);
                return Ok(());
            }
        };

        if !self.known_pools.contains(&target_address) {
            // Check if this is a router transaction before skipping
            let target_string = format!("{:#x}", target_address);
            let is_router = self.pool_db.is_dex_router(&target_string);

            if is_router {
                info!(
                    "🎯 Router transaction detected: {} -> {}",
                    tx_hash, target_address
                );
            } else {
                debug!(
                    "🔍 Skipping tx {} - not a DEX interaction (target: {})",
                    tx_hash, target_address
                );
                return Ok(()); // Not a DEX interaction
            }
        } else {
            info!(
                "🎯 Direct pool interaction: {} -> {}",
                tx_hash, target_address
            );
        }

        debug!(
            "🎯 Found DEX interaction: {} -> {}",
            tx_hash, target_address
        );

        // Convert to our mempool transaction format
        debug!("🔄 Converting transaction to mempool format...");
        let mempool_tx = self.convert_to_mempool_tx(tx_hash, tx_details).await?;

        if !mempool_tx.is_dex_interaction {
            debug!("🔍 Transaction is not a DEX interaction after analysis");
            return Ok(());
        }

        self.stats.dex_transactions_found += 1;
        debug!(
            "🎯 DEX transaction detected: {} (${:.0})",
            mempool_tx.hash, mempool_tx.estimated_value_usd
        );

        // Check gas price threshold
        let gas_price_gwei = mempool_tx.gas_price.to::<u64>() as f64 / 1e9;
        if gas_price_gwei > self.config.max_gas_price_gwei {
            debug!(
                "⛽ Gas price too high: {:.1} gwei > {:.1} gwei",
                gas_price_gwei, self.config.max_gas_price_gwei
            );
            return Ok(());
        }

        info!(
            "✨ Potential MEV target: {} (${:.0}, {:.1} gwei)",
            mempool_tx.hash, mempool_tx.estimated_value_usd, gas_price_gwei
        );

        // Analyze for sandwich opportunity
        debug!("🧠 Analyzing sandwich opportunity for transaction...");
        if let Some(opportunity) = self.analyze_sandwich_opportunity(mempool_tx).await? {
            self.stats.opportunities_detected += 1;
            debug!(
                "🎯 Opportunity found! Estimated profit: {:.4} ETH, confidence: {:.1}%",
                opportunity.estimated_profit_eth,
                opportunity.confidence_score * 100.0
            );

            if opportunity.estimated_profit_eth >= self.config.min_profit_threshold_eth
                && opportunity.confidence_score >= self.config.confidence_threshold
            {
                self.stats.profitable_opportunities += 1;
                info!(
                    "💰 Profitable MEV opportunity detected: {:.4} ETH profit ({}% confidence)",
                    opportunity.estimated_profit_eth,
                    (opportunity.confidence_score * 100.0) as u32
                );

                // Send opportunity to execution engine
                debug!("📤 Sending opportunity to execution engine...");
                if let Err(e) = self.opportunity_sender.send(opportunity) {
                    error!("❌ Failed to send opportunity to executor: {}", e);
                } else {
                    debug!("✅ Opportunity sent successfully");
                }
            } else {
                debug!("📊 Opportunity below thresholds - profit: {:.4} ETH (min: {:.4}), confidence: {:.1}% (min: {:.1}%)",
                    opportunity.estimated_profit_eth, self.config.min_profit_threshold_eth,
                    opportunity.confidence_score * 100.0, self.config.confidence_threshold * 100.0);
            }
        } else {
            debug!("🔍 No sandwich opportunity found for this transaction");
        }

        Ok(())
    }

    /// Analyze a DEX transaction for sandwich opportunity
    async fn analyze_sandwich_opportunity(
        &mut self,
        victim_tx: MempoolTransaction,
    ) -> Result<Option<MempoolOpportunity>> {
        let opportunity_id = format!("opp_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
        
        info!("🔍 === SANDWICH OPPORTUNITY ANALYSIS STARTED ===");
        info!("📊 Analysis ID: {}", opportunity_id);
        info!("🎯 VICTIM TRANSACTION DETAILS:");
        info!("   - TX Hash: {}", victim_tx.hash);
        info!("   - From: {}", victim_tx.from);
        info!("   - To: {:?}", victim_tx.to);
        info!("   - Value: {} wei ({} ETH)", victim_tx.value, victim_tx.value.to::<u128>() as f64 / 1e18);
        info!("   - Gas Price: {} wei ({} gwei)", victim_tx.gas_price, victim_tx.gas_price.to::<u128>() as f64 / 1e9);
        info!("   - Gas Limit: {}", victim_tx.gas_limit);
        info!("   - Nonce: {}", victim_tx.nonce);
        info!("   - Input Data Length: {} bytes", victim_tx.input.len());
        info!("   - Is DEX: {}", victim_tx.is_dex_interaction);
        
        let pool_address = match victim_tx.target_pool {
            Some(addr) => {
                info!("   - Target Pool: {:#x}", addr);
                addr
            },
            None => {
                info!("   - No target pool identified, skipping analysis");
                return Ok(None);
            },
        };

        // Create sandwich target for simulation
        info!("🎯 Creating sandwich target for analysis...");
        let mock_target = self
            .create_sandwich_target(&victim_tx, pool_address)
            .await?;
        info!(
            "✅ Successfully created sandwich target for pool: {:#x}",
            pool_address
        );
        
        info!("📊 SANDWICH TARGET DETAILS:");
        info!("   - Pool Address: {}", mock_target.pool.address);
        info!("   - Pool Protocol: {}", mock_target.pool.protocol);
        info!("   - Token0: {}", mock_target.pool.token0);
        info!("   - Token1: {}", mock_target.pool.token1);
        info!("   - Recommended Frontrun: {} wei", mock_target.recommended_frontrun_amount);
        info!("   - Trade Direction: {:?}", mock_target.victim_trade_direction);

        // Simulate the sandwich attack (temporarily mocked to avoid simulator hang)
        info!(
            "🧮 Running basic sandwich simulation for victim tx: {}",
            victim_tx.hash
        );

        // Get pool details from database for realistic simulation
        let pool_details = match self
            .pool_db
            .get_pool_by_address(&format!("{:#x}", pool_address))
        {
            Ok(Some(pool)) => pool,
            Ok(None) => {
                warn!("Pool not found in database: {:#x}", pool_address);
                return Ok(None);
            }
            Err(e) => {
                error!("Failed to query pool database: {}", e);
                return Ok(None);
            }
        };

        // Run basic sandwich simulation using pool state fetcher
        let simulation_result = match self
            .run_basic_sandwich_simulation(&victim_tx, &pool_details)
            .await
        {
            Ok(Some(result)) => result,
            Ok(None) => {
                info!("⚠️ Simulation determined sandwich not profitable");
                return Ok(None);
            }
            Err(e) => {
                error!("❌ Simulation failed: {}", e);
                return Ok(None);
            }
        };

        // Always log simulation results for debugging
        info!("📊 Simulation Result - Success: {}, Profit: {:.4} ETH, Gas Cost: {:.4} ETH, Net: {:.4} ETH",
            simulation_result.success,
            simulation_result.profit_eth,
            simulation_result.gas_cost_eth,
            simulation_result.net_profit_eth
        );

        if !simulation_result.success {
            return Ok(None);
        }

        // Calculate confidence score based on multiple factors
        let confidence = self.calculate_confidence_score(&victim_tx, &simulation_result);

        let opportunity = MempoolOpportunity {
            victim_tx,
            sandwich_target: mock_target,
            estimated_profit_eth: simulation_result.net_profit_eth,
            confidence_score: confidence,
            time_sensitivity: 12, // ~12 seconds before next block
            required_capital_eth: (simulation_result.frontrun_amount / U256::from(10u64.pow(18)))
                .to::<u64>() as f64,
            simulation_result,
        };

        Ok(Some(opportunity))
    }

    /// Calculate confidence score for the opportunity (0-1)
    fn calculate_confidence_score(
        &self,
        victim_tx: &MempoolTransaction,
        simulation: &crate::enhanced_revm_simulator::EnhancedSandwichResult,
    ) -> f32 {
        let mut confidence = 0.0f32;

        // Base confidence from simulation success
        confidence += 0.3;

        // Profit margin factor (higher profit = higher confidence)
        let profit_factor = (simulation.net_profit_eth / 0.1).min(1.0) as f32; // Cap at 0.1 ETH
        confidence += 0.2 * profit_factor;

        // Low price impact is good
        let impact_factor = (1.0 - simulation.price_impact).max(0.0) as f32;
        confidence += 0.2 * impact_factor;

        // Transaction value factor (higher value = more predictable)
        let value_factor = (victim_tx.estimated_value_usd / 100_000.0).min(1.0) as f32;
        confidence += 0.1 * value_factor;

        // Gas price factor (reasonable gas = higher confidence)
        let gas_gwei = victim_tx.gas_price.to::<u64>() as f64 / 1e9;
        let gas_factor = if gas_gwei < 50.0 {
            1.0
        } else {
            (100.0 - gas_gwei).max(0.0) / 50.0
        };
        confidence += 0.1 * gas_factor as f32;

        // Risk score factor (lower risk = higher confidence)
        let risk_factor = (100 - simulation.risk_score) as f32 / 100.0;
        confidence += 0.1 * risk_factor;

        confidence.min(1.0)
    }

    /// Load known pool addresses from database for fast filtering
    async fn load_known_pools(pool_db: &PoolDatabase) -> Result<HashSet<Address>> {
        let mut pools = HashSet::new();

        // Load from each protocol for Ethereum mainnet (chain_id = 1)
        for protocol in &["UniswapV2", "UniswapV3", "SushiSwap", "Curve"] {
            let protocol_pools = pool_db.get_pools_by_protocol(protocol, 1)?; // Ethereum mainnet
            info!(
                "📊 Loading {} pools for protocol: {}",
                protocol_pools.len(),
                protocol
            );
            for pool in protocol_pools {
                if let Ok(addr) = pool.address.parse::<Address>() {
                    pools.insert(addr);
                }
            }
        }

        info!("📊 Total known pool addresses loaded: {}", pools.len());
        Ok(pools)
    }

    /// Convert raw transaction to our mempool transaction format
    async fn convert_to_mempool_tx(
        &self,
        tx_hash: String,
        tx_details: crate::eth_client::MempoolTransaction,
    ) -> Result<MempoolTransaction> {
        // Parse from address
        let from_addr = Address::from_str(&tx_details.from)?;
        let to_addr = tx_details
            .to
            .as_ref()
            .and_then(|s| Address::from_str(s).ok());

        // Convert hex values to proper types using the pre-computed f64 values
        let eth_value = tx_details.value_f64;
        let value = U256::from((eth_value * 1e18) as u64);

        let gas_price_gwei = tx_details.gas_price_f64;
        let gas_price = U256::from((gas_price_gwei * 1e9) as u64);

        // Parse gas limit from hex string
        let gas_limit = U256::from_str_radix(tx_details.gas.trim_start_matches("0x"), 16)
            .unwrap_or(U256::from(21000u64));

        // Parse input data
        let input = Bytes::from_hex(&tx_details.data).unwrap_or_default();

        // Estimate USD value (simplified)
        let estimated_value_usd = eth_value * 2000.0; // Assume $2000 ETH

        // Check if this is a DEX interaction (either direct pool or router)
        let is_dex = to_addr.map_or(false, |addr| {
            let addr_string = format!("{:#x}", addr); // Use hex format like 0x1234...
            let is_pool = self.known_pools.contains(&addr);
            let is_router = self.pool_db.is_dex_router(&addr_string);

            // Log every transaction with value > $1 for debugging
            if estimated_value_usd > 1.0 {
                info!(
                    "🔍 Transaction to: {} (${:.2}) - Pool: {}, Router: {}",
                    addr_string, estimated_value_usd, is_pool, is_router
                );
            }

            is_pool || is_router
        });

        let target_pool = if is_dex {
            // For direct pool interactions, use the to_addr
            if self.known_pools.contains(&to_addr.unwrap_or_default()) {
                to_addr
            } else {
                // For router transactions, parse the input data to find target pool
                if let Some(addr) = to_addr {
                    self.parse_router_target_pool(&input, &addr.to_string())
                        .await
                        .unwrap_or_else(|e| {
                            debug!("⚠️ Failed to parse router target pool: {}", e);
                            None
                        })
                } else {
                    None
                }
            }
        } else {
            None
        };

        Ok(MempoolTransaction {
            hash: tx_hash,
            from: from_addr,
            to: to_addr,
            value,
            gas_price,
            gas_limit,
            input,
            nonce: tx_details.nonce,
            timestamp: SystemTime::now(),
            is_dex_interaction: is_dex,
            estimated_value_usd,
            target_pool,
            trade_direction: None, // TODO: Analyze trade direction from input
        })
    }

    /// Create sandwich target from victim transaction
    async fn create_sandwich_target(
        &mut self,
        victim_tx: &MempoolTransaction,
        pool_address: Address,
    ) -> Result<SandwichTarget> {
        // Get pool information from database
        let pools = self
            .pool_db
            .get_pools_by_address(&pool_address.to_string())?;
        let pool_info = pools
            .first()
            .ok_or_else(|| anyhow::anyhow!("Pool not found"))?;

        // Create pool state
        let pool_state = crate::sandwich_pool_integration::PoolState {
            address: pool_address,
            protocol: pool_info.protocol.clone(),
            token0: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse()?, // WETH
            token1: "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse()?, // USDT
            fee: 3000,                                                     // Default 0.3% fee
            reserve0: U256::from(1000000000000000000000000_u128),          // Mock reserves
            reserve1: U256::from(1000000000000_u128),
            block_number: 0,               // Mock block number
            total_liquidity_usd: 100000.0, // Mock liquidity
        };

        Ok(SandwichTarget {
            victim_tx_hash: victim_tx.hash.clone(),
            pool: pool_state,
            victim_trade_amount: victim_tx.value,
            victim_trade_direction:
                crate::sandwich_pool_integration::TradeDirection::Token0ToToken1,
            recommended_frontrun_amount: victim_tx.value * U256::from(2),
            estimated_profit_eth: 0.0, // Will be calculated by simulation
            risk_score: 30,
            gas_cost_estimate: U256::from(400_000u64) * U256::from(500_000_000u64), // 400k gas * 0.5 gwei
        })
    }

    /// Print monitoring statistics
    fn print_monitoring_stats(&mut self) {
        info!("📊 Mempool Monitor Stats:");
        info!(
            "   Total transactions: {}",
            self.stats.total_transactions_seen
        );
        info!(
            "   DEX transactions: {} ({:.1}%)",
            self.stats.dex_transactions_found,
            (self.stats.dex_transactions_found as f64
                / self.stats.total_transactions_seen.max(1) as f64)
                * 100.0
        );
        info!(
            "   Opportunities detected: {}",
            self.stats.opportunities_detected
        );
        info!(
            "   Profitable opportunities: {} ({:.1}%)",
            self.stats.profitable_opportunities,
            (self.stats.profitable_opportunities as f64
                / self.stats.opportunities_detected.max(1) as f64)
                * 100.0
        );

        // Update average confidence
        if self.stats.opportunities_detected > 0 {
            info!(
                "   Average confidence: {:.1}%",
                self.stats.average_confidence * 100.0
            );
        }
    }

    /// Parse router transaction input to find target pool
    async fn parse_router_target_pool(
        &self,
        input_data: &Bytes,
        _router_address: &str,
    ) -> Result<Option<Address>> {
        if input_data.len() < 4 {
            return Ok(None);
        }

        // Extract function selector (first 4 bytes)
        let function_selector = &input_data[0..4];
        
        info!("🔍 Function selector: 0x{}", hex::encode(function_selector));
        info!("🔍 Input data length: {} bytes", input_data.len());
        info!("🔍 Input data (first 100 bytes): 0x{}", hex::encode(&input_data[..std::cmp::min(100, input_data.len())]));

        // Common Uniswap V2 Router function selectors
        match function_selector {
            // swapExactTokensForTokens(uint256,uint256,address[],address,uint256)
            [0x38, 0xed, 0x17, 0x39] => {
                info!("📊 Detected swapExactTokensForTokens");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            // swapExactETHForTokens(uint256,address[],address,uint256)
            [0x7f, 0xf3, 0x6a, 0xb5] => {
                info!("📊 Detected swapExactETHForTokens");
                self.parse_uniswap_v2_eth_swap(&input_data[4..]).await
            }
            // swapExactTokensForETH(uint256,uint256,address[],address,uint256)
            [0x18, 0xcb, 0xaf, 0xe5] => {
                info!("📊 Detected swapExactTokensForETH");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            // swapTokensForExactTokens(uint256,uint256,address[],address,uint256)
            [0x8f, 0x0e, 0x15, 0xa4] => {
                info!("📊 Detected swapTokensForExactTokens");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            // swapTokensForExactETH(uint256,uint256,address[],address,uint256)
            [0x4a, 0x25, 0xa9, 0x4a] => {
                info!("📊 Detected swapTokensForExactETH");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            // swapETHForExactTokens(uint256,address[],address,uint256)
            [0xfb, 0x3b, 0xdb, 0x41] => {
                info!("📊 Detected swapETHForExactTokens");
                self.parse_uniswap_v2_eth_swap(&input_data[4..]).await
            }
            // swapExactTokensForTokensSupportingFeeOnTransferTokens(uint256,uint256,address[],address,uint256)
            [0x79, 0x1a, 0xc9, 0x47] => {
                info!("📊 Detected swapExactTokensForTokensSupportingFeeOnTransferTokens");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            // Uniswap V3 Router functions
            // exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))
            [0x41, 0x4b, 0xf3, 0x89] => {
                info!("📊 Detected Uniswap V3 exactInputSingle");
                self.parse_uniswap_v3_exact_input_single(&input_data[4..]).await
            }
            // exactInput((bytes,address,uint256,uint256,uint256))
            [0xb8, 0x58, 0x18, 0x3f] => {
                info!("📊 Detected Uniswap V3 exactInput");
                self.parse_uniswap_v3_exact_input(&input_data[4..]).await
            }
            // exactOutputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))
            [0xdb, 0x3e, 0x21, 0x98] => {
                info!("📊 Detected Uniswap V3 exactOutputSingle");
                self.parse_uniswap_v3_exact_output_single(&input_data[4..]).await
            }
            // multicall(bytes[])
            [0xac, 0x96, 0x50, 0xd8] => {
                info!("📊 Detected Uniswap V3 multicall (0xac9650d8)");
                self.parse_uniswap_v3_multicall(&input_data[4..]).await
            }
            // multicall(uint256,bytes[]) - Uniswap V3 Router V2
            [0x5a, 0xe4, 0x01, 0xdc] => {
                info!("📊 Detected Uniswap V3 multicall with deadline (0x5ae401dc)");
                self.parse_uniswap_v3_multicall(&input_data[4..]).await
            }
            // removeLiquidityETHWithPermit and other functions
            [0xde, 0xd9, 0x38, 0x2a] => {
                info!("📊 Detected router function (0xded9382a) - attempting V2 parsing");
                self.parse_uniswap_v2_swap(&input_data[4..]).await
            }
            _ => {
                debug!(
                    "🔍 Unknown router function selector: 0x{}",
                    hex::encode(function_selector)
                );
                Ok(None)
            }
        }
    }

    /// Parse Uniswap V2 swap (5 parameters: amountIn, amountOutMin, path, to, deadline)
    async fn parse_uniswap_v2_swap(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V2 SWAP ===");
        info!("📊 Input data length: {} bytes", params_data.len());
        
        if params_data.len() < 160 {
            info!(
                "🔍 Insufficient data for Uniswap V2 swap: {} bytes (minimum: 160)",
                params_data.len()
            );
            return Ok(None);
        }

        // Show hex dump of first 200 bytes for debugging
        if params_data.len() > 0 {
            let dump_len = std::cmp::min(200, params_data.len());
            info!("📋 Hex dump (first {} bytes):", dump_len);
            for (i, chunk) in params_data[..dump_len].chunks(32).enumerate() {
                info!("   {:02x}: {}", i * 32, hex::encode(chunk));
            }
        }

        // Path is 3rd parameter (index 2) - offset is at bytes 64-95 (32-byte word)
        // Read the offset value from the last 4 bytes of the 32-byte word
        if params_data.len() < 96 {
            info!("🔍 Not enough data to read path offset: {} bytes", params_data.len());
            return Ok(None);
        }

        let path_offset = u32::from_be_bytes([
            params_data[92],  // Last 4 bytes of the 32-byte offset word
            params_data[93],
            params_data[94],
            params_data[95],
        ]) as usize;

        info!("🔍 Path offset: {} bytes (0x{:x})", path_offset, path_offset);
        
        // Show the raw bytes we read for the offset
        info!("🔍 Path offset bytes: 0x{}", hex::encode(&params_data[92..96]));
        info!("🔍 Full offset word: 0x{}", hex::encode(&params_data[64..96]));

        // Validate offset is within bounds
        if path_offset >= params_data.len() || path_offset + 32 > params_data.len() {
            info!(
                "🔍 Path offset out of bounds: offset={}, data_len={}",
                path_offset,
                params_data.len()
            );
            return Ok(None);
        }

        // Read array length (first 32 bytes at offset) - take last 4 bytes 
        let path_length = u32::from_be_bytes([
            params_data[path_offset + 28],
            params_data[path_offset + 29],
            params_data[path_offset + 30],
            params_data[path_offset + 31],
        ]) as usize;

        info!("🔍 Path length: {} tokens", path_length);
        info!("🔍 Path length bytes: 0x{}", hex::encode(&params_data[path_offset + 28..path_offset + 32]));
        info!("🔍 Full length word: 0x{}", hex::encode(&params_data[path_offset..path_offset + 32]));

        if path_length < 2 {
            info!("🔍 Path too short: {} tokens (minimum: 2)", path_length);
            return Ok(None);
        }
        
        if path_length > 10 {
            info!("⚠️ Path unusually long: {} tokens - possible parsing error", path_length);
            return Ok(None);
        }

        // Validate we have enough data for the tokens
        let tokens_start = path_offset + 32;
        let required_length = tokens_start + (path_length * 32);

        if required_length > params_data.len() {
            info!(
                "🔍 Not enough data for token array: need {}, have {}",
                required_length,
                params_data.len()
            );
            return Ok(None);
        }

        // Extract first and last token addresses (each address is 32 bytes with 12 byte padding)
        let token0_start = tokens_start + 12; // Skip padding
        let token1_start = tokens_start + (path_length - 1) * 32 + 12; // Last token + skip padding
        
        info!("🔍 Token extraction positions:");
        info!("   - tokens_start: {}", tokens_start);
        info!("   - token0_start: {}", token0_start);
        info!("   - token1_start: {}", token1_start);

        if token0_start + 20 > params_data.len() || token1_start + 20 > params_data.len() {
            info!(
                "🔍 Token positions out of bounds: token0_end={}, token1_end={}, data_len={}",
                token0_start + 20, token1_start + 20, params_data.len()
            );
            return Ok(None);
        }

        let token0_bytes = &params_data[token0_start..token0_start + 20];
        let token1_bytes = &params_data[token1_start..token1_start + 20];

        let token0 = Address::from_slice(token0_bytes);
        let token1 = Address::from_slice(token1_bytes);
        
        // Extract amounts from V2 swap (amountIn and amountOutMin are first two 32-byte parameters)
        let amount_in = if params_data.len() >= 32 {
            u64::from_be_bytes([
                params_data[24], params_data[25], params_data[26], params_data[27],
                params_data[28], params_data[29], params_data[30], params_data[31],
            ])
        } else { 0 };

        let amount_out_min = if params_data.len() >= 64 {
            u64::from_be_bytes([
                params_data[56], params_data[57], params_data[58], params_data[59],
                params_data[60], params_data[61], params_data[62], params_data[63],
            ])
        } else { 0 };
        
        info!("🔍 Extracted amounts:");
        info!("   - Amount In: {} wei ({} ETH)", amount_in, amount_in as f64 / 1e18);
        info!("   - Amount Out Min: {} wei ({} ETH)", amount_out_min, amount_out_min as f64 / 1e18);
        info!("🔍 Extracted token addresses:");
        info!("   - Token 0: {} (bytes: 0x{})", token0, hex::encode(token0_bytes));
        info!("   - Token {} (last): {} (bytes: 0x{})", path_length - 1, token1, hex::encode(token1_bytes));

        info!("🔍 Token pair for pool lookup: {} -> {}", token0, token1);

        self.find_pool_for_token_pair(token0, token1).await
    }

    /// Parse Uniswap V2 ETH swap (4 parameters: amountOutMin, path, to, deadline)
    async fn parse_uniswap_v2_eth_swap(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V2 ETH SWAP ===");
        info!("📊 Input data length: {} bytes", params_data.len());
        
        if params_data.len() < 128 {
            info!(
                "🔍 Insufficient data for ETH swap: {} bytes (minimum: 128)",
                params_data.len()
            );
            return Ok(None);
        }

        // Path is 2nd parameter (index 1) - offset is at bytes 32-63
        if params_data.len() < 64 {
            info!("🔍 Not enough data to read ETH swap path offset: {} bytes", params_data.len());
            return Ok(None);
        }

        let path_offset = u32::from_be_bytes([
            params_data[60],  // Last 4 bytes of the path offset word (bytes 32-63)
            params_data[61],
            params_data[62],
            params_data[63],
        ]) as usize;

        info!("🔍 ETH swap path offset: {} bytes (0x{:x})", path_offset, path_offset);
        info!("🔍 ETH swap path offset bytes: 0x{}", hex::encode(&params_data[60..64]));

        // Validate offset is within bounds
        if path_offset >= params_data.len() || path_offset + 32 > params_data.len() {
            info!(
                "🔍 ETH swap path offset out of bounds: offset={}, data_len={}",
                path_offset,
                params_data.len()
            );
            return Ok(None);
        }

        // Read array length (first 32 bytes at offset)
        let path_length = u32::from_be_bytes([
            params_data[path_offset + 28],
            params_data[path_offset + 29],
            params_data[path_offset + 30],
            params_data[path_offset + 31],
        ]) as usize;

        info!("🔍 ETH swap path length: {} tokens", path_length);

        if path_length < 2 {
            info!("🔍 ETH swap path too short: {} tokens", path_length);
            return Ok(None);
        }
        
        if path_length > 10 {
            info!("⚠️ ETH swap path unusually long: {} tokens - possible parsing error", path_length);
            return Ok(None);
        }

        // Validate we have enough data for the tokens
        let tokens_start = path_offset + 32;
        let required_length = tokens_start + (path_length * 32);

        if required_length > params_data.len() {
            info!(
                "🔍 ETH swap not enough data for token array: need {}, have {}",
                required_length,
                params_data.len()
            );
            return Ok(None);
        }

        // Extract first and second token addresses
        let token0_start = tokens_start + 12; // Skip padding
        let token1_start = tokens_start + 32 + 12; // Next address + skip padding

        let token0_bytes = &params_data[token0_start..token0_start + 20];
        let token1_bytes = &params_data[token1_start..token1_start + 20];

        let token0 = Address::from_slice(token0_bytes);
        let token1 = Address::from_slice(token1_bytes);

        // Extract amountOutMin (first parameter, bytes 0-31, take last 8 bytes as u64)
        let amount_out_min = if params_data.len() >= 32 {
            u64::from_be_bytes([
                params_data[24], params_data[25], params_data[26], params_data[27],
                params_data[28], params_data[29], params_data[30], params_data[31],
            ])
        } else {
            0
        };

        info!("🔍 ETH swap extracted amounts:");
        info!("   - Amount Out Min: {} wei ({} ETH)", amount_out_min, amount_out_min as f64 / 1e18);
        info!("🔍 ETH swap extracted token pair: {} -> {}", token0, token1);

        self.find_pool_for_token_pair(token0, token1).await
    }

    /// Parse Uniswap V3 exactInputSingle function
    /// Function signature: exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))
    async fn parse_uniswap_v3_exact_input_single(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 EXACT INPUT SINGLE ===");
        info!("📊 Input data length: {} bytes", params_data.len());
        
        if params_data.len() < 256 {
            info!("🔍 Insufficient data for V3 exactInputSingle: {} bytes (minimum: 256)", params_data.len());
            return Ok(None);
        }

        // Parameters are packed in a struct at offset 0
        // tokenIn (address): bytes 12-31 (first 32 bytes, skip padding)
        // tokenOut (address): bytes 44-63 (second 32 bytes, skip padding)
        
        let token_in_bytes = &params_data[12..32];
        let token_out_bytes = &params_data[44..64];

        let token_in = Address::from_slice(token_in_bytes);
        let token_out = Address::from_slice(token_out_bytes);

        // Extract amount from V3 exactInputSingle struct
        // amountIn is typically at bytes 128-159 (5th parameter in struct)
        let amount_in = if params_data.len() >= 160 {
            u64::from_be_bytes([
                params_data[152], params_data[153], params_data[154], params_data[155],
                params_data[156], params_data[157], params_data[158], params_data[159],
            ])
        } else { 0 };

        info!("🔍 V3 exactInputSingle extracted amounts:");
        info!("   - Amount In: {} wei ({} ETH)", amount_in, amount_in as f64 / 1e18);
        info!("🔍 V3 exactInputSingle extracted tokens: {} -> {}", token_in, token_out);

        self.find_pool_for_token_pair(token_in, token_out).await
    }

    /// Parse Uniswap V3 exactInput function (multi-hop)
    /// Function signature: exactInput((bytes,address,uint256,uint256,uint256))
    async fn parse_uniswap_v3_exact_input(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 EXACT INPUT (MULTI-HOP) ===");
        info!("📊 Input data length: {} bytes", params_data.len());
        
        if params_data.len() < 160 {
            info!("🔍 Insufficient data for V3 exactInput: {} bytes (minimum: 160)", params_data.len());
            return Ok(None);
        }

        // The path is encoded in the first parameter (bytes)
        // For now, extract the first and last tokens from the path
        // This is a simplified implementation - V3 paths are more complex
        
        // Path offset is at bytes 0-31 (first parameter)
        let path_offset = u32::from_be_bytes([
            params_data[28], params_data[29], params_data[30], params_data[31]
        ]) as usize;

        info!("🔍 V3 path offset: {} bytes", path_offset);

        if path_offset >= params_data.len() || path_offset + 32 > params_data.len() {
            info!("🔍 V3 path offset out of bounds");
            return Ok(None);
        }

        // Path length
        let path_length = u32::from_be_bytes([
            params_data[path_offset + 28], params_data[path_offset + 29], 
            params_data[path_offset + 30], params_data[path_offset + 31]
        ]) as usize;

        info!("🔍 V3 path length: {} bytes", path_length);

        if path_length < 43 { // Minimum: 20 (token) + 3 (fee) + 20 (token)
            info!("🔍 V3 path too short: {} bytes", path_length);
            return Ok(None);
        }

        // Extract first token (bytes 0-19 of path data)
        let path_start = path_offset + 32;
        if path_start + 20 > params_data.len() || path_start + path_length > params_data.len() {
            info!("🔍 V3 path data out of bounds");
            return Ok(None);
        }

        let token_in_bytes = &params_data[path_start..path_start + 20];
        // Last token is at path_start + path_length - 20
        let token_out_bytes = &params_data[path_start + path_length - 20..path_start + path_length];

        let token_in = Address::from_slice(token_in_bytes);
        let token_out = Address::from_slice(token_out_bytes);

        info!("🔍 V3 exactInput extracted tokens: {} -> {}", token_in, token_out);

        self.find_pool_for_token_pair(token_in, token_out).await
    }

    /// Parse Uniswap V3 exactOutputSingle function
    async fn parse_uniswap_v3_exact_output_single(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 EXACT OUTPUT SINGLE ===");
        // Similar structure to exactInputSingle
        self.parse_uniswap_v3_exact_input_single(params_data).await
    }

    /// Parse Uniswap V3 multicall function
    async fn parse_uniswap_v3_multicall(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 MULTICALL ===");
        info!("📊 Input data length: {} bytes", params_data.len());
        
        // Multicall is complex - it contains multiple function calls
        // For now, just try to extract the first function call
        if params_data.len() < 64 {
            info!("🔍 Insufficient data for V3 multicall: {} bytes", params_data.len());
            return Ok(None);
        }

        // Array offset is at bytes 0-31
        let array_offset = u32::from_be_bytes([
            params_data[28], params_data[29], params_data[30], params_data[31]
        ]) as usize;

        info!("🔍 V3 multicall array offset: {} bytes", array_offset);

        if array_offset >= params_data.len() || array_offset + 64 > params_data.len() {
            info!("🔍 V3 multicall offset out of bounds");
            return Ok(None);
        }

        // Array length
        let array_length = u32::from_be_bytes([
            params_data[array_offset + 28], params_data[array_offset + 29],
            params_data[array_offset + 30], params_data[array_offset + 31]
        ]) as usize;

        info!("🔍 V3 multicall array length: {} calls", array_length);

        if array_length == 0 || array_length > 10 {
            info!("🔍 V3 multicall invalid array length: {}", array_length);
            return Ok(None);
        }

        // Get first call data offset
        let first_call_offset_pos = array_offset + 32;
        if first_call_offset_pos + 32 > params_data.len() {
            info!("🔍 V3 multicall first call offset out of bounds");
            return Ok(None);
        }

        let first_call_offset = array_offset + u32::from_be_bytes([
            params_data[first_call_offset_pos + 28], params_data[first_call_offset_pos + 29],
            params_data[first_call_offset_pos + 30], params_data[first_call_offset_pos + 31]
        ]) as usize;

        info!("🔍 V3 multicall first call offset: {} bytes", first_call_offset);

        if first_call_offset >= params_data.len() || first_call_offset + 36 > params_data.len() {
            info!("🔍 V3 multicall first call data out of bounds");
            return Ok(None);
        }

        // Get first call data length
        let first_call_length = u32::from_be_bytes([
            params_data[first_call_offset + 28], params_data[first_call_offset + 29],
            params_data[first_call_offset + 30], params_data[first_call_offset + 31]
        ]) as usize;

        info!("🔍 V3 multicall first call length: {} bytes", first_call_length);

        if first_call_length < 4 || first_call_offset + 32 + first_call_length > params_data.len() {
            info!("🔍 V3 multicall first call invalid");
            return Ok(None);
        }

        // Extract and analyze the first function call
        let first_call_data = &params_data[first_call_offset + 32..first_call_offset + 32 + first_call_length];
        
        if first_call_data.len() < 4 {
            info!("🔍 V3 multicall first call too short");
            return Ok(None);
        }
        
        let inner_selector = &first_call_data[0..4];
        info!("🔍 V3 multicall first call selector: 0x{}", hex::encode(inner_selector));

        // Handle common inner functions directly instead of recursing
        match inner_selector {
            // exactInputSingle
            [0x41, 0x4b, 0xf3, 0x89] => {
                info!("🔍 V3 multicall contains exactInputSingle");
                self.parse_uniswap_v3_exact_input_single(&first_call_data[4..]).await
            }
            // exactInput
            [0xb8, 0x58, 0x18, 0x3f] => {
                info!("🔍 V3 multicall contains exactInput");
                self.parse_uniswap_v3_exact_input(&first_call_data[4..]).await
            }
            _ => {
                info!("🔍 V3 multicall contains unknown function: 0x{}", hex::encode(inner_selector));
                Ok(None)
            }
        }
    }

    /// Find pool for token pair with enhanced discovery
    async fn find_pool_for_token_pair(
        &self,
        token0: Address,
        token1: Address,
    ) -> Result<Option<Address>> {
        info!(
            "🔍 Searching for pool with tokens {} and {}",
            token0, token1
        );

        // First try the static database lookup
        let pools = self
            .pool_db
            .find_pool_by_tokens(&format!("{:#x}", token0), &format!("{:#x}", token1))?;

        if let Some(pool) = pools.first() {
            let pool_address = pool.address.parse::<Address>()?;
            info!("🎯 Found target pool: {} ({})", pool_address, pool.protocol);
            return Ok(Some(pool_address));
        }

        // If no pool found in static database, try dynamic discovery
        info!("🔍 No pool found in database, attempting dynamic discovery for {} <-> {}", token0, token1);
        
        if let Some(discovered_pool) = self.discover_pool_dynamically(token0, token1).await? {
            info!("✨ Dynamic discovery found pool: {} (UniswapV2)", discovered_pool);
            
            // Add to database for future lookups
            let new_pool = crate::pool_db::DexPool {
                address: format!("{:#x}", discovered_pool),
                protocol: "UniswapV2".to_string(),
                token0: Some(format!("{:#x}", token0)),
                token1: Some(format!("{:#x}", token1)),
                chain_id: 1,
            };
            
            if let Err(e) = self.pool_db.add_pool(&new_pool) {
                debug!("⚠️ Failed to add discovered pool to database: {}", e);
            } else {
                info!("💾 Added discovered pool to database");
            }
            
            return Ok(Some(discovered_pool));
        }

        info!(
            "🔍 No pool found for token pair {} -> {} after all discovery methods",
            token0, token1
        );
        Ok(None)
    }

    /// Discover pool dynamically using CREATE2 address calculation
    async fn discover_pool_dynamically(&self, token0: Address, token1: Address) -> Result<Option<Address>> {
        // Uniswap V2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
        // Init code hash: 0x96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f
        
        let factory = Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")?;
        let init_code_hash = "0x96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f";
        
        // Sort tokens (Uniswap V2 requires token0 < token1)
        let (sorted_token0, sorted_token1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };
        
        // Calculate CREATE2 address
        // address = keccak256(0xff + factory + salt + init_code_hash)[12:]
        // salt = keccak256(abi.encodePacked(token0, token1))
        
        // Create salt: keccak256(token0 + token1)
        let mut salt_input = Vec::new();
        salt_input.extend_from_slice(sorted_token0.as_slice());
        salt_input.extend_from_slice(sorted_token1.as_slice());
        let salt = keccak256(&salt_input);
        
        // Create CREATE2 input: 0xff + factory + salt + init_code_hash
        let mut create2_input = Vec::new();
        create2_input.push(0xff);
        create2_input.extend_from_slice(factory.as_slice());
        create2_input.extend_from_slice(salt.as_slice());
        create2_input.extend_from_slice(&hex::decode(&init_code_hash[2..]).map_err(|e| anyhow::anyhow!("Invalid init code hash: {}", e))?);
        
        let pool_hash = keccak256(&create2_input);
        let pool_address = Address::from_slice(&pool_hash[12..]);
        
        info!("🧮 Calculated pool address for {}/{}: {}", sorted_token0, sorted_token1, pool_address);
        
        // Verify pool exists by checking if it has code
        match self.eth_client.get_code(pool_address).await {
            Ok(code) => {
                if !code.is_empty() && code != "0x" {
                    info!("✅ Pool {} has code - exists on chain", pool_address);
                    Ok(Some(pool_address))
                } else {
                    info!("❌ Pool {} has no code - doesn't exist", pool_address);
                    Ok(None)
                }
            }
            Err(e) => {
                debug!("⚠️ Error checking pool code: {}", e);
                Ok(None)
            }
        }
    }

    /// Run basic sandwich simulation without EnhancedSandwichSimulator
    async fn run_basic_sandwich_simulation(
        &self,
        victim_tx: &MempoolTransaction,
        pool_details: &crate::pool_db::DexPool,
    ) -> Result<Option<crate::enhanced_revm_simulator::EnhancedSandwichResult>> {
        use crate::pool_state_fetcher::PoolStateFetcher;
        
        let sim_id = format!("basic_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
        
        info!("🧮 === BASIC SANDWICH SIMULATION STARTED ===");
        info!("📊 Simulation ID: {}", sim_id);
        info!("🎯 SIMULATION INPUTS:");
        info!("   - Victim TX: {}", victim_tx.hash);
        info!("   - Pool Address: {}", pool_details.address);
        info!("   - Pool Protocol: {}", pool_details.protocol);
        info!("   - Pool Token0: {:?}", pool_details.token0);
        info!("   - Pool Token1: {:?}", pool_details.token1);
        info!("   - Pool Protocol: {}", pool_details.protocol);

        // Create a basic pool state fetcher for simulation
        info!("📡 Creating pool state fetcher...");
        let pool_fetcher = PoolStateFetcher::new(&format!("http://192.168.0.14:8545")).await?;
        let pool_address = pool_details.address.parse::<Address>()?;
        info!("   - Pool Address Parsed: {:#x}", pool_address);

        // Fetch current pool state
        info!("🔍 Fetching current pool state from blockchain...");
        let pool_state = pool_fetcher
            .fetch_pool_state(pool_address, &pool_details.protocol)
            .await?;
            
        info!("📊 FETCHED POOL STATE:");
        info!("   - Pool Address: {}", pool_state.address);
        info!("   - Token0: {}", pool_state.token0);
        info!("   - Token1: {}", pool_state.token1);
        info!("   - Reserve0: {} wei", pool_state.reserve0);
        info!("   - Reserve1: {} wei", pool_state.reserve1);
        info!("   - Protocol: {}", pool_state.protocol);

        // Basic profitability calculation
        info!("💰 Extracting trade amount from victim transaction...");
        let victim_amount = self.extract_trade_amount(victim_tx)?;
        
        info!("🧮 TRADE AMOUNT CALCULATIONS:");
        info!("   - Victim Amount: {} ETH", victim_amount);
        
        // Binary search for optimal frontrun amount
        let mut low_multiplier = 0.1;
        let mut high_multiplier = 10.0;
        let precision = 0.01; // 1% precision
        let mut best_profit = 0.0;
        let mut best_frontrun_amount = 0.0;
        let mut iteration = 0;
        
        info!("🔍 === BINARY SEARCH FOR OPTIMAL FRONTRUN AMOUNT ===");
        
        while (high_multiplier - low_multiplier) > precision && iteration < 10 {
            iteration += 1;
            
            // Test three points: low, mid, high
            let mid_multiplier = (low_multiplier + high_multiplier) / 2.0;
            let test_points = vec![
                (low_multiplier, "low"),
                (mid_multiplier, "mid"), 
                (high_multiplier, "high")
            ];
            
            let mut profits = Vec::new();
            
            for (multiplier, label) in test_points {
                let frontrun_amount = victim_amount * multiplier;
                
                info!("📊 Iteration {}: Testing {} point: {} ETH ({}x victim)", 
                     iteration, label, frontrun_amount, multiplier);
                
                // Simple price impact calculation (basic AMM formula)
                let price_impact = self.calculate_price_impact(&pool_state, victim_amount)?;
                
                // Estimate profit based on price impact
                let estimated_profit = frontrun_amount * price_impact;
                
                // Calculate dynamic gas cost based on current network conditions
                let current_gas_price_gwei = match self.eth_client.get_current_gas_price_wei().await {
                    Ok(gas_price_wei) => gas_price_wei as f64 / 1e9,
                    Err(_) => 30.0, // Fallback to 30 gwei if we can't fetch current price
                };
                
                // Estimate gas usage: frontrun (100k) + victim (200k) + backrun (100k) = 400k total
                let total_gas_units = 400_000.0;
                let gas_cost = total_gas_units * current_gas_price_gwei * 1e-9; // Convert gwei to ETH
                let net_profit = estimated_profit - gas_cost;
                
                info!("   - Price Impact: {:.6}", price_impact);
                info!("   - Estimated Profit: {} ETH", estimated_profit);
                info!("   - Current Gas Price: {:.1} gwei", current_gas_price_gwei);
                info!("   - Gas Cost: {:.6} ETH ({:.0}k gas units)", gas_cost, total_gas_units / 1000.0);
                info!("   - Net Profit: {} ETH", net_profit);
                
                profits.push((multiplier, frontrun_amount, net_profit));
                
                if net_profit > best_profit {
                    best_profit = net_profit;
                    best_frontrun_amount = frontrun_amount;
                }
            }
            
            // Determine which direction to search based on profit curve
            let low_profit = profits[0].2;
            let mid_profit = profits[1].2;
            let high_profit = profits[2].2;
            
            if mid_profit >= low_profit && mid_profit >= high_profit {
                // Peak is around mid, narrow search around it
                let range = (high_multiplier - low_multiplier) / 4.0;
                low_multiplier = (mid_multiplier - range).max(0.1);
                high_multiplier = (mid_multiplier + range).min(10.0);
                info!("🔍 Peak found at mid, narrowing search: [{:.3}, {:.3}]", low_multiplier, high_multiplier);
            } else if low_profit < mid_profit && mid_profit < high_profit {
                // Profit increasing, search upper half
                low_multiplier = mid_multiplier;
                info!("🔍 Profit increasing, searching upper half: [{:.3}, {:.3}]", low_multiplier, high_multiplier);
            } else {
                // Profit decreasing or peak in lower half, search lower half
                high_multiplier = mid_multiplier;
                info!("🔍 Profit decreasing, searching lower half: [{:.3}, {:.3}]", low_multiplier, high_multiplier);
            }
        }
        
        info!("🏆 === BINARY SEARCH COMPLETE ===");
        info!("   - Iterations: {}", iteration);
        info!("   - Final Range: [{:.3}, {:.3}]", low_multiplier, high_multiplier);
        info!("   - Best Frontrun Amount: {} ETH", best_frontrun_amount);
        info!("   - Best Net Profit: {} ETH", best_profit);
        // Only proceed if profitable
        if best_profit <= 0.0 {
            info!("❌ No profitable frontrun amount found (best: {} ETH)", best_profit);
            return Ok(None);
        }
        
        // Calculate final gas cost for result
        let final_gas_price_gwei = match self.eth_client.get_current_gas_price_wei().await {
            Ok(gas_price_wei) => gas_price_wei as f64 / 1e9,
            Err(_) => 30.0, // Fallback to 30 gwei
        };
        let final_total_gas_units = 400_000.0;
        let final_gas_cost = final_total_gas_units * final_gas_price_gwei * 1e-9; // Convert gwei to ETH

        let result = crate::enhanced_revm_simulator::EnhancedSandwichResult {
            pool_address,
            protocol: pool_details.protocol.clone(),
            victim_tx_hash: victim_tx.hash.clone(),
            success: true,
            profit_eth: best_profit + final_gas_cost, // Add back gas cost to get gross profit
            profit_usd: (best_profit + final_gas_cost) * 3200.0, // Assume ETH price
            gas_used: final_total_gas_units as u64,                     // Use calculated gas units
            gas_cost_eth: final_gas_cost,
            net_profit_eth: best_profit,
            net_profit_usd: best_profit * 3200.0,
            price_impact: self.calculate_price_impact(&pool_state, victim_amount)?,
            slippage: self.calculate_price_impact(&pool_state, victim_amount)? * 0.3, // Assume 30% of price impact is slippage
            risk_score: if self.calculate_price_impact(&pool_state, victim_amount)? > 0.05 { 80 } else { 20 }, // High risk if >5% impact
            execution_time_ms: 150,
            frontrun_amount: U256::from((best_frontrun_amount * 1e18) as u64),
            backrun_amount: U256::from(((best_frontrun_amount + best_profit + final_gas_cost) * 1e18) as u64),
            pool_liquidity_before: pool_state.total_liquidity_usd,
            pool_liquidity_after: pool_state.total_liquidity_usd,
            simulation_accuracy: 0.75, // Basic simulation accuracy
        };

        Ok(Some(result))
    }

    /// Extract trade amount from victim transaction by parsing calldata
    fn extract_trade_amount(&self, victim_tx: &MempoolTransaction) -> Result<f64> {
        // First try to extract from ETH value (for ETH swaps)
        let value_wei = victim_tx.value;
        let value_eth = value_wei.to::<u64>() as f64 / 1e18;

        if value_eth > 0.001 {
            // ETH swap - use the ETH value
            info!("💰 Detected ETH swap: {} ETH", value_eth);
            return Ok(value_eth);
        }

        // Try to parse calldata for token swap amounts
        if let Some(parsed_amount) = self.parse_swap_amount_from_calldata(victim_tx) {
            info!("💰 Detected token swap: {} ETH equivalent", parsed_amount);
            return Ok(parsed_amount);
        }

        // Use estimated USD value if available
        if victim_tx.estimated_value_usd > 1.0 {
            let eth_equivalent = victim_tx.estimated_value_usd / 3200.0; // Assume $3200 per ETH
            info!("💰 Using USD estimate: ${} → {} ETH", victim_tx.estimated_value_usd, eth_equivalent);
            return Ok(eth_equivalent);
        }

        // Conservative default for unknown swaps
        let default_amount = 0.05; // Reduced from 0.1 to be more conservative
        info!("⚠️ Using default victim amount: {} ETH (could not parse calldata)", default_amount);
        Ok(default_amount)
    }

    /// Parse swap amount from transaction calldata
    fn parse_swap_amount_from_calldata(&self, victim_tx: &MempoolTransaction) -> Option<f64> {
        let input_data = &victim_tx.input;
        if input_data.len() < 4 {
            info!("🔍 Calldata parsing failed: insufficient data length {}", input_data.len());
            return None;
        }

        // Get function selector (first 4 bytes)
        let selector = &input_data[0..4];
        let selector_hex = format!("0x{:02x}{:02x}{:02x}{:02x}", selector[0], selector[1], selector[2], selector[3]);
        info!("🔍 Parsing calldata for selector: {}", selector_hex);
        
        match selector {
            // swapExactETHForTokens(uint256,address[],address,uint256)
            [0x7f, 0xf3, 0x6a, 0xb5] => {
                info!("🔍 Detected swapExactETHForTokens - checking msg.value");
                // ETH amount should be in msg.value, but let's also try parsing minimum amount out
                if input_data.len() >= 36 {
                    let amount_bytes: [u8; 32] = input_data[4..36].try_into().ok()?;
                    let min_amount_out = U256::from_be_bytes(amount_bytes);
                    let min_out_f64 = min_amount_out.to::<u128>() as f64 / 1e18;
                    info!("🔍 Minimum amount out: {} (as ETH equivalent estimate)", min_out_f64);
                    
                    // Use a conservative estimate based on minimum out (assume ~1:1000 ratio for popular tokens)
                    if min_out_f64 > 1.0 {
                        let estimated_eth = min_out_f64 / 1000.0; // Very rough estimate
                        Some(estimated_eth.min(10.0).max(0.001))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            
            // swapExactTokensForTokens(uint256,uint256,address[],address,uint256)
            [0x38, 0xed, 0x17, 0x39] => {
                info!("🔍 Detected swapExactTokensForTokens");
                if input_data.len() >= 36 {
                    let amount_bytes: [u8; 32] = input_data[4..36].try_into().ok()?;
                    let amount = U256::from_be_bytes(amount_bytes);
                    let amount_f64 = amount.to::<u128>() as f64 / 1e18;
                    info!("🔍 Parsed token amount: {} (assuming 18 decimals)", amount_f64);
                    
                    if amount_f64 > 0.001 && amount_f64 < 1000.0 {
                        Some(amount_f64.min(10.0))
                    } else {
                        info!("🔍 Amount out of range: {}", amount_f64);
                        None
                    }
                } else {
                    info!("🔍 Insufficient data for swapExactTokensForTokens");
                    None
                }
            }
            
            // swapExactTokensForTokensSupportingFeeOnTransferTokens(uint256,uint256,address[],address,uint256)
            [0x79, 0x1a, 0xc9, 0x47] => {
                info!("🔍 Detected swapExactTokensForTokensSupportingFeeOnTransferTokens");
                if input_data.len() >= 36 {
                    let amount_bytes: [u8; 32] = input_data[4..36].try_into().ok()?;
                    let amount = U256::from_be_bytes(amount_bytes);
                    let amount_f64 = amount.to::<u128>() as f64 / 1e18;
                    info!("🔍 Parsed fee-on-transfer token amount: {} (assuming 18 decimals)", amount_f64);
                    
                    if amount_f64 > 0.001 && amount_f64 < 1000.0 {
                        Some(amount_f64.min(10.0))
                    } else {
                        info!("🔍 Amount out of range: {}", amount_f64);
                        None
                    }
                } else {
                    info!("🔍 Insufficient data for swapExactTokensForTokensSupportingFeeOnTransferTokens");
                    None
                }
            }
            
            // swapTokensForExactTokens(uint256,uint256,address[],address,uint256)
            [0x88, 0x03, 0xdb, 0xee] => {
                info!("🔍 Detected swapTokensForExactTokens");
                if input_data.len() >= 68 {
                    let amount_bytes: [u8; 32] = input_data[36..68].try_into().ok()?;
                    let amount = U256::from_be_bytes(amount_bytes);
                    let amount_f64 = amount.to::<u128>() as f64 / 1e18;
                    info!("🔍 Parsed max input amount: {} (assuming 18 decimals)", amount_f64);
                    
                    if amount_f64 > 0.001 && amount_f64 < 1000.0 {
                        Some(amount_f64.min(10.0))
                    } else {
                        info!("🔍 Amount out of range: {}", amount_f64);
                        None
                    }
                } else {
                    info!("🔍 Insufficient data for swapTokensForExactTokens");
                    None
                }
            }
            
            _ => {
                info!("🔍 Unknown function selector: {} - using gas-based estimation", selector_hex);
                // Unknown function - try to estimate based on gas usage
                let gas_limit = victim_tx.gas_limit.to::<u64>() as f64;
                if gas_limit > 200_000.0 {
                    info!("🔍 High gas usage ({}) - estimated large swap: 0.1 ETH", gas_limit);
                    Some(0.1)
                } else {
                    info!("🔍 Low gas usage ({}) - estimated small swap: 0.02 ETH", gas_limit);
                    Some(0.02)
                }
            }
        }
    }

    /// Calculate price impact using basic AMM formula
    fn calculate_price_impact(
        &self,
        pool_state: &crate::sandwich_pool_integration::PoolState,
        trade_amount: f64,
    ) -> Result<f64> {
        // Convert reserves to f64 for calculation (handle overflow safely)
        let reserve0 = if pool_state.reserve0 > U256::from(u64::MAX) {
            // If reserves are too large, use a scaled down version
            (pool_state.reserve0 / U256::from(1e9 as u64)).to::<u64>() as f64 / 1e9
        } else {
            pool_state.reserve0.to::<u64>() as f64 / 1e18
        };

        let reserve1 = if pool_state.reserve1 > U256::from(u64::MAX) {
            // If reserves are too large, use a scaled down version
            (pool_state.reserve1 / U256::from(1e9 as u64)).to::<u64>() as f64 / 1e9
        } else {
            pool_state.reserve1.to::<u64>() as f64 / 1e18
        };

        // Basic constant product formula: x * y = k
        // Price impact = (trade_amount * reserve1) / (reserve0 * (reserve0 + trade_amount))
        let price_impact = (trade_amount * reserve1) / (reserve0 * (reserve0 + trade_amount));

        // Clamp price impact between 0.1% and 10%
        Ok(price_impact.max(0.001).min(0.10))
    }
}
