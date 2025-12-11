use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
use ethereum_transaction_pool_monitor::eth_client::{EthClient, MempoolTransaction};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Manual DEX Detection Test");
    
    let pool_db = PoolDatabase::new("database.sqlite3")?;
    let eth_client = EthClient::new("ws://192.168.0.14:8547", &pool_db).await?;
    
    // Test router address detection
    let uniswap_v2 = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d";
    let uniswap_v3 = "0xe592427a0aece92de3edee1f18e0157c05861564";
    let sushiswap = "0xd9e1ce17f2641f24ae83637ab66a2cca9c378b9f";
    let random_addr = "0x1234567890123456789012345678901234567890";
    
    println!("Router Detection Tests:");
    println!("  Uniswap V2 ({}): {}", uniswap_v2, pool_db.is_dex_router(uniswap_v2));
    println!("  Uniswap V3 ({}): {}", uniswap_v3, pool_db.is_dex_router(uniswap_v3));
    println!("  SushiSwap ({}): {}", sushiswap, pool_db.is_dex_router(sushiswap));
    println!("  Random Address ({}): {}", random_addr, pool_db.is_dex_router(random_addr));
    
    // Create a mock DEX transaction to test the detection pipeline
    println!("\n🔄 Testing Transaction Detection Pipeline:");
    let mock_tx = MempoolTransaction {
        from: "0x742d35cc6634C0532925a3b8D3Ac0cfB5c5c5c5c".to_string(),
        to: Some(uniswap_v2.to_string()), // Point to Uniswap V2 router
        value: "0x16345785d8a0000".to_string(), // 0.1 ETH
        gas: "0x493e0".to_string(), // 300,000 gas
        gas_price: "0x4a817c800".to_string(), // 20 gwei
        data: "0x7ff36ab5".to_string(), // swapExactETHForTokens function signature
        value_f64: 0.1, // Pre-computed
        gas_price_f64: 20.0, // Pre-computed
    };
    
    println!("Mock Transaction Details:");
    println!("  From: {}", mock_tx.from);
    println!("  To: {:?}", mock_tx.to);
    println!("  Value: {} ETH", mock_tx.value_f64);
    println!("  Gas Price: {} gwei", mock_tx.gas_price_f64);
    println!("  To Router: {}", pool_db.is_dex_router(&mock_tx.to.as_ref().unwrap()));
    println!("  Estimated USD Value: ${:.2}", mock_tx.value_f64 * 2000.0);
    
    println!("\n✅ Manual test complete!");
    Ok(())
}