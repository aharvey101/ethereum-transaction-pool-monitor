/// Real-Time Mempool Monitor for MEV Bot
/// 
/// This module provides WebSocket-based real-time monitoring of the Ethereum mempool,
/// filtering for DEX transactions that could be potential sandwich targets.

use crate::{
    pool_db::PoolDatabase,
    eth_client::EthereumClient,
    enhanced_revm_simulator::{EnhancedSandwichSimulator, PoolSelectionCriteria},
    sandwich_pool_integration::SandwichTarget,
};
use alloy_primitives::{Address, U256, Bytes, hex::FromHex};
use anyhow::Result;
use tokio::sync::mpsc;
use std::collections::HashSet;
use std::time::SystemTime;
use std::str::FromStr;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error, debug};

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
    Buy,   // Buying token with ETH/stable
    Sell,  // Selling token for ETH/stable
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
            min_tx_value_usd: 10_000.0,    // $10k minimum transaction size
            max_gas_price_gwei: 100.0,      // 100 gwei max gas price
            target_protocols: vec![
                "UniswapV2".to_string(),
                "UniswapV3".to_string(), 
                "SushiSwap".to_string(),
            ],
            min_profit_threshold_eth: 0.005, // 0.005 ETH minimum profit (after gas)
            max_price_impact: 0.05,          // 5% maximum price impact
            confidence_threshold: 0.7,       // 70% minimum confidence
        }
    }
}

/// Real-time mempool monitor
pub struct MempoolMonitor {
    eth_client: EthereumClient,
    pool_db: PoolDatabase,
    simulator: EnhancedSandwichSimulator,
    config: MempoolConfig,
    known_pools: HashSet<Address>,
    opportunity_sender: mpsc::UnboundedSender<MempoolOpportunity>,
    stats: MonitorStats,
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
        
        // Initialize enhanced simulator
        debug!("🧠 Initializing sandwich simulator...");
        let criteria = PoolSelectionCriteria {
            min_liquidity_usd: 50_000.0,
            max_price_impact: config.max_price_impact,
            min_volume_24h_usd: 25_000.0,
            supported_protocols: config.target_protocols.clone(),
            max_gas_price_gwei: config.max_gas_price_gwei,
            min_profit_threshold_eth: config.min_profit_threshold_eth,
        };
        
        let simulator = EnhancedSandwichSimulator::new(
            db_path,
            rpc_url,
            eth_client.clone(),
            Some(criteria),
        ).await?;
        info!("✅ Enhanced sandwich simulator ready");
        
        // Pre-load known pool addresses for fast filtering
        debug!("📚 Loading known pool addresses for mempool filtering...");
        let known_pools = Self::load_known_pools(&pool_db).await?;
        info!("✅ Loaded {} known pool addresses for mempool filtering", known_pools.len());
        
        let (opportunity_sender, opportunity_receiver) = mpsc::unbounded_channel();
        
        let monitor = Self {
            eth_client,
            pool_db,
            simulator,
            config,
            known_pools,
            opportunity_sender,
            stats: MonitorStats::default(),
        };
        
        Ok((monitor, opportunity_receiver))
    }
    
    /// Start monitoring the mempool for MEV opportunities
    pub async fn start_monitoring(&mut self) -> Result<()> {
        info!("🔍 Starting real-time mempool monitoring for MEV opportunities");
        info!("📊 Config: min_value=${}, max_gas={} gwei, min_profit={} ETH", 
            self.config.min_tx_value_usd, 
            self.config.max_gas_price_gwei, 
            self.config.min_profit_threshold_eth
        );
        info!("🎯 Monitoring {} protocols: {:?}", 
            self.config.target_protocols.len(),
            self.config.target_protocols
        );
        
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
                                        debug!("📨 Processing new pending transaction: {}", tx_hash);
                                        let tx_hash_for_debug = tx_hash.clone();
                                        
                                        if let Err(e) = self.process_mempool_transaction(tx_hash).await {
                                            debug!("⚠️  Failed to process transaction {}: {}", tx_hash_for_debug, e);
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
                    error!("❌ Failed to subscribe to mempool: {}. Retrying in 5 seconds...", e);
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
        let tx_details = self.eth_client.get_transaction_by_hash(&tx_hash, &self.pool_db, 1).await?;
        
        // Quick filter: Check if transaction interacts with known pools  
        let target_address = match &tx_details.to {
            Some(addr_str) => {
                match addr_str.parse::<Address>() {
                    Ok(addr) => addr,
                    Err(_) => {
                        debug!("🔍 Skipping tx {} - invalid address format", tx_hash);
                        return Ok(());
                    }
                }
            },
            None => {
                debug!("🔍 Skipping tx {} - contract creation", tx_hash);
                return Ok(());
            }
        };
        
        if !self.known_pools.contains(&target_address) {
            debug!("🔍 Skipping tx {} - not a DEX interaction (target: {})", tx_hash, target_address);
            return Ok(()); // Not a DEX interaction
        }
        
        debug!("🎯 Found DEX interaction: {} -> {}", tx_hash, target_address);
        
        // Convert to our mempool transaction format
        debug!("🔄 Converting transaction to mempool format...");
        let mempool_tx = self.convert_to_mempool_tx(tx_hash, tx_details).await?;
        
        if !mempool_tx.is_dex_interaction {
            debug!("🔍 Transaction is not a DEX interaction after analysis");
            return Ok(());
        }
        
        self.stats.dex_transactions_found += 1;
        debug!("🎯 DEX transaction detected: {} (${:.0})", mempool_tx.hash, mempool_tx.estimated_value_usd);
        
        // Check if transaction meets our value threshold
        if mempool_tx.estimated_value_usd < self.config.min_tx_value_usd {
            debug!("💸 Transaction value too low: ${:.0} < ${:.0}", 
                mempool_tx.estimated_value_usd, self.config.min_tx_value_usd);
            return Ok(());
        }
        
        // Check gas price threshold
        let gas_price_gwei = mempool_tx.gas_price.to::<u64>() as f64 / 1e9;
        if gas_price_gwei > self.config.max_gas_price_gwei {
            debug!("⛽ Gas price too high: {:.1} gwei > {:.1} gwei", 
                gas_price_gwei, self.config.max_gas_price_gwei);
            return Ok(());
        }
        
        info!("✨ Potential MEV target: {} (${:.0}, {:.1} gwei)", 
            mempool_tx.hash, mempool_tx.estimated_value_usd, gas_price_gwei);
        
        // Analyze for sandwich opportunity
        debug!("🧠 Analyzing sandwich opportunity for transaction...");
        if let Some(opportunity) = self.analyze_sandwich_opportunity(mempool_tx).await? {
            self.stats.opportunities_detected += 1;
            debug!("🎯 Opportunity found! Estimated profit: {:.4} ETH, confidence: {:.1}%", 
                opportunity.estimated_profit_eth, opportunity.confidence_score * 100.0);
            
            if opportunity.estimated_profit_eth >= self.config.min_profit_threshold_eth 
                && opportunity.confidence_score >= self.config.confidence_threshold {
                
                self.stats.profitable_opportunities += 1;
                info!("💰 Profitable MEV opportunity detected: {:.4} ETH profit ({}% confidence)", 
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
        victim_tx: MempoolTransaction
    ) -> Result<Option<MempoolOpportunity>> {
        
        let pool_address = match victim_tx.target_pool {
            Some(addr) => addr,
            None => return Ok(None),
        };
        
        // Create mock sandwich target for simulation
        let mock_target = self.create_sandwich_target(&victim_tx, pool_address).await?;
        
        // Simulate the sandwich attack
        let simulation_result = self.simulator.simulate_sandwich_enhanced(
            &mock_target, 
            victim_tx.value, 
            2.0 // 2x frontrun multiplier
        ).await?;
        
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
            required_capital_eth: simulation_result.frontrun_amount.to::<u64>() as f64 / 1e18,
        };
        
        Ok(Some(opportunity))
    }
    
    /// Calculate confidence score for the opportunity (0-1)
    fn calculate_confidence_score(
        &self, 
        victim_tx: &MempoolTransaction, 
        simulation: &crate::enhanced_revm_simulator::EnhancedSandwichResult
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
        let gas_factor = if gas_gwei < 50.0 { 1.0 } else { (100.0 - gas_gwei).max(0.0) / 50.0 };
        confidence += 0.1 * gas_factor as f32;
        
        // Risk score factor (lower risk = higher confidence)
        let risk_factor = (100 - simulation.risk_score) as f32 / 100.0;
        confidence += 0.1 * risk_factor;
        
        confidence.min(1.0)
    }
    
    /// Load known pool addresses from database for fast filtering
    async fn load_known_pools(pool_db: &PoolDatabase) -> Result<HashSet<Address>> {
        let mut pools = HashSet::new();
        
        // Load from each protocol
        for protocol in &["UniswapV2", "UniswapV3", "SushiSwap", "Curve"] {
            let protocol_pools = pool_db.get_pools_by_protocol(protocol, 50000)?; // Load up to 50k pools per protocol
            for pool in protocol_pools {
                if let Ok(addr) = pool.address.parse::<Address>() {
                    pools.insert(addr);
                }
            }
        }
        
        Ok(pools)
    }
    
    /// Convert raw transaction to our mempool transaction format
    async fn convert_to_mempool_tx(
        &self, 
        tx_hash: String, 
        tx_details: crate::eth_client::MempoolTransaction
    ) -> Result<MempoolTransaction> {
        // Parse from address
        let from_addr = Address::from_str(&tx_details.from)?;
        let to_addr = tx_details.to.as_ref()
            .and_then(|s| Address::from_str(s).ok());
        
        // Convert hex values to proper types using the pre-computed f64 values
        let eth_value = tx_details.value_f64;
        let value = U256::from((eth_value * 1e18) as u64);
        
        let gas_price_gwei = tx_details.gas_price_f64;
        let gas_price = U256::from((gas_price_gwei * 1e9) as u64);
        
        // Parse gas limit from hex string
        let gas_limit = U256::from_str_radix(
            tx_details.gas.trim_start_matches("0x"), 16
        ).unwrap_or(U256::from(21000u64));
        
        // Parse input data
        let input = Bytes::from_hex(&tx_details.data).unwrap_or_default();
        
        // Check if this is a DEX interaction (either direct pool or router)
        let is_dex = to_addr.map_or(false, |addr| {
            let addr_string = format!("{:#x}", addr); // Use hex format like 0x1234...
            let is_pool = self.known_pools.contains(&addr);
            let is_router = self.pool_db.is_dex_router(&addr_string);
            debug!("🔍 Checking address: {} - Pool: {}, Router: {}", addr_string, is_pool, is_router);
            is_pool || is_router
        });
        
        // Estimate USD value (simplified)
        let estimated_value_usd = eth_value * 2000.0; // Assume $2000 ETH
        
        let target_pool = if is_dex {
            // For direct pool interactions, use the to_addr
            if self.known_pools.contains(&to_addr.unwrap_or_default()) {
                to_addr
            } else {
                // For router transactions, we'd need to parse the input data
                // to determine the target pool. For now, return None and 
                // let the sandwich analyzer handle pool detection.
                None
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
        let pools = self.pool_db.get_pools_by_address(&pool_address.to_string())?;
        let pool_info = pools.first().ok_or_else(|| anyhow::anyhow!("Pool not found"))?;
        
        // Create pool state
        let pool_state = crate::sandwich_pool_integration::PoolState {
            address: pool_address,
            protocol: pool_info.protocol.clone(),
            token0: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse()?, // WETH
            token1: "0xdAC17F958D2ee523a2206206994597C13D831ec7".parse()?, // USDT
            fee: 3000, // Default 0.3% fee
            reserve0: U256::from(1000000000000000000000000_u128), // Mock reserves
            reserve1: U256::from(1000000000000_u128),
            block_number: 0, // Mock block number
            total_liquidity_usd: 100000.0, // Mock liquidity
        };
        
        Ok(SandwichTarget {
            victim_tx_hash: victim_tx.hash.clone(),
            pool: pool_state,
            victim_trade_amount: victim_tx.value,
            victim_trade_direction: crate::sandwich_pool_integration::TradeDirection::Token0ToToken1,
            recommended_frontrun_amount: victim_tx.value * U256::from(2),
            estimated_profit_eth: 0.0, // Will be calculated by simulation
            risk_score: 30,
            gas_cost_estimate: U256::from(500_000u64) * U256::from(20_000_000_000u64),
        })
    }
    
    /// Print monitoring statistics
    fn print_monitoring_stats(&mut self) {
        info!("📊 Mempool Monitor Stats:");
        info!("   Total transactions: {}", self.stats.total_transactions_seen);
        info!("   DEX transactions: {} ({:.1}%)", 
            self.stats.dex_transactions_found,
            (self.stats.dex_transactions_found as f64 / self.stats.total_transactions_seen.max(1) as f64) * 100.0
        );
        info!("   Opportunities detected: {}", self.stats.opportunities_detected);
        info!("   Profitable opportunities: {} ({:.1}%)", 
            self.stats.profitable_opportunities,
            (self.stats.profitable_opportunities as f64 / self.stats.opportunities_detected.max(1) as f64) * 100.0
        );
        
        // Update average confidence
        if self.stats.opportunities_detected > 0 {
            info!("   Average confidence: {:.1}%", self.stats.average_confidence * 100.0);
        }
    }
}