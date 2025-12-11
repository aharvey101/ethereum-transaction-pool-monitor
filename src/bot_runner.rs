/// Continuous MEV Bot Runner
///
/// This module provides the main execution loop for the MEV bot, coordinating
/// mempool monitoring, opportunity detection, bundle building, and submission.
use crate::{
    eth_client::EthereumClient,
    mempool_monitor::{MempoolMonitor, MempoolOpportunity},
    mev_bundle_builder::MevBundleBuilder,
    pool_db::PoolDatabase,
};
use alloy_primitives::U256;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::{
    select,
    sync::{mpsc, RwLock},
    time::interval,
};
use tracing::{debug, error, info, warn};

/// Configuration for the MEV bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub min_value_usd: f64,
    pub min_gas_price_gwei: f64,
    pub max_gas_price_gwei: f64,
    pub confidence_threshold: f64,
    pub enable_websocket: bool,
    pub max_gas_price: U256,
    pub min_profit_threshold: f64, // ETH
    pub max_concurrent_bundles: usize,
    pub bundle_timeout_seconds: u64,
    pub stats_interval_seconds: u64,
    pub max_opportunities_per_block: usize,
    pub enable_flashbots: bool,
    pub signing_key: Option<String>,
    pub sandwich_contract_address: Option<String>, // Deployed sandwich contract
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            min_value_usd: 1.0, // $1 minimum for testing
            min_gas_price_gwei: 5.0,
            max_gas_price_gwei: 200.0, // Increased for testing
            confidence_threshold: 0.3, // Lower threshold for testing
            enable_websocket: true,
            max_gas_price: U256::from(200_000_000_000_u64), // 200 gwei for testing
            min_profit_threshold: 0.001,                    // 1 mETH default (should be overridden by CLI)
            max_concurrent_bundles: 5,
            bundle_timeout_seconds: 10,
            stats_interval_seconds: 30,
            max_opportunities_per_block: 3,
            enable_flashbots: false, // Start with simulation mode
            signing_key: None,
            sandwich_contract_address: None,
        }
    }
}

/// MEV bot execution statistics
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BotStats {
    pub start_time: SystemTime,
    pub opportunities_detected: u64,
    pub bundles_built: u64,
    pub bundles_submitted: u64,
    pub bundles_included: u64,
    pub total_profit_eth: f64,
    pub total_gas_fees_eth: f64,
    pub successful_sandwiches: u64,
    pub failed_submissions: u64,
    pub current_block: u64,
    pub mempool_tx_processed: u64,
    pub avg_opportunity_confidence: f64,
}

impl Default for BotStats {
    fn default() -> Self {
        Self {
            start_time: SystemTime::now(),
            opportunities_detected: 0,
            bundles_built: 0,
            bundles_submitted: 0,
            bundles_included: 0,
            total_profit_eth: 0.0,
            total_gas_fees_eth: 0.0,
            successful_sandwiches: 0,
            failed_submissions: 0,
            current_block: 0,
            mempool_tx_processed: 0,
            avg_opportunity_confidence: 0.0,
        }
    }
}

impl BotStats {
    /// Calculate bot performance metrics
    pub fn calculate_metrics(&self) -> BotMetrics {
        let uptime_hours = self.start_time.elapsed().unwrap_or_default().as_secs_f64() / 3600.0;

        BotMetrics {
            uptime_hours,
            success_rate: if self.bundles_submitted > 0 {
                self.bundles_included as f64 / self.bundles_submitted as f64
            } else {
                0.0
            },
            profit_per_hour: if uptime_hours > 0.0 {
                self.total_profit_eth / uptime_hours
            } else {
                0.0
            },
            opportunities_per_hour: if uptime_hours > 0.0 {
                self.opportunities_detected as f64 / uptime_hours
            } else {
                0.0
            },
            net_profit_eth: self.total_profit_eth - self.total_gas_fees_eth,
        }
    }
}

/// Calculated bot performance metrics
#[derive(Debug, Clone)]
pub struct BotMetrics {
    pub uptime_hours: f64,
    pub success_rate: f64,           // Bundles included / bundles submitted
    pub profit_per_hour: f64,        // ETH per hour
    pub opportunities_per_hour: f64, // Opportunities detected per hour
    pub net_profit_eth: f64,         // Total profit minus gas costs
}

/// Active bundle tracking
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ActiveBundle {
    opportunity: MempoolOpportunity,
    target_block: u64,
    submission_time: Instant,
    bundle_hash: Option<String>,
}

/// Main MEV bot runner
pub struct MevBotRunner {
    config: BotConfig,
    rpc_url: String,
    db_path: String,
    eth_client: Arc<EthereumClient>,
    pool_db: Arc<PoolDatabase>,
    stats: Arc<RwLock<BotStats>>,
    active_bundles: Arc<RwLock<HashMap<String, ActiveBundle>>>,
    opportunity_queue: Arc<RwLock<VecDeque<MempoolOpportunity>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl MevBotRunner {
    /// Create new MEV bot runner
    pub async fn new(
        config: BotConfig,
        eth_client: EthereumClient,
        pool_db: PoolDatabase,
        rpc_url: String,
        db_path: String,
    ) -> Result<Self> {
        let eth_client = Arc::new(eth_client);
        let pool_db = Arc::new(pool_db);

        let stats = Arc::new(RwLock::new(BotStats {
            start_time: SystemTime::now(),
            ..Default::default()
        }));

        Ok(Self {
            config,
            rpc_url,
            db_path,
            eth_client,
            pool_db,
            stats,
            active_bundles: Arc::new(RwLock::new(HashMap::new())),
            opportunity_queue: Arc::new(RwLock::new(VecDeque::new())),
            shutdown_tx: None,
        })
    }

    /// Start the MEV bot main execution loop
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting MEV bot runner...");
        self.print_startup_banner().await;

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Create mempool config
        let mempool_config = crate::mempool_monitor::MempoolConfig {
            min_tx_value_usd: self.config.min_value_usd,
            max_gas_price_gwei: self.config.max_gas_price_gwei,
            target_protocols: vec![
                "UniswapV2".to_string(),
                "UniswapV3".to_string(),
                "SushiSwap".to_string(),
            ],
            min_profit_threshold_eth: self.config.min_profit_threshold,
            max_price_impact: 0.05, // 5%
            confidence_threshold: self.config.confidence_threshold as f32,
        };

        // Start mempool monitoring
        let (mempool_monitor, opportunity_rx) =
            MempoolMonitor::new(&self.rpc_url, &self.db_path, mempool_config).await?;

        let mempool_task = {
            let mut monitor = mempool_monitor;
            tokio::spawn(async move {
                if let Err(e) = monitor.start_monitoring().await {
                    error!("Mempool monitoring failed: {}", e);
                }
            })
        };

        // Start opportunity processing
        let opportunity_task = {
            let stats = self.stats.clone();
            let opportunity_queue = self.opportunity_queue.clone();
            tokio::spawn(async move {
                Self::process_opportunities(opportunity_rx, stats, opportunity_queue).await;
            })
        };

        // Start bundle execution
        let bundle_task = {
            let config = self.config.clone();
            let rpc_url = self.rpc_url.clone();
            let eth_client = self.eth_client.clone();
            let stats = self.stats.clone();
            let active_bundles = self.active_bundles.clone();
            let opportunity_queue = self.opportunity_queue.clone();
            tokio::spawn(async move {
                Self::execute_bundles_loop(
                    config,
                    rpc_url,
                    eth_client,
                    stats,
                    active_bundles,
                    opportunity_queue,
                )
                .await;
            })
        };

        // Start statistics reporting
        let stats_task = {
            let stats = self.stats.clone();
            let interval_secs = self.config.stats_interval_seconds;
            tokio::spawn(async move {
                Self::report_statistics_loop(stats, interval_secs).await;
            })
        };

        // Start bundle cleanup
        let cleanup_task = {
            let active_bundles = self.active_bundles.clone();
            let timeout = Duration::from_secs(self.config.bundle_timeout_seconds);
            tokio::spawn(async move {
                Self::cleanup_bundles_loop(active_bundles, timeout).await;
            })
        };

        info!("All MEV bot components started successfully");

        // Wait for shutdown signal
        select! {
            _ = shutdown_rx.recv() => {
                info!("Shutdown signal received");
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C received, shutting down gracefully");
            }
        }

        // Graceful shutdown
        info!("Stopping MEV bot...");
        mempool_task.abort();
        opportunity_task.abort();
        bundle_task.abort();
        stats_task.abort();
        cleanup_task.abort();

        self.print_final_stats().await;
        info!("MEV bot stopped");

        Ok(())
    }

    /// Process detected opportunities from mempool
    async fn process_opportunities(
        mut opportunity_rx: mpsc::UnboundedReceiver<MempoolOpportunity>,
        stats: Arc<RwLock<BotStats>>,
        opportunity_queue: Arc<RwLock<VecDeque<MempoolOpportunity>>>,
    ) {
        while let Some(opportunity) = opportunity_rx.recv().await {
            info!("Processing new opportunity: {}", opportunity.victim_tx.hash);

            // Update stats
            {
                let mut stats_lock = stats.write().await;
                stats_lock.opportunities_detected += 1;

                // Update average confidence
                let total_conf = stats_lock.avg_opportunity_confidence
                    * (stats_lock.opportunities_detected - 1) as f64;
                stats_lock.avg_opportunity_confidence = (total_conf
                    + opportunity.confidence_score as f64)
                    / stats_lock.opportunities_detected as f64;
            }

            // Add to queue for bundle execution
            {
                let mut queue = opportunity_queue.write().await;
                queue.push_back(opportunity);

                // Limit queue size to prevent memory issues
                if queue.len() > 1000 {
                    queue.pop_front();
                    warn!("Opportunity queue full, dropping oldest opportunity");
                }
            }
        }
    }

    /// Print startup banner with configuration
    async fn print_startup_banner(&self) {
        info!("╔══════════════════════════════════════════════════════════════╗");
        info!("║                  MEV BOT RUNNER STARTED                     ║");
        info!("╠══════════════════════════════════════════════════════════════╣");
        info!(
            "║ Min Profit Threshold: {:.4} ETH                           ║",
            self.config.min_profit_threshold
        );
        info!(
            "║ Max Gas Price: {} gwei                                  ║",
            self.config.max_gas_price / U256::from(1_000_000_000)
        );
        info!(
            "║ Max Concurrent Bundles: {}                                ║",
            self.config.max_concurrent_bundles
        );
        info!(
            "║ Flashbots Enabled: {}                                    ║",
            self.config.enable_flashbots
        );
        info!(
            "║ Pool Database: {} pools loaded                         ║",
            self.pool_db.get_total_pools().await.unwrap_or(0)
        );
        info!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Print final statistics on shutdown
    async fn print_final_stats(&self) {
        let stats = self.stats.read().await;
        let metrics = stats.calculate_metrics();

        info!("╔══════════════════════════════════════════════════════════════╗");
        info!("║                     FINAL MEV BOT STATS                     ║");
        info!("╠══════════════════════════════════════════════════════════════╣");
        info!(
            "║ Uptime: {:.2} hours                                       ║",
            metrics.uptime_hours
        );
        info!(
            "║ Opportunities Detected: {}                                ║",
            stats.opportunities_detected
        );
        info!(
            "║ Bundles Submitted: {}                                      ║",
            stats.bundles_submitted
        );
        info!(
            "║ Bundles Included: {}                                       ║",
            stats.bundles_included
        );
        info!(
            "║ Success Rate: {:.1}%                                       ║",
            metrics.success_rate * 100.0
        );
        info!(
            "║ Total Profit: {:.4} ETH                                   ║",
            stats.total_profit_eth
        );
        info!(
            "║ Net Profit: {:.4} ETH                                     ║",
            metrics.net_profit_eth
        );
        info!(
            "║ Profit/Hour: {:.4} ETH                                    ║",
            metrics.profit_per_hour
        );
        info!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Statistics reporting loop
    async fn report_statistics_loop(stats: Arc<RwLock<BotStats>>, interval_seconds: u64) {
        let mut interval = interval(Duration::from_secs(interval_seconds));

        loop {
            interval.tick().await;

            let stats_snapshot = stats.read().await.clone();
            let metrics = stats_snapshot.calculate_metrics();

            info!("🤖 MEV Bot Stats | Uptime: {:.1}h | Opportunities: {} | Bundles: {}/{} ({:.1}%) | Profit: {:.4} ETH | Rate: {:.2}/h",
                metrics.uptime_hours,
                stats_snapshot.opportunities_detected,
                stats_snapshot.bundles_included,
                stats_snapshot.bundles_submitted,
                metrics.success_rate * 100.0,
                metrics.net_profit_eth,
                metrics.opportunities_per_hour
            );
        }
    }

    /// Bundle cleanup loop - remove expired bundles
    async fn cleanup_bundles_loop(
        active_bundles: Arc<RwLock<HashMap<String, ActiveBundle>>>,
        timeout: Duration,
    ) {
        let mut interval = interval(Duration::from_secs(10));

        loop {
            interval.tick().await;

            let mut bundles = active_bundles.write().await;
            let now = Instant::now();

            let expired: Vec<String> = bundles
                .iter()
                .filter(|(_, bundle)| now.duration_since(bundle.submission_time) > timeout)
                .map(|(hash, _)| hash.clone())
                .collect();

            for hash in expired {
                bundles.remove(&hash);
                debug!("Removed expired bundle: {}", hash);
            }
        }
    }

    /// Stop the MEV bot
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(()).await;
        }
        Ok(())
    }

    /// Main bundle execution loop (static method)
    async fn execute_bundles_loop(
        config: BotConfig,
        _rpc_url: String,
        eth_client: Arc<EthereumClient>,
        stats: Arc<RwLock<BotStats>>,
        active_bundles: Arc<RwLock<HashMap<String, ActiveBundle>>>,
        opportunity_queue: Arc<RwLock<VecDeque<MempoolOpportunity>>>,
    ) {
        let execution_method = if config.enable_flashbots {
            crate::mev_bundle_builder::ExecutionMethod::Flashbots
        } else {
            crate::mev_bundle_builder::ExecutionMethod::SimulationOnly
        };

        let mut bundle_builder = MevBundleBuilder::new();

        // Configure the bundle builder
        bundle_builder.max_gas_price = config
            .max_gas_price
            .to_string()
            .parse::<u128>()
            .unwrap_or(200_000_000_000);
        bundle_builder.min_profit_threshold =
            U256::from((config.min_profit_threshold * 1e18) as u64);

        // Set sandwich contract for Flashbots execution
        if config.enable_flashbots {
            if let Some(contract_addr) = &config.sandwich_contract_address {
                match contract_addr.parse::<alloy_primitives::Address>() {
                    Ok(sandwich_contract_address) => {
                        info!("🏗️ Using sandwich contract at: {}", sandwich_contract_address);
                        // Note: The signer address comes from the signing_key, not separate config
                        if let Some(signing_key) = &config.signing_key {
                            match alloy::signers::local::PrivateKeySigner::from_slice(
                                &hex::decode(signing_key.trim_start_matches("0x")).unwrap_or_default()
                            ) {
                                Ok(signer) => {
                                    let signer_address = signer.address();
                                    info!("🔑 Using signer address: {}", signer_address);
                                    bundle_builder.set_sandwich_contract(sandwich_contract_address, signer_address);
                                }
                                Err(e) => {
                                    warn!("❌ Invalid signing key: {}. Disabling Flashbots execution.", e);
                                }
                            }
                        } else {
                            warn!("❌ No signing key provided. Disabling Flashbots execution.");
                        }
                    }
                    Err(e) => {
                        warn!("❌ Invalid sandwich contract address '{}': {}. Disabling Flashbots execution.", contract_addr, e);
                    }
                }
            } else {
                warn!("❌ No sandwich contract address configured. Disabling Flashbots execution.");
                warn!("   To enable Flashbots, deploy the contract from contracts/FlashLoanSandwich.sol");
                warn!("   and set the address in config.sandwich_contract_address");
            }
        }

        let mut interval = interval(Duration::from_millis(100)); // Check every 100ms

        loop {
            interval.tick().await;

            // Get current block number
            let current_block = match eth_client.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    warn!("Failed to get block number: {}", e);
                    continue;
                }
            };

            // Update stats
            {
                let mut stats_lock = stats.write().await;
                stats_lock.current_block = current_block;
            }

            // Process opportunities for next block
            let target_block = current_block + 1;

            // Check if we have capacity for more bundles
            let active_count = active_bundles.read().await.len();
            if active_count >= config.max_concurrent_bundles {
                continue;
            }

            // Get next opportunity
            let opportunity = {
                let mut queue = opportunity_queue.write().await;
                queue.pop_front()
            };

            if let Some(opportunity) = opportunity {
                // Only process high-confidence opportunities
                if opportunity.confidence_score < config.confidence_threshold as f32 {
                    debug!(
                        "Skipping low-confidence opportunity: {:.2}",
                        opportunity.confidence_score
                    );
                    continue;
                }

                // Check profit threshold
                if opportunity.estimated_profit_eth < config.min_profit_threshold {
                    debug!(
                        "Skipping low-profit opportunity: {:.4} ETH",
                        opportunity.estimated_profit_eth
                    );
                    continue;
                }

                if let Err(e) = Self::execute_opportunity(
                    &bundle_builder,
                    opportunity,
                    execution_method.clone(),
                    target_block,
                    &stats,
                    &active_bundles,
                )
                .await
                {
                    error!("Failed to execute opportunity: {}", e);
                }
            }
        }
    }

    /// Execute a single MEV opportunity (static method)
    async fn execute_opportunity(
        bundle_builder: &MevBundleBuilder,
        opportunity: MempoolOpportunity,
        execution_method: crate::mev_bundle_builder::ExecutionMethod,
        target_block: u64,
        stats: &Arc<RwLock<BotStats>>,
        active_bundles: &Arc<RwLock<HashMap<String, ActiveBundle>>>,
    ) -> Result<()> {
        let start_time = Instant::now();
        debug!(
            "Executing opportunity for block {}: {}",
            target_block, opportunity.victim_tx.hash
        );

        // Execute sandwich attack using the specified method
        let submission_result = match bundle_builder
            .execute_sandwich_attack(&opportunity, execution_method.clone())
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    "Failed to execute sandwich for {}: {}",
                    opportunity.victim_tx.hash, e
                );
                return Err(e);
            }
        };

        // Update stats
        {
            let mut stats_lock = stats.write().await;
            stats_lock.bundles_built += 1;
        }

        // Update stats based on result
        {
            let mut stats_lock = stats.write().await;

            if submission_result.submitted {
                stats_lock.bundles_submitted += 1;
                stats_lock.total_profit_eth += submission_result.profit_eth;
                stats_lock.total_gas_fees_eth += submission_result.total_gas_used as f64 * 0.5e-9; // Estimate gas cost at 0.5 gwei

                info!(
                    "Bundle submitted for block {} | Hash: {:?} | Profit: {:.4} ETH | Gas: {}",
                    target_block,
                    submission_result.bundle_hash,
                    submission_result.profit_eth,
                    submission_result.total_gas_used
                );
            } else {
                stats_lock.failed_submissions += 1;
                warn!("Bundle submission failed: {:?}", submission_result.error);
            }
        }

        // Track active bundle for inclusion monitoring
        if let Some(bundle_hash) = submission_result.bundle_hash {
            let active_bundle = ActiveBundle {
                opportunity,
                target_block,
                submission_time: start_time,
                bundle_hash: Some(bundle_hash.clone()),
            };

            active_bundles
                .write()
                .await
                .insert(bundle_hash, active_bundle);
        }

        debug!(
            "Opportunity execution completed in {:?}",
            start_time.elapsed()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mempool_monitor::{MempoolTransaction, TradeDirection};
    use alloy_primitives::Address;

    #[test]
    fn test_bot_config_default() {
        let config = BotConfig::default();
        assert_eq!(config.min_profit_threshold, 0.01);
        assert_eq!(config.max_concurrent_bundles, 5);
        assert!(!config.enable_flashbots); // Should start in simulation mode
    }

    #[test]
    fn test_bot_stats_metrics() {
        let mut stats = BotStats::default();
        stats.start_time = SystemTime::now() - Duration::from_secs(3600); // 1 hour ago
        stats.opportunities_detected = 100;
        stats.bundles_submitted = 50;
        stats.bundles_included = 25;
        stats.total_profit_eth = 2.5;
        stats.total_gas_fees_eth = 0.5;

        let metrics = stats.calculate_metrics();
        assert!(metrics.uptime_hours >= 0.9 && metrics.uptime_hours <= 1.1); // ~1 hour
        assert_eq!(metrics.success_rate, 0.5); // 50% success rate
        assert_eq!(metrics.net_profit_eth, 2.0); // 2.5 - 0.5
        assert!(metrics.opportunities_per_hour >= 90.0); // ~100/hour
    }
}
