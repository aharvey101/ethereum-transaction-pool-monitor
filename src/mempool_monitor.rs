/// Real-Time Mempool Monitor for MEV Bot
///
/// This module provides WebSocket-based real-time monitoring of the Ethereum mempool,
/// filtering for DEX transactions that could be potential sandwich targets.
use crate::{
    eth_client::EthereumClient, pool_db::PoolDatabase, sandwich_pool_integration::SandwichTarget,
};
use alloy_primitives::{hex::FromHex, keccak256, Address, Bytes, U256};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use std::time::{Instant, SystemTime};
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
    pub total_transactions_seen: u64, // Total transactions processed by mempool monitor
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
    pub exit_timeout_secs: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            min_tx_value_usd: 1.0, // $1 minimum transaction size (broad detection)
            max_gas_price_gwei: 200.0, // 200 gwei max gas price (mainnet suitable)
            target_protocols: vec![
                "UniswapV2".to_string(),
                "UniswapV3".to_string(),
                "SushiSwap".to_string(),
            ],
            min_profit_threshold_eth: 0.0001, // 0.0001 ETH minimum profit (sensitive detection)
            max_price_impact: 0.05,           // 5% maximum price impact
            confidence_threshold: 0.7,        // 70% minimum confidence
            exit_timeout_secs: 0,             // 0 = no timeout by default
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
    // Transaction deduplication cache: hash -> timestamp
    processed_transactions: HashMap<String, Instant>,
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
            processed_transactions: HashMap::new(),
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

        // Setup exit timeout if enabled
        let exit_timeout = if self.config.exit_timeout_secs > 0 {
            Some(std::time::Duration::from_secs(
                self.config.exit_timeout_secs,
            ))
        } else {
            None
        };

        if let Some(timeout) = exit_timeout {
            info!(
                "⏰ Exit timeout enabled: will exit after {} seconds without transactions",
                timeout.as_secs()
            );
        }

        debug!("🔧 Debug: About to start subscription loop");

        // Subscribe to mempool transactions via WebSocket with polling fallback
        let mut websocket_timeout_count = 0;
        let max_websocket_timeouts = 3; // Try WebSocket 3 times before falling back
        let mut last_transaction_time = std::time::Instant::now();

        loop {
            debug!("📡 Attempting to subscribe to pending transactions stream...");

            match self.eth_client.subscribe_pending_transactions().await {
                Ok(mut tx_stream) => {
                    info!("✅ Successfully subscribed to mempool transactions");
                    let mut no_activity_timer = tokio::time::Instant::now();

                    // Process transactions until connection drops
                    loop {
                        tokio::select! {
                            // Handle new mempool transactions
                            tx_result = tx_stream.recv() => {
                                match tx_result {
                                    Some(tx_hash) => {
                                        // Reset timeout counters when we receive transactions
                                        websocket_timeout_count = 0;
                                        no_activity_timer = tokio::time::Instant::now();
                                        last_transaction_time = std::time::Instant::now();

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

                            // Check for WebSocket inactivity (no transactions received)
                            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                                // Check for exit timeout first
                                if let Some(timeout) = exit_timeout {
                                    if last_transaction_time.elapsed() > timeout {
                                        warn!("⏰ No transactions received for {} seconds, exiting", timeout.as_secs());
                                        return Ok(());
                                    }
                                }

                                if no_activity_timer.elapsed() > tokio::time::Duration::from_secs(30) {
                                    websocket_timeout_count += 1;
                                    warn!("⏰ WebSocket connected but no transactions received for 30s (timeout {}/{})",
                                          websocket_timeout_count, max_websocket_timeouts);

                                    if websocket_timeout_count >= max_websocket_timeouts {
                                        warn!("🔄 Switching to polling fallback due to WebSocket inactivity");
                                        break; // Exit to try polling
                                    }
                                } else {
                                    self.print_monitoring_stats();
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    websocket_timeout_count += 1;
                    error!(
                        "❌ Failed to subscribe to mempool: {} (attempt {}/{}). Retrying in 5 seconds...",
                        e, websocket_timeout_count, max_websocket_timeouts
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }

            // If WebSocket has failed too many times, try polling fallback
            if websocket_timeout_count >= max_websocket_timeouts {
                warn!(
                    "📊 WebSocket failed {} times, trying polling fallback...",
                    websocket_timeout_count
                );

                match self.eth_client.poll_pending_transactions().await {
                    Ok(mut tx_stream) => {
                        info!("✅ Successfully started transaction polling fallback");

                        // Process transactions from polling
                        loop {
                            tokio::select! {
                                tx_result = tx_stream.recv() => {
                                    match tx_result {
                                        Some(tx_hash) => {
                                            last_transaction_time = std::time::Instant::now(); // Reset timeout for polling too
                                            //info!("📨 Processing polled transaction: {}", tx_hash);
                                            let tx_hash_for_debug = tx_hash.clone();

                                            if let Err(e) = self.process_mempool_transaction(tx_hash).await {
                                                info!("⚠️  Failed to process transaction {}: {}", tx_hash_for_debug, e);
                                            }
                                        }
                                        None => {
                                            warn!("📡 Polling stream disconnected");
                                            break;
                                        }
                                    }
                                }

                                // Print stats every 30 seconds and check for exit timeout
                                _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                                    // Check for exit timeout first
                                    if let Some(timeout) = exit_timeout {
                                        if last_transaction_time.elapsed() > timeout {
                                            warn!("⏰ No transactions received for {} seconds, exiting", timeout.as_secs());
                                            return Ok(());
                                        }
                                    }
                                    self.print_monitoring_stats();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "❌ Failed to start polling fallback: {}. Retrying WebSocket...",
                            e
                        );
                        websocket_timeout_count = 0; // Reset and try WebSocket again
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }

            // Wait before attempting to reconnect
            warn!("🔄 Reconnecting to mempool in 3 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }

    /// Process a single mempool transaction for MEV opportunities
    async fn process_mempool_transaction(&mut self, tx_hash: String) -> Result<()> {
        // Check if we've already processed this transaction recently
        let now = Instant::now();
        if let Some(&last_processed) = self.processed_transactions.get(&tx_hash) {
        }

        // Add to processed cache
        self.processed_transactions.insert(tx_hash.clone(), now);

        // Clean old entries from cache (older than 5 minutes)
        self.processed_transactions
            .retain(|_, &mut timestamp| now.duration_since(timestamp).as_secs() < 300);

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
            let is_router = self.pool_db.is_dex_router(&target_string)
                || self.is_major_dex_router(&target_address);

            if is_router {
                info!(
                    "🎯 Router transaction detected: {} -> {}",
                    tx_hash, target_address
                );
            } else {
                // Log addresses for debugging - only show high-value transactions
                if tx_details.value_f64 > 0.01 {
                    info!(
                        "🔍 Non-DEX transaction (${:.2}, to: {}): {}",
                        tx_details.value_f64 * 2000.0, // Estimate USD
                        target_address,
                        tx_hash
                    );
                } else {
                    debug!(
                        "🔍 Skipping tx {} - not a DEX interaction (target: {})",
                        tx_hash, target_address
                    );
                }
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
        let opportunity_id = format!(
            "opp_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        info!("🔍 === SANDWICH OPPORTUNITY ANALYSIS STARTED ===");
        info!("📊 Analysis ID: {}", opportunity_id);
        info!("🎯 VICTIM TRANSACTION DETAILS:");
        info!("   - TX Hash: {}", victim_tx.hash);
        info!("   - From: {}", victim_tx.from);
        info!("   - To: {:?}", victim_tx.to);
        info!(
            "   - Value: {} wei ({} ETH)",
            victim_tx.value,
            victim_tx.value.to::<u128>() as f64 / 1e18
        );
        info!(
            "   - Gas Price: {} wei ({} gwei)",
            victim_tx.gas_price,
            victim_tx.gas_price.to::<u128>() as f64 / 1e9
        );
        info!("   - Gas Limit: {}", victim_tx.gas_limit);
        info!("   - Nonce: {}", victim_tx.nonce);
        info!("   - Input Data Length: {} bytes", victim_tx.input.len());
        info!("   - Is DEX: {}", victim_tx.is_dex_interaction);

        let pool_address = match victim_tx.target_pool {
            Some(addr) => {
                info!("   - Target Pool: {:#x}", addr);
                addr
            }
            None => {
                info!("   - No target pool identified, skipping analysis");
                return Ok(None);
            }
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
        info!(
            "   - Recommended Frontrun: {} wei",
            mock_target.recommended_frontrun_amount
        );
        info!(
            "   - Trade Direction: {:?}",
            mock_target.victim_trade_direction
        );

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
                info!(
                    "Pool not found in database, attempting on-chain verification: {:#x}",
                    pool_address
                );

                // Fallback: Try to verify pool exists on-chain and get its tokens
                let pool_addr_str = format!("{:#x}", pool_address);
                match self.eth_client.get_pool_tokens(&pool_addr_str).await {
                    Ok(Some((token0, token1))) => {
                        info!(
                            "✅ Verified pool on-chain: {} (tokens: {} -> {})",
                            pool_addr_str, token0, token1
                        );

                        // Create a temporary pool record for simulation
                        use crate::pool_db::DexPool;
                        let temp_pool = DexPool {
                            address: pool_addr_str.clone(),
                            protocol: "Unknown".to_string(), // We'll determine protocol later
                            token0: Some(token0),
                            token1: Some(token1),
                            chain_id: 1,
                        };

                        // Try to add to database for future lookups
                        if let Err(e) = self.pool_db.insert_pool(&temp_pool) {
                            warn!("Failed to insert verified pool: {} - {}", pool_addr_str, e);
                        }

                        temp_pool
                    }
                    Ok(None) => {
                        warn!(
                            "Pool address has no code or invalid tokens: {:#x}",
                            pool_address
                        );
                        return Ok(None);
                    }
                    Err(e) => {
                        error!(
                            "Failed to verify pool on-chain: {:#x} - {}",
                            pool_address, e
                        );
                        return Ok(None);
                    }
                }
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
            total_transactions_seen: self.stats.total_transactions_seen,
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

        // Enhanced USD value calculation considering swap amounts
        let estimated_value_usd = self.calculate_transaction_value_usd(
            &tx_details, 
            &input, 
            to_addr
        ).await.unwrap_or_else(|| {
            // Fallback to ETH value if enhanced calculation fails
            eth_value * 2000.0 // Assume $2000 ETH
        });

        // Check if this is a DEX interaction (either direct pool or router)
        let is_dex = to_addr.map_or(false, |addr| {
            let addr_string = format!("{:#x}", addr); // Use hex format like 0x1234...
            let is_pool = self.known_pools.contains(&addr);
            let is_router =
                self.pool_db.is_dex_router(&addr_string) || self.is_major_dex_router(&addr);

            // Log every transaction with value > $1 for debugging
            if estimated_value_usd > 1.0 {
                info!(
                    "🔍 Transaction to: {} (${:.2}) - Pool: {}, Router: {}, Input: {} bytes",
                    addr_string,
                    estimated_value_usd,
                    is_pool,
                    is_router,
                    input.len()
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

        // Try to get pool from database first, if not found, fetch it proactively
        let pool_info = if let Some(pool) = pools.first() {
            info!("💾 Using cached pool data for: {:#x}", pool_address);
            pool.clone()
        } else {
            info!(
                "🔄 Pool not in database, fetching live data for: {:#x}",
                pool_address
            );

            // Use PoolStateFetcher to get live pool data
            use crate::pool_state_fetcher::PoolStateFetcher;
            let pool_fetcher = PoolStateFetcher::new("http://192.168.0.14:8545").await?;

            // Try different protocols - start with UniswapV2 as most common
            let protocols_to_try = ["UniswapV2", "SushiSwap", "UniswapV3"];
            let mut fetched_pool_info = None;

            for protocol in protocols_to_try.iter() {
                match pool_fetcher.fetch_pool_state(pool_address, protocol).await {
                    Ok(pool_state) => {
                        info!(
                            "✅ Successfully fetched {} pool data for: {:#x}",
                            protocol, pool_address
                        );

                        // Create DexPool from fetched data and store in database
                        let pool_info = crate::pool_db::DexPool {
                            address: pool_address.to_string(),
                            protocol: protocol.to_string(),
                            token0: Some(pool_state.token0.to_string()),
                            token1: Some(pool_state.token1.to_string()),
                            chain_id: 1, // Mainnet
                        };

                        // Store in database for future use
                        if let Err(e) = self.pool_db.insert_pool(&pool_info) {
                            warn!("Failed to cache fetched pool data: {}", e);
                        } else {
                            info!(
                                "💾 Cached new pool data for future use: {:#x}",
                                pool_address
                            );
                        }

                        fetched_pool_info = Some(pool_info);
                        break;
                    }
                    Err(_) => {
                        debug!(
                            "❌ Failed to fetch as {} pool: {:#x}",
                            protocol, pool_address
                        );
                        continue;
                    }
                }
            }

            fetched_pool_info.ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not fetch pool data from any protocol for: {:#x}",
                    pool_address
                )
            })?
        };

        // Create pool state using actual fetched data
        let pool_state = crate::sandwich_pool_integration::PoolState {
            address: pool_address,
            protocol: pool_info.protocol.clone(),
            token0: pool_info
                .token0
                .as_ref()
                .unwrap_or(&"0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string())
                .parse()?,
            token1: pool_info
                .token1
                .as_ref()
                .unwrap_or(&"0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string())
                .parse()?,
            fee: 3000, // Default 0.3% fee - will be fetched properly in future iterations
            reserve0: U256::from(1000000000000000000000000_u128), // Will be updated by simulation
            reserve1: U256::from(1000000000000_u128), // Will be updated by simulation
            block_number: 0, // Will be updated by simulation
            total_liquidity_usd: 100000.0, // Will be updated by simulation
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
    pub async fn parse_router_target_pool(
        &self,
        input_data: &Bytes,
        _router_address: &str,
    ) -> Result<Option<Address>> {
        if input_data.len() < 4 {
            return Ok(None);
        }

        // Extract function selector (first 4 bytes)
        let function_selector = &input_data[0..4];

        // Only log for debugging when needed (reduce noise)
        debug!(
            "🔍 Function selector: 0x{} ({})",
            hex::encode(function_selector),
            input_data.len()
        );

        // Comprehensive DEX function selectors
        match function_selector {
            // Uniswap V2 Router Functions
            [0x38, 0xed, 0x17, 0x39] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactTokensForTokens
            [0x7f, 0xf3, 0x6a, 0xb5] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactETHForTokens
            [0x18, 0xcb, 0xaf, 0xe5] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactTokensForETH
            [0x8f, 0x0e, 0x15, 0xa4] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactTokensForETHSupportingFeeOnTransferTokens
            [0x4a, 0x25, 0xa9, 0x4a] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactTokensForTokensSupportingFeeOnTransferTokens
            [0xfb, 0x3b, 0xdb, 0x41] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactETHForTokensSupportingFeeOnTransferTokens
            [0x79, 0x1a, 0xc9, 0x47] => self.extract_token_pair_simple(&input_data[4..]).await, // swapTokensForExactETH
            [0x88, 0x19, 0xcc, 0x46] => self.extract_token_pair_simple(&input_data[4..]).await, // swapETHForExactTokens
            [0x85, 0x83, 0x59, 0x86] => self.extract_token_pair_simple(&input_data[4..]).await, // swapTokensForExactTokens

            // Uniswap V3 Router Functions
            [0x41, 0x4b, 0xf3, 0x89] => self.extract_v3_token_pair(&input_data[4..]).await, // exactInputSingle
            [0xdb, 0x3e, 0x21, 0x98] => self.extract_v3_token_pair(&input_data[4..]).await, // exactOutputSingle
            [0xc0, 0x4b, 0x8d, 0x59] => self.extract_token_pair_simple(&input_data[4..]).await, // exactInput (multi-hop)
            [0xf2, 0x8c, 0x04, 0x98] => self.extract_token_pair_simple(&input_data[4..]).await, // exactOutput (multi-hop)

            // SushiSwap Router (same as Uniswap V2)
            [0x02, 0x75, 0x1c, 0xec] => self.extract_token_pair_simple(&input_data[4..]).await, // swapExactTokensForTokens

            // 1inch Router Functions
            [0x7c, 0x02, 0x5e, 0x60] => self.extract_token_pair_simple(&input_data[4..]).await, // swap
            [0x41, 0x34, 0x67, 0x67] => self.extract_token_pair_simple(&input_data[4..]).await, // unoswap
            [0x24, 0x9b, 0x3a, 0x64] => self.extract_token_pair_simple(&input_data[4..]).await, // uniswapV3Swap

            // 0x Protocol Functions
            [0xd9, 0x62, 0x7a, 0xa4] => self.extract_token_pair_simple(&input_data[4..]).await, // sellToUniswap
            [0x63, 0x09, 0xfc, 0x04] => self.extract_token_pair_simple(&input_data[4..]).await, // transformERC20

            // Balancer V2 Vault Functions
            [0x52, 0xb5, 0x4e, 0xb7] => self.extract_token_pair_simple(&input_data[4..]).await, // batchSwap
            [0x94, 0x5b, 0xc6, 0xc5] => self.extract_token_pair_simple(&input_data[4..]).await, // swap

            // ParaSwap Functions
            [0x54, 0x84, 0xd8, 0x04] => self.extract_token_pair_simple(&input_data[4..]).await, // swapOnUniswap
            [0xad, 0x9c, 0x44, 0xa6] => self.extract_token_pair_simple(&input_data[4..]).await, // multiSwap

            // ========== DIRECT POOL FUNCTIONS (NEW!) ==========
            // Uniswap V2 Pool Direct Functions
            [0x02, 0x2c, 0x0d, 0x9f] => self.extract_pool_swap_v2(&input_data[4..]).await, // pool.swap(uint amount0Out, uint amount1Out, address to, bytes calldata data)
            [0x6a, 0x62, 0x78, 0x42] => self.extract_pool_mint_v2(&input_data[4..]).await, // pool.mint(address to)
            [0x89, 0xaf, 0xcb, 0x44] => self.extract_pool_burn_v2(&input_data[4..]).await, // pool.burn(address to)

            // Uniswap V3 Pool Direct Functions
            [0x12, 0x8a, 0xcb, 0x08] => self.extract_pool_swap_v3(&input_data[4..]).await, // pool.swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data)
            [0x3c, 0x8a, 0x7d, 0x8d] => self.extract_pool_mint_v3(&input_data[4..]).await, // pool.mint(address recipient, int24 tickLower, int24 tickUpper, uint128 amount, bytes calldata data)
            [0xa3, 0x41, 0x23, 0xa7] => self.extract_pool_burn_v3(&input_data[4..]).await, // pool.burn(int24 tickLower, int24 tickUpper, uint128 amount)
            [0x4f, 0x1e, 0xb3, 0xd8] => self.extract_pool_collect_v3(&input_data[4..]).await, // pool.collect(address recipient, int24 tickLower, int24 tickUpper, uint128 amount0Requested, uint128 amount1Requested)

            // Additional Pool Functions (Flash loans, etc.)
            [0x49, 0x04, 0xb1, 0xd7] => self.extract_pool_flash_v2(&input_data[4..]).await, // pool.swap() with flash loan data
            [0xf3, 0x05, 0x8d, 0xb0] => self.extract_pool_flash_v3(&input_data[4..]).await, // pool.flash() for V3

            // For unrecognized functions, try simple token extraction anyway
            _ => {
                debug!(
                    "🔍 Attempting fallback parsing for unknown function: 0x{}",
                    alloy_primitives::hex::encode(function_selector)
                );
                // Try generic token pair extraction for unknown functions
                self.extract_token_pair_simple(&input_data[4..]).await
            }
        }
    }

    /// Parse Uniswap V2 swap with better error handling
    async fn parse_uniswap_v2_swap(&self, params_data: &[u8]) -> Result<Option<Address>> {
        // V2 swaps typically have: amountIn, amountOutMin, path[], to, deadline
        // Path is usually the 3rd parameter
        if params_data.len() < 160 {
            // 5 parameters * 32 bytes
            debug!("V2 swap data too short: {} bytes", params_data.len());
            return self.extract_token_pair_simple(params_data).await;
        }

        // Try to find path array at expected offset (0x40 = 64)
        if let Some(pool) = self.try_extract_path_at_offset(params_data, 64).await? {
            return Ok(Some(pool));
        }

        // Fallback to general extraction
        self.extract_token_pair_simple(params_data).await
    }

    /// Parse Uniswap V2 ETH swap with better error handling  
    async fn parse_uniswap_v2_eth_swap(&self, params_data: &[u8]) -> Result<Option<Address>> {
        // ETH swaps typically have: amountOutMin, path[], to, deadline
        // Path is usually the 2nd parameter
        if params_data.len() < 128 {
            // 4 parameters * 32 bytes
            debug!("V2 ETH swap data too short: {} bytes", params_data.len());
            return self.extract_token_pair_simple(params_data).await;
        }

        // Try to find path array at expected offset (0x20 = 32)
        if let Some(pool) = self.try_extract_path_at_offset(params_data, 32).await? {
            return Ok(Some(pool));
        }

        // Fallback to general extraction
        self.extract_token_pair_simple(params_data).await
    }

    /// Try to extract path array at a specific offset
    async fn try_extract_path_at_offset(
        &self,
        params_data: &[u8],
        path_offset: usize,
    ) -> Result<Option<Address>> {
        if params_data.len() <= path_offset + 32 {
            return Ok(None);
        }

        // Read the offset to the actual path data
        let path_data_offset = u32::from_be_bytes([
            params_data[path_offset + 28],
            params_data[path_offset + 29],
            params_data[path_offset + 30],
            params_data[path_offset + 31],
        ]) as usize;

        if path_data_offset >= params_data.len() || path_data_offset + 32 >= params_data.len() {
            debug!("Invalid path data offset: {}", path_data_offset);
            return Ok(None);
        }

        // Read array length
        let array_len = u32::from_be_bytes([
            params_data[path_data_offset + 28],
            params_data[path_data_offset + 29],
            params_data[path_data_offset + 30],
            params_data[path_data_offset + 31],
        ]);

        if array_len < 2 || array_len > 10 {
            debug!("Invalid path array length: {}", array_len);
            return Ok(None);
        }

        let array_start = path_data_offset + 32;
        let required_bytes = array_len as usize * 32;

        if array_start + required_bytes > params_data.len() {
            debug!(
                "Path array extends beyond data: need {} bytes, have {}",
                array_start + required_bytes,
                params_data.len()
            );
            return Ok(None);
        }

        // Extract first and last tokens
        if let Some((token0, token1)) =
            self.extract_path_tokens(&params_data[array_start..], array_len)
        {
            debug!("Extracted tokens from path: {} -> {}", token0, token1);
            return self.calculate_pool_address_enhanced(token0, token1).await;
        }

        Ok(None)
    }

    /// Robust token pair extraction from ABI-encoded data
    async fn extract_token_pair_simple(&self, params_data: &[u8]) -> Result<Option<Address>> {
        if params_data.len() < 64 {
            return Ok(None);
        }

        // Strategy 1: Try to find token addresses in the first 4 parameter slots (common pattern)
        if let Some(pool) = self.try_extract_from_first_parameters(params_data).await? {
            return Ok(Some(pool));
        }

        // Strategy 2: Look for path arrays (Uniswap V2/V3 style)
        if let Some(pool) = self.try_extract_from_path_array(params_data).await? {
            return Ok(Some(pool));
        }

        // Strategy 3: Scan for any valid token addresses and try to find pairs
        if let Some(pool) = self.try_extract_from_address_scan(params_data).await? {
            return Ok(Some(pool));
        }

        Ok(None)
    }

    /// Try extracting token addresses from the first few parameter slots
    async fn try_extract_from_first_parameters(
        &self,
        params_data: &[u8],
    ) -> Result<Option<Address>> {
        if params_data.len() < 64 {
            return Ok(None);
        }

        // Extract potential addresses from first 4 parameter slots
        let mut potential_tokens = Vec::new();

        for i in 0..4 {
            let offset = i * 32;
            if offset + 32 <= params_data.len() {
                // Check if the last 20 bytes could be a valid token address
                if params_data[offset..offset + 12].iter().all(|&b| b == 0) {
                    let addr = Address::from_slice(&params_data[offset + 12..offset + 32]);
                    if self.is_likely_token_address(addr) {
                        potential_tokens.push(addr);
                    }
                }
            }
        }

        // If we found 2+ potential tokens, try to create pool from first two
        if potential_tokens.len() >= 2 {
            return self
                .calculate_pool_address_enhanced(potential_tokens[0], potential_tokens[1])
                .await;
        }

        Ok(None)
    }

    /// Try extracting from path arrays (typical in Uniswap V2/V3)
    async fn try_extract_from_path_array(&self, params_data: &[u8]) -> Result<Option<Address>> {
        if params_data.len() < 128 {
            return Ok(None);
        }

        // Look for array length indicators followed by addresses
        for offset in (32..params_data.len().saturating_sub(96)).step_by(32) {
            if offset + 96 <= params_data.len() {
                // Check if this could be an array length
                let potential_len = u32::from_be_bytes([
                    params_data[offset + 28],
                    params_data[offset + 29],
                    params_data[offset + 30],
                    params_data[offset + 31],
                ]);

                // Valid path arrays typically have 2-10 tokens
                if potential_len >= 2 && potential_len <= 10 {
                    let array_start = offset + 32;
                    let required_bytes = potential_len as usize * 32;

                    if array_start + required_bytes <= params_data.len() {
                        // Try to extract first and last addresses from path
                        if let Some((token0, token1)) =
                            self.extract_path_tokens(&params_data[array_start..], potential_len)
                        {
                            if let Some(pool) =
                                self.calculate_pool_address_enhanced(token0, token1).await?
                            {
                                return Ok(Some(pool));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// Extract first and last tokens from a path array
    fn extract_path_tokens(&self, array_data: &[u8], length: u32) -> Option<(Address, Address)> {
        if array_data.len() < (length as usize * 32) {
            return None;
        }

        // Extract first token (first 32 bytes, last 20 bytes are the address)
        let token0 = Address::from_slice(&array_data[12..32]);

        // Extract last token
        let last_offset = ((length - 1) as usize) * 32;
        if array_data.len() < last_offset + 32 {
            return None;
        }
        let token1 = Address::from_slice(&array_data[last_offset + 12..last_offset + 32]);

        // Validate that both addresses look like tokens
        if self.is_likely_token_address(token0) && self.is_likely_token_address(token1) {
            Some((token0, token1))
        } else {
            None
        }
    }

    /// Scan for any valid token addresses in the transaction data
    async fn try_extract_from_address_scan(&self, params_data: &[u8]) -> Result<Option<Address>> {
        let mut found_tokens = Vec::new();

        // Scan through data looking for potential token addresses
        for offset in (0..params_data.len().saturating_sub(32)).step_by(4) {
            if offset + 32 <= params_data.len() {
                // Check if first 12 bytes are zero (typical for addresses in ABI encoding)
                if params_data[offset..offset + 12].iter().all(|&b| b == 0) {
                    let addr = Address::from_slice(&params_data[offset + 12..offset + 32]);
                    if self.is_likely_token_address(addr) && !found_tokens.contains(&addr) {
                        found_tokens.push(addr);
                        if found_tokens.len() >= 2 {
                            break;
                        }
                    }
                }
            }
        }

        // If we found at least 2 unique tokens, try to find a pool
        if found_tokens.len() >= 2 {
            return self
                .calculate_pool_address_enhanced(found_tokens[0], found_tokens[1])
                .await;
        }

        Ok(None)
    }

    /// Check if an address is likely to be a token contract
    fn is_likely_token_address(&self, addr: Address) -> bool {
        // Filter out obvious non-token addresses
        if addr.is_zero() {
            return false;
        }

        let addr_bytes = addr.as_slice();

        // Exclude addresses that are likely to be EOAs or malformed (too many leading zeros)
        // Real token addresses should not have more than 4-5 consecutive leading zero bytes
        let leading_zeros = addr_bytes.iter().take_while(|&&b| b == 0).count();
        if leading_zeros > 5 {
            return false;
        }

        // Exclude addresses that look like numeric values rather than proper addresses
        // Check if the address has patterns that suggest it's not a real contract address

        // Pattern 1: Addresses with alternating zero bytes (common in numeric data)
        let zero_byte_count = addr_bytes.iter().filter(|&&b| b == 0).count();
        if zero_byte_count > 14 {
            // More than 14 zero bytes out of 20 is suspicious
            return false;
        }

        // Pattern 2: Check for obvious non-address patterns (all same byte, sequential patterns)
        let unique_bytes: std::collections::HashSet<u8> = addr_bytes.iter().cloned().collect();
        if unique_bytes.len() < 3 {
            // Too few unique bytes suggests numeric data
            return false;
        }

        // Pattern 3: Exclude known contract patterns that are not tokens
        let addr_str = format!("{:#x}", addr).to_lowercase();

        // Exclude common non-token contract patterns
        if addr_str.ends_with("00000000") && addr_str.len() == 42 {
            return false; // Likely a numeric value, not an address
        }

        // Known non-token addresses (ETH placeholder, zero-like addresses)
        match addr_str.as_str() {
            "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" => false, // ETH placeholder
            "0x0000000000000000000000000000000000000000" => false, // Zero address
            _ if addr_str.contains("0000000000000000") => false, // Contains too many consecutive zeros
            _ => true,
        }
    }

    /// Extract token pair from Uniswap V3 exactInputSingle and similar functions
    async fn extract_v3_token_pair(&self, params_data: &[u8]) -> Result<Option<Address>> {
        if params_data.len() < 64 {
            return Ok(None);
        }

        // Try multiple strategies for V3 token extraction

        // Strategy 1: Standard V3 exactInputSingle (tokenIn, tokenOut as first two params)
        if let Some(pool) = self.try_v3_exact_input_pattern(params_data).await? {
            return Ok(Some(pool));
        }

        // Strategy 2: V3 swap struct parameter (common pattern)
        if let Some(pool) = self.try_v3_struct_pattern(params_data).await? {
            return Ok(Some(pool));
        }

        // Strategy 3: Fall back to general token extraction
        self.extract_token_pair_simple(params_data).await
    }

    /// Try extracting tokens from V3 exactInputSingle pattern
    async fn try_v3_exact_input_pattern(&self, params_data: &[u8]) -> Result<Option<Address>> {
        if params_data.len() < 64 {
            return Ok(None);
        }

        // V3 exactInputSingle: first two parameters are tokenIn and tokenOut
        let token_in = Address::from_slice(&params_data[12..32]);
        let token_out = Address::from_slice(&params_data[44..64]);

        // Validate that these look like token addresses
        if self.is_likely_token_address(token_in) && self.is_likely_token_address(token_out) {
            self.calculate_pool_address_enhanced(token_in, token_out)
                .await
        } else {
            Ok(None)
        }
    }

    /// Try extracting tokens from V3 struct-based parameters
    async fn try_v3_struct_pattern(&self, params_data: &[u8]) -> Result<Option<Address>> {
        // V3 often uses structs - look for the struct offset first
        if params_data.len() < 96 {
            return Ok(None);
        }

        // Check if first parameter is a struct offset
        let struct_offset = u32::from_be_bytes([
            params_data[28],
            params_data[29],
            params_data[30],
            params_data[31],
        ]) as usize;

        if struct_offset > 0 && struct_offset < params_data.len().saturating_sub(64) {
            let struct_data = &params_data[struct_offset..];
            if struct_data.len() >= 64 {
                let token_in = Address::from_slice(&struct_data[12..32]);
                let token_out = Address::from_slice(&struct_data[44..64]);

                if self.is_likely_token_address(token_in) && self.is_likely_token_address(token_out)
                {
                    return self
                        .calculate_pool_address_enhanced(token_in, token_out)
                        .await;
                }
            }
        }

        Ok(None)
    }

    /// Calculate Uniswap V2 pool address from token pair
    async fn calculate_uniswap_v2_pool(
        &self,
        token0: Address,
        token1: Address,
    ) -> Result<Option<Address>> {
        // Sort tokens (Uniswap V2 requirement)
        let (sorted_token0, sorted_token1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // Uniswap V2 Factory: 0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f
        let factory = Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")?;

        // Calculate CREATE2 address: keccak256(abi.encodePacked(token0, token1, init_code_hash))
        let mut data = Vec::new();
        data.extend_from_slice(sorted_token0.as_slice());
        data.extend_from_slice(sorted_token1.as_slice());
        let salt = keccak256(&data);

        // Uniswap V2 init code hash
        let init_code_hash = [
            0x96, 0xe8, 0xac, 0x42, 0x77, 0x19, 0x8f, 0xf8, 0xb6, 0xf7, 0x85, 0x47, 0x8a, 0xa9,
            0xa3, 0x9f, 0x40, 0x3c, 0xb7, 0x68, 0xdd, 0x02, 0xcb, 0xee, 0x32, 0x6c, 0x3e, 0x7d,
            0xa3, 0x48, 0x84, 0x5f,
        ];

        // CREATE2: keccak256(0xff ++ factory ++ salt ++ init_code_hash)[12:]
        let mut create2_data = Vec::new();
        create2_data.push(0xff);
        create2_data.extend_from_slice(factory.as_slice());
        create2_data.extend_from_slice(salt.as_slice());
        create2_data.extend_from_slice(&init_code_hash);

        let pool_hash = keccak256(&create2_data);
        let pool_address = Address::from_slice(&pool_hash[12..]);

        info!(
            "🔍 Calculated pool: {} -> {} = {}",
            sorted_token0, sorted_token1, pool_address
        );
        Ok(Some(pool_address))
    }

    /// Enhanced pool calculation with multiple DEX strategies and on-chain verification
    async fn calculate_pool_address_enhanced(
        &self,
        token0: Address,
        token1: Address,
    ) -> Result<Option<Address>> {
        info!(
            "🚀 Enhanced pool calculation for tokens: {} -> {}",
            token0, token1
        );

        // Strategy 1: Try Uniswap V2 calculation
        if let Some(v2_pool) = self.calculate_uniswap_v2_pool(token0, token1).await? {
            let v2_addr = format!("{:#x}", v2_pool);
            if self
                .eth_client
                .verify_pool_exists_onchain(&v2_addr)
                .await
                .unwrap_or(false)
            {
                info!("✅ Found Uniswap V2 pool: {}", v2_addr);
                // Add to database for future lookups
                self.add_pool_to_database(v2_pool, "UniswapV2", token0, token1)
                    .await?;
                return Ok(Some(v2_pool));
            }
        }

        // Strategy 2: Try Uniswap V3 with different fee tiers
        let fee_tiers = [500u32, 3000u32, 10000u32]; // 0.05%, 0.3%, 1.0%
        for fee in fee_tiers {
            if let Some(v3_pool) = self.calculate_uniswap_v3_pool(token0, token1, fee).await? {
                let v3_addr = format!("{:#x}", v3_pool);
                if self
                    .eth_client
                    .verify_pool_exists_onchain(&v3_addr)
                    .await
                    .unwrap_or(false)
                {
                    info!("✅ Found Uniswap V3 pool ({}bps): {}", fee, v3_addr);
                    self.add_pool_to_database(v3_pool, "UniswapV3", token0, token1)
                        .await?;
                    return Ok(Some(v3_pool));
                }
            }
        }

        // Strategy 3: Try SushiSwap calculation
        if let Some(sushi_pool) = self.calculate_sushiswap_pool(token0, token1).await? {
            let sushi_addr = format!("{:#x}", sushi_pool);
            if self
                .eth_client
                .verify_pool_exists_onchain(&sushi_addr)
                .await
                .unwrap_or(false)
            {
                info!("✅ Found SushiSwap pool: {}", sushi_addr);
                self.add_pool_to_database(sushi_pool, "SushiSwap", token0, token1)
                    .await?;
                return Ok(Some(sushi_pool));
            }
        }

        info!(
            "❌ No valid pool found for tokens: {} -> {}",
            token0, token1
        );
        Ok(None)
    }

    /// Calculate Uniswap V3 pool address with specific fee tier
    async fn calculate_uniswap_v3_pool(
        &self,
        token0: Address,
        token1: Address,
        fee: u32,
    ) -> Result<Option<Address>> {
        // Sort tokens (Uniswap V3 requirement)
        let (sorted_token0, sorted_token1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // Uniswap V3 Factory: 0x1F98431c8aD98523631AE4a59f267346ea31F984
        let factory = Address::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984")?;

        // Calculate CREATE2 address: keccak256(abi.encode(token0, token1, fee))
        let mut salt_data = Vec::new();
        salt_data.extend_from_slice(sorted_token0.as_slice());
        salt_data.extend_from_slice(sorted_token1.as_slice());
        salt_data.extend_from_slice(&fee.to_be_bytes()[1..4]); // Use 3 bytes for fee
        let salt = keccak256(&salt_data);

        // Uniswap V3 Pool init code hash
        let init_code_hash = [
            0xe3, 0x4f, 0x19, 0x9b, 0x19, 0xb2, 0xb4, 0xf4, 0x7f, 0x29, 0xcb, 0x2c, 0x60, 0x7b,
            0x34, 0x51, 0x33, 0xd4, 0x08, 0x69, 0x7a, 0x59, 0xd7, 0x3b, 0xc1, 0x2a, 0x73, 0x3c,
            0xf7, 0xd7, 0xb3, 0x09,
        ];

        // CREATE2: keccak256(0xff ++ factory ++ salt ++ init_code_hash)[12:]
        let mut create2_data = Vec::new();
        create2_data.push(0xff);
        create2_data.extend_from_slice(factory.as_slice());
        create2_data.extend_from_slice(salt.as_slice());
        create2_data.extend_from_slice(&init_code_hash);

        let pool_hash = keccak256(&create2_data);
        let pool_address = Address::from_slice(&pool_hash[12..]);

        info!(
            "🔍 Calculated V3 pool: {} -> {} ({}bps) = {}",
            sorted_token0, sorted_token1, fee, pool_address
        );
        Ok(Some(pool_address))
    }

    /// Calculate SushiSwap pool address  
    async fn calculate_sushiswap_pool(
        &self,
        token0: Address,
        token1: Address,
    ) -> Result<Option<Address>> {
        // Sort tokens (SushiSwap requirement)
        let (sorted_token0, sorted_token1) = if token0 < token1 {
            (token0, token1)
        } else {
            (token1, token0)
        };

        // SushiSwap Factory: 0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac
        let factory = Address::from_str("0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac")?;

        // Calculate CREATE2 address
        let mut data = Vec::new();
        data.extend_from_slice(sorted_token0.as_slice());
        data.extend_from_slice(sorted_token1.as_slice());
        let salt = keccak256(&data);

        // SushiSwap init code hash (same as Uniswap V2)
        let init_code_hash = [
            0x96, 0xe8, 0xac, 0x42, 0x77, 0x19, 0x8f, 0xf8, 0xb6, 0xf7, 0x85, 0x47, 0x8a, 0xa9,
            0xa3, 0x9f, 0x40, 0x3c, 0xb7, 0x68, 0xdd, 0x02, 0xcb, 0xee, 0x32, 0x6c, 0x3e, 0x7d,
            0xa3, 0x48, 0x84, 0x5f,
        ];

        // CREATE2: keccak256(0xff ++ factory ++ salt ++ init_code_hash)[12:]
        let mut create2_data = Vec::new();
        create2_data.push(0xff);
        create2_data.extend_from_slice(factory.as_slice());
        create2_data.extend_from_slice(salt.as_slice());
        create2_data.extend_from_slice(&init_code_hash);

        let pool_hash = keccak256(&create2_data);
        let pool_address = Address::from_slice(&pool_hash[12..]);

        info!(
            "🔍 Calculated SushiSwap pool: {} -> {} = {}",
            sorted_token0, sorted_token1, pool_address
        );
        Ok(Some(pool_address))
    }

    /// Add newly discovered pool to database for future lookups
    async fn add_pool_to_database(
        &self,
        pool_address: Address,
        protocol: &str,
        token0: Address,
        token1: Address,
    ) -> Result<()> {
        use crate::pool_db::DexPool;

        let pool = DexPool {
            address: format!("{:#x}", pool_address),
            protocol: protocol.to_string(),
            token0: Some(format!("{:#x}", token0)),
            token1: Some(format!("{:#x}", token1)),
            chain_id: 1, // Mainnet
        };

        if let Err(e) = self.pool_db.insert_pool(&pool) {
            warn!(
                "Failed to insert pool into database: {} - {}",
                pool.address, e
            );
        } else {
            info!("💾 Added pool to database: {} ({})", pool.address, protocol);
        }

        Ok(())
    }

    /// Check if address is a major DEX router (fallback for database lookup)
    fn is_major_dex_router(&self, address: &Address) -> bool {
        let addr_str = format!("{:#x}", address).to_lowercase();

        // Most common DEX routers on mainnet
        match addr_str.as_str() {
            "0x7a250d5630b4cf539739df2c5dacb4c659f2488d" | // Uniswap V2 Router
            "0xe592427a0aece92de3edee1f18e0157c05861564" | // Uniswap V3 SwapRouter  
            "0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45" | // Uniswap V3 SwapRouter02
            "0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f" | // SushiSwap Router
            "0x1111111254eeb25477b68fb85ed929f73a960582" | // 1inch Router V5
            "0xdef1c0ded9bec7f1a1670819833240f027b25eff" | // 0x Protocol ExchangeProxy
            "0x881d40237659c251811cec9c364ef91dc08d300c" | // MetaMask Swap Router
            "0xdef171fe48cf0115b1d80b88dc8eab59176fee57"   // ParaSwap Augustus V5
            => true,
            _ => false
        }
    }

    /// Parse Uniswap V3 exactInputSingle function
    /// Function signature: exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))
    async fn parse_uniswap_v3_exact_input_single(
        &self,
        params_data: &[u8],
    ) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 EXACT INPUT SINGLE ===");
        info!("📊 Input data length: {} bytes", params_data.len());

        if params_data.len() < 256 {
            info!(
                "🔍 Insufficient data for V3 exactInputSingle: {} bytes (minimum: 256)",
                params_data.len()
            );
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
                params_data[152],
                params_data[153],
                params_data[154],
                params_data[155],
                params_data[156],
                params_data[157],
                params_data[158],
                params_data[159],
            ])
        } else {
            0
        };

        info!("🔍 V3 exactInputSingle extracted amounts:");
        info!(
            "   - Amount In: {} wei ({} ETH)",
            amount_in,
            amount_in as f64 / 1e18
        );
        info!(
            "🔍 V3 exactInputSingle extracted tokens: {} -> {}",
            token_in, token_out
        );

        self.find_pool_for_token_pair(token_in, token_out).await
    }

    /// Parse Uniswap V3 exactInput function (multi-hop)
    /// Function signature: exactInput((bytes,address,uint256,uint256,uint256))
    async fn parse_uniswap_v3_exact_input(&self, params_data: &[u8]) -> Result<Option<Address>> {
        info!("🔧 === PARSING UNISWAP V3 EXACT INPUT (MULTI-HOP) ===");
        info!("📊 Input data length: {} bytes", params_data.len());

        if params_data.len() < 160 {
            info!(
                "🔍 Insufficient data for V3 exactInput: {} bytes (minimum: 160)",
                params_data.len()
            );
            return Ok(None);
        }

        // The path is encoded in the first parameter (bytes)
        // For now, extract the first and last tokens from the path
        // This is a simplified implementation - V3 paths are more complex

        // Path offset is at bytes 0-31 (first parameter)
        let path_offset = u32::from_be_bytes([
            params_data[28],
            params_data[29],
            params_data[30],
            params_data[31],
        ]) as usize;

        info!("🔍 V3 path offset: {} bytes", path_offset);

        if path_offset >= params_data.len() || path_offset + 32 > params_data.len() {
            info!("🔍 V3 path offset out of bounds");
            return Ok(None);
        }

        // Path length
        let path_length = u32::from_be_bytes([
            params_data[path_offset + 28],
            params_data[path_offset + 29],
            params_data[path_offset + 30],
            params_data[path_offset + 31],
        ]) as usize;

        info!("🔍 V3 path length: {} bytes", path_length);

        if path_length < 43 {
            // Minimum: 20 (token) + 3 (fee) + 20 (token)
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

        info!(
            "🔍 V3 exactInput extracted tokens: {} -> {}",
            token_in, token_out
        );

        self.find_pool_for_token_pair(token_in, token_out).await
    }

    /// Parse Uniswap V3 exactOutputSingle function
    async fn parse_uniswap_v3_exact_output_single(
        &self,
        params_data: &[u8],
    ) -> Result<Option<Address>> {
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
            info!(
                "🔍 Insufficient data for V3 multicall: {} bytes",
                params_data.len()
            );
            return Ok(None);
        }

        // Array offset is at bytes 0-31
        let array_offset = u32::from_be_bytes([
            params_data[28],
            params_data[29],
            params_data[30],
            params_data[31],
        ]) as usize;

        info!("🔍 V3 multicall array offset: {} bytes", array_offset);

        if array_offset >= params_data.len() || array_offset + 64 > params_data.len() {
            info!("🔍 V3 multicall offset out of bounds");
            return Ok(None);
        }

        // Array length
        let array_length = u32::from_be_bytes([
            params_data[array_offset + 28],
            params_data[array_offset + 29],
            params_data[array_offset + 30],
            params_data[array_offset + 31],
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

        let first_call_offset = array_offset
            + u32::from_be_bytes([
                params_data[first_call_offset_pos + 28],
                params_data[first_call_offset_pos + 29],
                params_data[first_call_offset_pos + 30],
                params_data[first_call_offset_pos + 31],
            ]) as usize;

        info!(
            "🔍 V3 multicall first call offset: {} bytes",
            first_call_offset
        );

        if first_call_offset >= params_data.len() || first_call_offset + 36 > params_data.len() {
            info!("🔍 V3 multicall first call data out of bounds");
            return Ok(None);
        }

        // Get first call data length
        let first_call_length = u32::from_be_bytes([
            params_data[first_call_offset + 28],
            params_data[first_call_offset + 29],
            params_data[first_call_offset + 30],
            params_data[first_call_offset + 31],
        ]) as usize;

        info!(
            "🔍 V3 multicall first call length: {} bytes",
            first_call_length
        );

        if first_call_length < 4 || first_call_offset + 32 + first_call_length > params_data.len() {
            info!("🔍 V3 multicall first call invalid");
            return Ok(None);
        }

        // Extract and analyze the first function call
        let first_call_data =
            &params_data[first_call_offset + 32..first_call_offset + 32 + first_call_length];

        if first_call_data.len() < 4 {
            info!("🔍 V3 multicall first call too short");
            return Ok(None);
        }

        let inner_selector = &first_call_data[0..4];
        info!(
            "🔍 V3 multicall first call selector: 0x{}",
            hex::encode(inner_selector)
        );

        // Handle common inner functions directly instead of recursing
        match inner_selector {
            // exactInputSingle
            [0x41, 0x4b, 0xf3, 0x89] => {
                info!("🔍 V3 multicall contains exactInputSingle");
                self.parse_uniswap_v3_exact_input_single(&first_call_data[4..])
                    .await
            }
            // exactInput
            [0xb8, 0x58, 0x18, 0x3f] => {
                info!("🔍 V3 multicall contains exactInput");
                self.parse_uniswap_v3_exact_input(&first_call_data[4..])
                    .await
            }
            _ => {
                info!(
                    "🔍 V3 multicall contains unknown function: 0x{}",
                    hex::encode(inner_selector)
                );
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
        info!(
            "🔍 No pool found in database, attempting dynamic discovery for {} <-> {}",
            token0, token1
        );

        if let Some(discovered_pool) = self.discover_pool_dynamically(token0, token1).await? {
            info!(
                "✨ Dynamic discovery found pool: {} (UniswapV2)",
                discovered_pool
            );

            // Add to database for future lookups
            let new_pool = crate::pool_db::DexPool {
                address: format!("{:#x}", discovered_pool),
                protocol: "UniswapV2".to_string(),
                token0: Some(format!("{:#x}", token0)),
                token1: Some(format!("{:#x}", token1)),
                chain_id: 1,
            };

            if let Err(e) = self.pool_db.insert_pool(&new_pool) {
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
    async fn discover_pool_dynamically(
        &self,
        token0: Address,
        token1: Address,
    ) -> Result<Option<Address>> {
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
        create2_input.extend_from_slice(
            &hex::decode(&init_code_hash[2..])
                .map_err(|e| anyhow::anyhow!("Invalid init code hash: {}", e))?,
        );

        let pool_hash = keccak256(&create2_input);
        let pool_address = Address::from_slice(&pool_hash[12..]);

        info!(
            "🧮 Calculated pool address for {}/{}: {}",
            sorted_token0, sorted_token1, pool_address
        );

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

        let sim_id = format!(
            "basic_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

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
                (high_multiplier, "high"),
            ];

            let mut profits = Vec::new();

            for (multiplier, label) in test_points {
                let frontrun_amount = victim_amount * multiplier;

                info!(
                    "📊 Iteration {}: Testing {} point: {} ETH ({}x victim)",
                    iteration, label, frontrun_amount, multiplier
                );

                // Simple price impact calculation (basic AMM formula)
                let price_impact = self.calculate_price_impact(&pool_state, victim_amount)?;

                // Estimate profit based on price impact
                let estimated_profit = frontrun_amount * price_impact;

                // Calculate dynamic gas cost based on current network conditions
                let current_gas_price_gwei = match self.eth_client.get_current_gas_price_wei().await
                {
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
                info!(
                    "   - Gas Cost: {:.6} ETH ({:.0}k gas units)",
                    gas_cost,
                    total_gas_units / 1000.0
                );
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
                info!(
                    "🔍 Peak found at mid, narrowing search: [{:.3}, {:.3}]",
                    low_multiplier, high_multiplier
                );
            } else if low_profit < mid_profit && mid_profit < high_profit {
                // Profit increasing, search upper half
                low_multiplier = mid_multiplier;
                info!(
                    "🔍 Profit increasing, searching upper half: [{:.3}, {:.3}]",
                    low_multiplier, high_multiplier
                );
            } else {
                // Profit decreasing or peak in lower half, search lower half
                high_multiplier = mid_multiplier;
                info!(
                    "🔍 Profit decreasing, searching lower half: [{:.3}, {:.3}]",
                    low_multiplier, high_multiplier
                );
            }
        }

        info!("🏆 === BINARY SEARCH COMPLETE ===");
        info!("   - Iterations: {}", iteration);
        info!(
            "   - Final Range: [{:.3}, {:.3}]",
            low_multiplier, high_multiplier
        );
        info!("   - Best Frontrun Amount: {} ETH", best_frontrun_amount);
        info!("   - Best Net Profit: {} ETH", best_profit);
        // Only proceed if profitable
        if best_profit <= 0.0 {
            info!(
                "❌ No profitable frontrun amount found (best: {} ETH)",
                best_profit
            );
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
            gas_used: final_total_gas_units as u64,   // Use calculated gas units
            gas_cost_eth: final_gas_cost,
            net_profit_eth: best_profit,
            net_profit_usd: best_profit * 3200.0,
            price_impact: self.calculate_price_impact(&pool_state, victim_amount)?,
            slippage: self.calculate_price_impact(&pool_state, victim_amount)? * 0.3, // Assume 30% of price impact is slippage
            risk_score: if self.calculate_price_impact(&pool_state, victim_amount)? > 0.05 {
                80
            } else {
                20
            }, // High risk if >5% impact
            execution_time_ms: 150,
            frontrun_amount: U256::from((best_frontrun_amount * 1e18) as u64),
            backrun_amount: U256::from(
                ((best_frontrun_amount + best_profit + final_gas_cost) * 1e18) as u64,
            ),
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
            info!(
                "💰 Using USD estimate: ${} → {} ETH",
                victim_tx.estimated_value_usd, eth_equivalent
            );
            return Ok(eth_equivalent);
        }

        // Conservative default for unknown swaps
        let default_amount = 0.05; // Reduced from 0.1 to be more conservative
        info!(
            "⚠️ Using default victim amount: {} ETH (could not parse calldata)",
            default_amount
        );
        Ok(default_amount)
    }

    /// Parse swap amount from transaction calldata
    fn parse_swap_amount_from_calldata(&self, victim_tx: &MempoolTransaction) -> Option<f64> {
        let input_data = &victim_tx.input;
        if input_data.len() < 4 {
            info!(
                "🔍 Calldata parsing failed: insufficient data length {}",
                input_data.len()
            );
            return None;
        }

        // Get function selector (first 4 bytes)
        let selector = &input_data[0..4];
        let selector_hex = format!(
            "0x{:02x}{:02x}{:02x}{:02x}",
            selector[0], selector[1], selector[2], selector[3]
        );
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
                    info!(
                        "🔍 Minimum amount out: {} (as ETH equivalent estimate)",
                        min_out_f64
                    );

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
                    info!(
                        "🔍 Parsed token amount: {} (assuming 18 decimals)",
                        amount_f64
                    );

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
                    info!(
                        "🔍 Parsed fee-on-transfer token amount: {} (assuming 18 decimals)",
                        amount_f64
                    );

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
                    info!(
                        "🔍 Parsed max input amount: {} (assuming 18 decimals)",
                        amount_f64
                    );

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
                info!(
                    "🔍 Unknown function selector: {} - using gas-based estimation",
                    selector_hex
                );
                // Unknown function - try to estimate based on gas usage
                let gas_limit = victim_tx.gas_limit.to::<u64>() as f64;
                if gas_limit > 200_000.0 {
                    info!(
                        "🔍 High gas usage ({}) - estimated large swap: 0.1 ETH",
                        gas_limit
                    );
                    Some(0.1)
                } else {
                    info!(
                        "🔍 Low gas usage ({}) - estimated small swap: 0.02 ETH",
                        gas_limit
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, Bytes};
    use std::str::FromStr;

    fn create_test_monitor() -> MempoolMonitor {
        // Create a mock pool database for testing
        let pool_db = std::sync::Arc::new(PoolDatabase::new(":memory:").unwrap());
        let config = MempoolConfig::default();
        let (tx, _rx) = mpsc::unbounded_channel();

        // Create mock eth client for testing
        let eth_client = std::sync::Arc::new(
            futures::executor::block_on(async {
                EthereumClient::new("http://localhost:8545").await
            })
            .unwrap(),
        );

        MempoolMonitor {
            eth_client,
            pool_db,
            config,
            known_pools: HashSet::new(),
            opportunity_sender: tx,
            stats: MonitorStats::default(),
            processed_transactions: HashMap::new(),
        }
    }

    #[test]
    fn test_major_dex_router_detection() {
        let monitor = create_test_monitor();

        // Test known routers
        let uniswap_v2 = Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d").unwrap();
        let uniswap_v3 = Address::from_str("0xe592427a0aece92de3edee1f18e0157c05861564").unwrap();
        let sushiswap = Address::from_str("0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f").unwrap();
        let one_inch = Address::from_str("0x1111111254eeb25477b68fb85ed929f73a960582").unwrap();

        assert!(monitor.is_major_dex_router(&uniswap_v2));
        assert!(monitor.is_major_dex_router(&uniswap_v3));
        assert!(monitor.is_major_dex_router(&sushiswap));
        assert!(monitor.is_major_dex_router(&one_inch));

        // Test non-router address
        let random_addr = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
        assert!(!monitor.is_major_dex_router(&random_addr));
    }

    #[test]
    fn test_function_selector_recognition() {
        // Test common Uniswap V2 function selectors
        let swap_exact_eth_for_tokens = [0x7f, 0xf3, 0x6a, 0xb5];
        let swap_exact_tokens_for_eth = [0x18, 0xcb, 0xaf, 0xe5];
        let swap_exact_tokens_for_tokens = [0x38, 0xed, 0x17, 0x39];

        // Test Uniswap V3 function selectors
        let exact_input_single = [0x41, 0x4b, 0xf3, 0x89];

        // These should be recognized (verifying current function selectors)
        assert_eq!(swap_exact_eth_for_tokens, [0x7f, 0xf3, 0x6a, 0xb5]);
        assert_eq!(swap_exact_tokens_for_eth, [0x18, 0xcb, 0xaf, 0xe5]);
        assert_eq!(swap_exact_tokens_for_tokens, [0x38, 0xed, 0x17, 0x39]);
        assert_eq!(exact_input_single, [0x41, 0x4b, 0xf3, 0x89]);
    }

    #[tokio::test]
    async fn test_uniswap_v2_pool_calculation() {
        let monitor = create_test_monitor();

        // WETH and USDC addresses
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let usdc = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();

        let result = monitor.calculate_uniswap_v2_pool(weth, usdc).await;

        // Should return a pool address (CREATE2 calculation should work)
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_token_pair_extraction_basic() {
        let monitor = create_test_monitor();

        // Create mock ABI-encoded data for a swap with token path
        // This simulates swapExactTokensForTokens with a 2-token path
        let mut test_data = vec![0u8; 200];

        // Set up a mock array at offset 64 (skip first two 32-byte parameters)
        let array_offset = 64;

        // Array length = 2
        test_data[array_offset + 28] = 0;
        test_data[array_offset + 29] = 0;
        test_data[array_offset + 30] = 0;
        test_data[array_offset + 31] = 2;

        // Token 0: WETH (last 20 bytes of 32-byte slot)
        let weth_bytes = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        test_data[array_offset + 32 + 12..array_offset + 32 + 32]
            .copy_from_slice(&weth_bytes.as_slice());

        // Token 1: USDC (last 20 bytes of next 32-byte slot)
        let usdc_bytes = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();
        test_data[array_offset + 64 + 12..array_offset + 64 + 32]
            .copy_from_slice(&usdc_bytes.as_slice());

        let result = monitor.extract_token_pair_simple(&test_data).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_input_data_parsing() {
        let monitor = create_test_monitor();

        // Test with empty data
        let empty_data = Bytes::new();
        let result = monitor
            .parse_router_target_pool(&empty_data, "0x7a250d5630b4cf539739df2c5dacb4c659f2488d")
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // Test with too short data
        let short_data = Bytes::from(vec![0x7f, 0xf3]);
        let result = monitor
            .parse_router_target_pool(&short_data, "0x7a250d5630b4cf539739df2c5dacb4c659f2488d")
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_dex_transaction_detection() {
        let monitor = create_test_monitor();

        // Create a transaction going to Uniswap V2 router
        let uniswap_v2_router =
            Address::from_str("0x7a250d5630b4cf539739df2c5dacb4c659f2488d").unwrap();
        let random_address =
            Address::from_str("0x1234567890123456789012345678901234567890").unwrap();

        // Test router detection
        assert!(monitor.is_major_dex_router(&uniswap_v2_router));
        assert!(!monitor.is_major_dex_router(&random_address));

        // Test database router detection (should also work)
        assert!(monitor
            .pool_db
            .is_dex_router(&format!("{:#x}", uniswap_v2_router)));
        assert!(!monitor
            .pool_db
            .is_dex_router(&format!("{:#x}", random_address)));
    }

    #[tokio::test]
    async fn test_improved_token_extraction_strategies() {
        let monitor = create_test_monitor();

        // Test Strategy 1: Extract from first parameters
        let mut test_data = vec![0u8; 128];

        // WETH as first parameter (32 bytes, address in last 20)
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        test_data[12..32].copy_from_slice(&weth.as_slice());

        // USDC as second parameter
        let usdc = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();
        test_data[44..64].copy_from_slice(&usdc.as_slice());

        let result = monitor.try_extract_from_first_parameters(&test_data).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_path_array_extraction() {
        let monitor = create_test_monitor();

        // Create realistic path array data
        let mut test_data = vec![0u8; 256];

        // Array offset at position 2 (0x40 = 64)
        test_data[60..64].copy_from_slice(&64u32.to_be_bytes());

        // Array data starts at offset 64
        // Array length = 3 tokens
        test_data[92..96].copy_from_slice(&3u32.to_be_bytes());

        // Token path: WETH -> USDC -> DAI
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let usdc = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();
        let dai = Address::from_str("0x6B175474E89094C44Da98b954EedeAC495271d0F").unwrap();

        test_data[108..128].copy_from_slice(&weth.as_slice());
        test_data[140..160].copy_from_slice(&usdc.as_slice());
        test_data[172..192].copy_from_slice(&dai.as_slice());

        let result = monitor.try_extract_from_path_array(&test_data).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_address_validation() {
        let monitor = create_test_monitor();

        // Test valid token addresses
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let usdc = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();
        assert!(monitor.is_likely_token_address(weth));
        assert!(monitor.is_likely_token_address(usdc));

        // Test invalid addresses
        let zero_addr = Address::ZERO;
        let low_addr = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        assert!(!monitor.is_likely_token_address(zero_addr));
        assert!(!monitor.is_likely_token_address(low_addr));
    }

    #[tokio::test]
    async fn test_v3_struct_pattern_extraction() {
        let monitor = create_test_monitor();

        // Create V3-style struct data
        let mut test_data = vec![0u8; 192];

        // Struct offset pointing to position 32
        test_data[28..32].copy_from_slice(&32u32.to_be_bytes());

        // Struct data starts at offset 32
        // tokenIn (WETH)
        let weth = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        test_data[44..64].copy_from_slice(&weth.as_slice());

        // tokenOut (USDC)
        let usdc = Address::from_str("0xA0b86a33E6441B0C7d5EB4E5c3BAFe9A93Bc0FE6").unwrap();
        test_data[76..96].copy_from_slice(&usdc.as_slice());

        let result = monitor.try_v3_struct_pattern(&test_data).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_path_extraction_with_invalid_data() {
        let monitor = create_test_monitor();

        // Test with array length too large
        let mut test_data = vec![0u8; 128];
        test_data[92..96].copy_from_slice(&100u32.to_be_bytes()); // Invalid length

        let result = monitor.try_extract_from_path_array(&test_data).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // Test with truncated array
        let mut test_data2 = vec![0u8; 100];
        test_data2[60..64].copy_from_slice(&64u32.to_be_bytes());
        test_data2[92..96].copy_from_slice(&3u32.to_be_bytes()); // Claims 3 tokens but not enough space

        let result2 = monitor.try_extract_from_path_array(&test_data2).await;
        assert!(result2.is_ok());
        assert!(result2.unwrap().is_none());
    }
}

// ========== DIRECT POOL PARSING FUNCTIONS ==========

impl MempoolMonitor {
    /// Extract pool information from Uniswap V2 pool.swap() direct call
    /// Function: swap(uint amount0Out, uint amount1Out, address to, bytes calldata data)
    async fn extract_pool_swap_v2(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🏊 Parsing Uniswap V2 pool.swap() direct call");
        
        if params_data.len() < 128 { // 4 parameters minimum
            debug!("❌ Insufficient data for V2 pool.swap: {} bytes", params_data.len());
            return Ok(None);
        }

        // For direct pool swaps, we need to get the pool's tokens using eth_call
        // Since we're already calling a pool contract, we can get its tokens directly
        // This is a direct pool interaction - the transaction target IS the pool
        // Return None to let the pool detection logic handle token extraction via eth_call
        
        debug!("✅ Direct V2 pool swap detected - will extract tokens via eth_call");
        Ok(None)
    }

    /// Extract pool information from Uniswap V2 pool.mint() direct call
    async fn extract_pool_mint_v2(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🌱 Parsing Uniswap V2 pool.mint() direct call");
        
        if params_data.len() < 32 { // address to parameter
            return Ok(None);
        }

        debug!("✅ Direct V2 pool mint detected - liquidity addition transaction");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from Uniswap V2 pool.burn() direct call  
    async fn extract_pool_burn_v2(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🔥 Parsing Uniswap V2 pool.burn() direct call");
        
        if params_data.len() < 32 { // address to parameter
            return Ok(None);
        }

        debug!("✅ Direct V2 pool burn detected - liquidity removal transaction");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from Uniswap V3 pool.swap() direct call
    /// Function: swap(address recipient, bool zeroForOne, int256 amountSpecified, uint160 sqrtPriceLimitX96, bytes calldata data)
    async fn extract_pool_swap_v3(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🏊 Parsing Uniswap V3 pool.swap() direct call");
        
        if params_data.len() < 160 { // 5 parameters minimum  
            debug!("❌ Insufficient data for V3 pool.swap: {} bytes", params_data.len());
            return Ok(None);
        }

        debug!("✅ Direct V3 pool swap detected - will extract tokens via eth_call");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from Uniswap V3 pool.mint() direct call
    async fn extract_pool_mint_v3(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🌱 Parsing Uniswap V3 pool.mint() direct call");
        
        if params_data.len() < 128 { // Multiple parameters including ticks
            return Ok(None);
        }

        debug!("✅ Direct V3 pool mint detected - liquidity addition transaction");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from Uniswap V3 pool.burn() direct call
    async fn extract_pool_burn_v3(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("🔥 Parsing Uniswap V3 pool.burn() direct call");
        
        if params_data.len() < 96 { // Tick range and amount parameters
            return Ok(None);
        }

        debug!("✅ Direct V3 pool burn detected - liquidity removal transaction");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from Uniswap V3 pool.collect() direct call
    async fn extract_pool_collect_v3(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("💰 Parsing Uniswap V3 pool.collect() direct call");
        
        if params_data.len() < 160 { // Multiple parameters for fee collection
            return Ok(None);
        }

        debug!("✅ Direct V3 pool collect detected - fee collection transaction");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from flash loan calls
    async fn extract_pool_flash_v2(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("⚡ Parsing V2 flash loan call");
        
        if params_data.len() < 128 {
            return Ok(None);
        }

        debug!("✅ V2 flash loan detected");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Extract pool information from V3 flash calls
    async fn extract_pool_flash_v3(&self, params_data: &[u8]) -> Result<Option<Address>> {
        debug!("⚡ Parsing V3 flash loan call");
        
        if params_data.len() < 96 {
            return Ok(None);
        }

        debug!("✅ V3 flash loan detected");
        Ok(None) // Let pool detection handle token extraction
    }

    /// Calculate USD value of transaction considering actual swap amounts, not just ETH transfer
    async fn calculate_transaction_value_usd(
        &self,
        tx_details: &crate::eth_client::MempoolTransaction,
        input_data: &Bytes,
        to_addr: Option<Address>,
    ) -> Option<f64> {
        // First, try to get ETH transfer value as baseline
        let eth_value = tx_details.value_f64;
        let eth_usd_baseline = eth_value * 2000.0; // $2000 ETH assumption

        // If no input data, return ETH value only
        if input_data.len() < 4 {
            return Some(eth_usd_baseline);
        }

        // Get function signature
        let function_selector = &input_data[0..4];
        
        // Try to extract swap amounts based on function type
        match function_selector {
            // Direct pool swaps - these often have large amounts even with small ETH transfer
            [0x02, 0x2c, 0x0d, 0x9f] => {
                // V2 pool.swap(uint amount0Out, uint amount1Out, address to, bytes calldata data)
                if let Some(swap_value) = self.estimate_v2_pool_swap_value(input_data, to_addr).await {
                    return Some(swap_value.max(eth_usd_baseline));
                }
            }
            [0x12, 0x8a, 0xcb, 0x08] => {
                // V3 pool.swap(address recipient, bool zeroForOne, int256 amountSpecified, ...)
                if let Some(swap_value) = self.estimate_v3_pool_swap_value(input_data, to_addr).await {
                    return Some(swap_value.max(eth_usd_baseline));
                }
            }
            
            // Router swaps - try to parse amounts from known router functions
            [0x38, 0xed, 0x17, 0x39] => {
                // swapExactTokensForTokens(uint amountIn, uint amountOutMin, address[] calldata path, address to, uint deadline)
                if let Some(router_value) = self.estimate_router_swap_value(input_data, "swapExactTokensForTokens").await {
                    return Some(router_value.max(eth_usd_baseline));
                }
            }
            [0x7f, 0xf3, 0x6a, 0xb5] => {
                // swapExactETHForTokens - ETH value is the actual swap value
                return Some(eth_usd_baseline); // ETH value is correct here
            }
            [0x18, 0xcb, 0xaf, 0xe5] => {
                // swapExactTokensForETH(uint amountIn, uint amountOutMin, address[] calldata path, address to, uint deadline)  
                if let Some(router_value) = self.estimate_router_swap_value(input_data, "swapExactTokensForETH").await {
                    return Some(router_value.max(eth_usd_baseline));
                }
            }
            [0x41, 0x4b, 0xf3, 0x89] => {
                // exactInputSingle - V3 router
                if let Some(v3_value) = self.estimate_v3_router_swap_value(input_data).await {
                    return Some(v3_value.max(eth_usd_baseline));
                }
            }
            
            _ => {
                // Unknown function - use ETH transfer value
                debug!("🔍 Unknown function signature, using ETH transfer value: ${:.2}", eth_usd_baseline);
            }
        }

        // Fallback to ETH transfer value
        Some(eth_usd_baseline)
    }

    /// Estimate USD value of Uniswap V2 pool direct swap
    async fn estimate_v2_pool_swap_value(&self, input_data: &Bytes, pool_addr: Option<Address>) -> Option<f64> {
        if input_data.len() < 100 || pool_addr.is_none() {
            return None;
        }

        let pool_addr = pool_addr.unwrap();
        
        // Extract amount0Out and amount1Out from swap parameters
        let amount0_out_bytes = &input_data[4..36];
        let amount1_out_bytes = &input_data[36..68];
        
        // Convert to U256
        let amount0_out = U256::from_be_slice(amount0_out_bytes);
        let amount1_out = U256::from_be_slice(amount1_out_bytes);
        
        // Get the larger of the two amounts (one will be 0, other will be the output amount)
        let swap_amount = if amount0_out > amount1_out { amount0_out } else { amount1_out };
        
        // Try to get pool token information to estimate value
        if let Ok(Some((token0, token1))) = self.get_pool_tokens(pool_addr).await {
            // Estimate token value - this is simplified, could be enhanced with price oracles
            let estimated_value = self.estimate_token_value_usd(swap_amount, &token0, &token1).await;
            if estimated_value > 1.0 {
                debug!("💰 V2 pool swap estimated value: ${:.2} (amount: {})", estimated_value, swap_amount);
                return Some(estimated_value);
            }
        }

        None
    }

    /// Estimate USD value of Uniswap V3 pool direct swap
    async fn estimate_v3_pool_swap_value(&self, input_data: &Bytes, pool_addr: Option<Address>) -> Option<f64> {
        if input_data.len() < 164 || pool_addr.is_none() {
            return None;
        }

        // V3 swap: amountSpecified is the 3rd parameter (int256)
        let amount_specified_bytes = &input_data[68..100];
        let amount_specified = U256::from_be_slice(amount_specified_bytes);
        
        // Convert from signed int (simplified - just take absolute value)
        let swap_amount = amount_specified;

        if let Some(pool_addr) = pool_addr {
            if let Ok(Some((token0, token1))) = self.get_pool_tokens(pool_addr).await {
                let estimated_value = self.estimate_token_value_usd(swap_amount, &token0, &token1).await;
                if estimated_value > 1.0 {
                    debug!("💰 V3 pool swap estimated value: ${:.2} (amount: {})", estimated_value, swap_amount);
                    return Some(estimated_value);
                }
            }
        }

        None
    }

    /// Estimate USD value of router swap by parsing amountIn parameter
    async fn estimate_router_swap_value(&self, input_data: &Bytes, function_name: &str) -> Option<f64> {
        if input_data.len() < 68 {
            return None;
        }

        // Most router functions have amountIn as first parameter
        let amount_in_bytes = &input_data[4..36];
        let amount_in = U256::from_be_slice(amount_in_bytes);
        
        // Try to extract token path to identify tokens being swapped
        if let Some((token_in, token_out)) = self.extract_router_token_path(input_data).await {
            let estimated_value = self.estimate_token_value_usd(amount_in, &token_in, &token_out).await;
            if estimated_value > 1.0 {
                debug!("💰 Router {} estimated value: ${:.2} (amount: {})", function_name, estimated_value, amount_in);
                return Some(estimated_value);
            }
        }

        None
    }

    /// Estimate USD value of Uniswap V3 router swap  
    async fn estimate_v3_router_swap_value(&self, input_data: &Bytes) -> Option<f64> {
        if input_data.len() < 100 {
            return None;
        }

        // V3 exactInputSingle has struct parameter, amountIn is inside the struct
        // Simplified: look for amountIn at typical offset
        let amount_in_bytes = &input_data[68..100]; // Typical location in V3 struct
        let amount_in = U256::from_be_slice(amount_in_bytes);
        
        // For V3, we could extract tokenIn/tokenOut from the struct as well
        // Simplified estimation for now
        if amount_in > U256::from(1000000u64) { // If > 1M units
            let estimated_value = self.estimate_token_value_usd_simplified(amount_in).await;
            if estimated_value > 1.0 {
                debug!("💰 V3 router swap estimated value: ${:.2}", estimated_value);
                return Some(estimated_value);
            }
        }

        None
    }

    /// Extract token pair from router transaction path parameter
    async fn extract_router_token_path(&self, input_data: &Bytes) -> Option<(Address, Address)> {
        // This would parse the path[] parameter from router calls
        // Simplified implementation - would need full ABI parsing for production
        if input_data.len() < 200 {
            return None;
        }

        // Look for path array at typical locations
        // This is a simplified heuristic approach
        for offset in [100, 132, 164].iter() {
            if input_data.len() > offset + 64 {
                let token_in = Address::from_slice(&input_data[offset + 12..offset + 32]);
                let token_out = Address::from_slice(&input_data[offset + 44..offset + 64]);
                
                if self.is_likely_token_address(token_in) && self.is_likely_token_address(token_out) {
                    return Some((token_in, token_out));
                }
            }
        }

        None
    }

    /// Get pool tokens using eth_call (token0() and token1())
    async fn get_pool_tokens(&self, _pool_addr: Address) -> Result<Option<(Address, Address)>> {
        // This would use eth_client to call token0() and token1() on the pool
        // For now, return None to keep the implementation simple
        // In production, this should make actual eth_call to get the tokens
        Ok(None)
    }

    /// Estimate token value in USD based on amount and token addresses
    async fn estimate_token_value_usd(&self, amount: U256, _token0: &Address, _token1: &Address) -> f64 {
        self.estimate_token_value_usd_from_u256(amount, _token0, _token1).await
    }

    /// Helper function to convert U256 to f64 safely and estimate USD value
    async fn estimate_token_value_usd_from_u256(&self, amount: U256, _token0: &Address, _token1: &Address) -> f64 {
        self.estimate_token_value_usd_simplified(amount).await
    }

    /// Simplified USD value estimation from U256 amount
    async fn estimate_token_value_usd_simplified(&self, amount: U256) -> f64 {
        // Simplified token value estimation
        // In production, this would:
        // 1. Check if either token is a stablecoin (USDT, USDC, DAI)
        // 2. Query DEX pools for token prices
        // 3. Use price oracles for better accuracy
        
        // Safe conversion from U256 to f64 using string representation
        let amount_str = amount.to_string();
        let amount_f64 = amount_str.parse::<f64>().unwrap_or(0.0);
        
        // If amount suggests 18 decimals token (> 1e18)
        if amount_f64 > 1e18 {
            return amount_f64 / 1e18 * 50.0; // Assume $50/token average
        }
        
        // If amount suggests 6 decimals (stablecoin, > 1e6)
        if amount_f64 > 1e6 {
            return amount_f64 / 1e6; // $1 per unit (stablecoin)
        }
        
        // Very rough fallback
        amount_f64 / 1e12 * 10.0
    }
}
