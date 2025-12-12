use anyhow::Result;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;
use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use tracing::{info, warn, debug};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <rpc_url>", args[0]);
        println!("Example: {} http://192.168.0.14:8545", args[0]);
        return Ok(());
    }

    let rpc_url = &args[1];
    info!("🔍 Analyzing mempool DEX transactions from: {}", rpc_url);

    // Initialize clients and database
    info!("📡 Connecting to Ethereum node...");
    let eth_client = EthereumClient::new(rpc_url).await?;
    
    info!("📊 Loading pool database...");
    let pool_db = PoolDatabase::new("./dex_pools.db")?;
    
    // Get current mempool status
    info!("🔍 Fetching mempool status...");
    let mempool_status = get_mempool_status(rpc_url).await?;
    info!("📊 Mempool Status: {} pending, {} queued transactions", 
          mempool_status.pending, mempool_status.queued);

    // Get all pending transactions
    info!("📦 Fetching all pending transactions...");
    let pending_txs = get_all_pending_transactions(rpc_url).await?;
    info!("🎯 Found {} pending transactions to analyze", pending_txs.len());

    if pending_txs.is_empty() {
        warn!("❌ No pending transactions found in mempool");
        return Ok(());
    }

    // Analyze transactions for DEX interactions
    info!("🔍 Analyzing transactions for DEX interactions...");
    let analysis_result = analyze_dex_transactions(&pool_db, pending_txs).await?;
    
    // Print comprehensive results
    print_analysis_results(&analysis_result);

    Ok(())
}

#[derive(Debug)]
struct MempoolStatus {
    pending: u32,
    queued: u32,
}

#[derive(Debug, Clone)]
struct TransactionInfo {
    hash: String,
    from: String,
    to: Option<String>,
    value: String,
    gas: String,
    gas_price: String,
    data: String,
}

#[derive(Debug)]
struct DexAnalysisResult {
    total_transactions: usize,
    dex_transactions: Vec<DexTransaction>,
    contract_creations: usize,
    regular_transfers: usize,
    unknown_contracts: usize,
    router_interactions: HashMap<String, usize>,
    pool_interactions: HashMap<String, usize>,
}

#[derive(Debug)]
struct DexTransaction {
    hash: String,
    from: String,
    to: String,
    dex_type: DexType,
    estimated_value_eth: f64,
    gas_price_gwei: f64,
}

#[derive(Debug, Clone)]
enum DexType {
    UniswapV2Router,
    UniswapV3Router,
    SushiSwapRouter,
    CurvePool,
    BalancerVault,
    OneInchRouter,
    DirectPool,
    UnknownDex,
}

async fn get_mempool_status(rpc_url: &str) -> Result<MempoolStatus> {
    let response = call_rpc(rpc_url, "txpool_status", json!([])).await?;
    
    let pending_hex = response["pending"].as_str().unwrap_or("0x0");
    let queued_hex = response["queued"].as_str().unwrap_or("0x0");
    
    let pending = u32::from_str_radix(&pending_hex[2..], 16).unwrap_or(0);
    let queued = u32::from_str_radix(&queued_hex[2..], 16).unwrap_or(0);
    
    Ok(MempoolStatus { pending, queued })
}

async fn get_all_pending_transactions(rpc_url: &str) -> Result<Vec<TransactionInfo>> {
    debug!("📦 Calling txpool_content...");
    let response = call_rpc(rpc_url, "txpool_content", json!([])).await?;
    debug!("✅ Received txpool_content response");
    
    let mut transactions = Vec::new();
    
    if let Some(pending) = response["pending"].as_object() {
        info!("📊 Found {} addresses with pending transactions", pending.len());
        
        // Limit to first 50 addresses to avoid overwhelming processing
        for (i, (address, nonces)) in pending.iter().enumerate() {
            if i >= 50 {
                info!("📝 Limiting to first {} addresses (out of {}) for faster analysis", i, pending.len());
                break;
            }
            
            if i % 10 == 0 && i > 0 {
                info!("📈 Progress: {}/{} addresses processed", i, pending.len().min(50));
            }
            
            if let Some(nonce_map) = nonces.as_object() {
                for (_nonce, tx_data) in nonce_map {
                    if let Some(tx) = tx_data.as_object() {
                        let tx_info = TransactionInfo {
                            hash: tx.get("hash").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            from: address.clone(),
                            to: tx.get("to").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            value: tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0").to_string(),
                            gas: tx.get("gas").and_then(|v| v.as_str()).unwrap_or("0x0").to_string(),
                            gas_price: tx.get("gasPrice")
                                .or_else(|| tx.get("maxFeePerGas"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("0x0").to_string(),
                            data: tx.get("input").and_then(|v| v.as_str()).unwrap_or("0x").to_string(),
                        };
                        
                        if !tx_info.hash.is_empty() {
                            transactions.push(tx_info);
                        }
                    }
                }
            }
        }
    }
    
    info!("🎯 Collected {} transactions for analysis", transactions.len());
    
    // Additional limit to first 100 transactions total for analysis
    if transactions.len() > 100 {
        info!("📝 Limiting analysis to first 100 transactions (out of {})", transactions.len());
        transactions.truncate(100);
    }
    
    Ok(transactions)
}

// Helper function to make RPC calls
async fn call_rpc(rpc_url: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let client = reqwest::Client::new();
    let response = client
        .post(rpc_url)
        .json(&request_body)
        .send()
        .await?;

    let response_body: serde_json::Value = response.json().await?;

    if let Some(error) = response_body.get("error") {
        anyhow::bail!("JSON-RPC Error: {}", error);
    }

    Ok(response_body["result"].clone())
}

async fn analyze_dex_transactions(
    pool_db: &PoolDatabase, 
    transactions: Vec<TransactionInfo>
) -> Result<DexAnalysisResult> {
    
    let mut result = DexAnalysisResult {
        total_transactions: transactions.len(),
        dex_transactions: Vec::new(),
        contract_creations: 0,
        regular_transfers: 0,
        unknown_contracts: 0,
        router_interactions: HashMap::new(),
        pool_interactions: HashMap::new(),
    };

    info!("🔍 Loading known DEX addresses...");
    let known_routers = get_known_dex_routers();
    let known_pools = load_known_pools(pool_db)?;
    
    info!("📊 Analyzing {} transactions...", transactions.len());
    
    for (i, tx) in transactions.iter().enumerate() {
        if i % 10 == 0 && i > 0 {
            info!("📈 Progress: {}/{} transactions analyzed", i, transactions.len());
        }
        
        // Skip contract creations
        if tx.to.is_none() {
            result.contract_creations += 1;
            continue;
        }
        
        let to_address = tx.to.as_ref().unwrap();
        
        // Check if it's a known DEX router
        if let Some(dex_type) = classify_dex_router(to_address, &known_routers) {
            let dex_tx = DexTransaction {
                hash: tx.hash.clone(),
                from: tx.from.clone(),
                to: to_address.clone(),
                dex_type,
                estimated_value_eth: parse_hex_to_eth(&tx.value),
                gas_price_gwei: parse_hex_to_gwei(&tx.gas_price),
            };
            
            *result.router_interactions.entry(to_address.clone()).or_insert(0) += 1;
            result.dex_transactions.push(dex_tx);
            continue;
        }
        
        // Check if it's a known pool address
        if known_pools.contains(to_address) {
            let dex_tx = DexTransaction {
                hash: tx.hash.clone(),
                from: tx.from.clone(),
                to: to_address.clone(),
                dex_type: DexType::DirectPool,
                estimated_value_eth: parse_hex_to_eth(&tx.value),
                gas_price_gwei: parse_hex_to_gwei(&tx.gas_price),
            };
            
            *result.pool_interactions.entry(to_address.clone()).or_insert(0) += 1;
            result.dex_transactions.push(dex_tx);
            continue;
        }
        
        // Check if transaction data looks like DEX interaction
        if is_dex_like_transaction(&tx.data) {
            let dex_tx = DexTransaction {
                hash: tx.hash.clone(),
                from: tx.from.clone(),
                to: to_address.clone(),
                dex_type: DexType::UnknownDex,
                estimated_value_eth: parse_hex_to_eth(&tx.value),
                gas_price_gwei: parse_hex_to_gwei(&tx.gas_price),
            };
            
            result.dex_transactions.push(dex_tx);
            continue;
        }
        
        // Check if it's a simple transfer or unknown contract
        if tx.data == "0x" || tx.data.len() <= 10 {
            result.regular_transfers += 1;
        } else {
            result.unknown_contracts += 1;
        }
    }
    
    info!("✅ Analysis complete!");
    Ok(result)
}

fn get_known_dex_routers() -> HashMap<String, DexType> {
    let mut routers = HashMap::new();
    
    // Uniswap V2
    routers.insert("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".to_lowercase(), DexType::UniswapV2Router);
    
    // Uniswap V3
    routers.insert("0xE592427A0AEce92De3Edee1F18E0157C05861564".to_lowercase(), DexType::UniswapV3Router);
    routers.insert("0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45".to_lowercase(), DexType::UniswapV3Router);
    
    // SushiSwap
    routers.insert("0xd9e1cE17f2641f24aE83637ab66a2cca9C378B9F".to_lowercase(), DexType::SushiSwapRouter);
    
    // 1inch
    routers.insert("0x1111111254EEB25477B68fb85Ed929f73A960582".to_lowercase(), DexType::OneInchRouter);
    routers.insert("0x111111125421cA6dc452d289314280a0f8842A65".to_lowercase(), DexType::OneInchRouter);
    
    // Balancer V2
    routers.insert("0xBA12222222228d8Ba445958a75a0704d566BF2C8".to_lowercase(), DexType::BalancerVault);
    
    routers
}

fn load_known_pools(pool_db: &PoolDatabase) -> Result<std::collections::HashSet<String>> {
    let mut pools = std::collections::HashSet::new();
    
    // Load a sample of pools for performance
    let uniswap_v2_pools = pool_db.get_pools_by_protocol("UniswapV2", 1)?;
    for pool in uniswap_v2_pools.into_iter().take(1000) {
        pools.insert(pool.address.to_lowercase());
    }
    
    let uniswap_v3_pools = pool_db.get_pools_by_protocol("UniswapV3", 1)?;
    for pool in uniswap_v3_pools.into_iter().take(1000) {
        pools.insert(pool.address.to_lowercase());
    }
    
    info!("📊 Loaded {} known pool addresses for matching", pools.len());
    Ok(pools)
}

fn classify_dex_router(address: &str, known_routers: &HashMap<String, DexType>) -> Option<DexType> {
    known_routers.get(&address.to_lowercase()).cloned()
}

fn is_dex_like_transaction(data: &str) -> bool {
    if data.len() < 10 {
        return false;
    }
    
    // Common DEX function selectors
    let dex_selectors = [
        "0x38ed1739", // swapExactTokensForTokens
        "0x8803dbee", // swapTokensForExactTokens
        "0x7ff36ab5", // swapExactETHForTokens
        "0x18cbafe5", // swapExactTokensForETH
        "0xfb3bdb41", // swapETHForExactTokens
        "0x4a25d94a", // swapTokensForExactETH
        "0x414bf389", // exactInputSingle
        "0xc04b8d59", // exactInput
        "0xdb3e2198", // exactOutputSingle
        "0xf28c0498", // exactOutput
        "0xa9059cbb", // transfer
        "0x095ea7b3", // approve
    ];
    
    let selector = &data[..10];
    dex_selectors.contains(&selector)
}

fn parse_hex_to_eth(hex_value: &str) -> f64 {
    if hex_value.starts_with("0x") {
        if let Ok(value) = u64::from_str_radix(&hex_value[2..], 16) {
            return value as f64 / 1e18;
        }
    }
    0.0
}

fn parse_hex_to_gwei(hex_value: &str) -> f64 {
    if hex_value.starts_with("0x") {
        if let Ok(value) = u64::from_str_radix(&hex_value[2..], 16) {
            return value as f64 / 1e9;
        }
    }
    0.0
}

fn print_analysis_results(result: &DexAnalysisResult) {
    info!(""); 
    info!("╔══════════════════════════════════════════════════════════════╗");
    info!("║                    MEMPOOL DEX ANALYSIS                     ║");
    info!("╠══════════════════════════════════════════════════════════════╣");
    info!("║ Total Transactions Analyzed: {:>31} ║", result.total_transactions);
    info!("║ DEX Transactions Found: {:>35} ║", result.dex_transactions.len());
    info!("║ Contract Creations: {:>39} ║", result.contract_creations);
    info!("║ Regular Transfers: {:>40} ║", result.regular_transfers);
    info!("║ Unknown Contracts: {:>40} ║", result.unknown_contracts);
    info!("╚══════════════════════════════════════════════════════════════╝");
    
    if !result.dex_transactions.is_empty() {
        info!("");
        info!("🎯 DEX TRANSACTIONS FOUND:");
        info!("═══════════════════════════════");
        
        for (i, tx) in result.dex_transactions.iter().enumerate() {
            info!("{}. Hash: {}", i + 1, tx.hash);
            info!("   Type: {:?}", tx.dex_type);
            info!("   To: {}", tx.to);
            info!("   Value: {:.4} ETH", tx.estimated_value_eth);
            info!("   Gas Price: {:.2} gwei", tx.gas_price_gwei);
            info!("   ────────────────────────────────");
        }
    }
    
    if !result.router_interactions.is_empty() {
        info!("");
        info!("🔄 ROUTER INTERACTIONS:");
        info!("═══════════════════════");
        for (router, count) in &result.router_interactions {
            info!("   {}: {} transactions", router, count);
        }
    }
    
    if !result.pool_interactions.is_empty() {
        info!("");
        info!("🏊 DIRECT POOL INTERACTIONS:");
        info!("════════════════════════════");
        for (pool, count) in &result.pool_interactions {
            info!("   {}: {} transactions", pool, count);
        }
    }
    
    // Calculate percentages
    let dex_percentage = if result.total_transactions > 0 {
        (result.dex_transactions.len() as f64 / result.total_transactions as f64) * 100.0
    } else {
        0.0
    };
    
    info!("");
    info!("📊 SUMMARY STATISTICS:");
    info!("═══════════════════════");
    info!("   DEX Transaction Rate: {:.2}%", dex_percentage);
    info!("   MEV Opportunity Potential: {} transactions", result.dex_transactions.len());
    
    if !result.dex_transactions.is_empty() {
        let total_value: f64 = result.dex_transactions.iter().map(|tx| tx.estimated_value_eth).sum();
        let gas_prices: Vec<f64> = result.dex_transactions.iter().map(|tx| tx.gas_price_gwei).collect();
        let avg_gas_price: f64 = gas_prices.iter().sum::<f64>() / gas_prices.len() as f64;
        
        info!("   Total DEX Volume: {:.4} ETH", total_value);
        info!("   Average Gas Price: {:.2} gwei", avg_gas_price);
    }
    
    info!("");
    info!("🎉 Analysis complete! Found {} potential MEV opportunities", result.dex_transactions.len());
}