/// Test script to verify the new dynamic gas price fetching
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    println!("🔍 Testing Dynamic Gas Price Fetching");
    
    // You can change this to your preferred RPC URL
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://eth.llamarpc.com".to_string());
    
    println!("📡 Connecting to: {}", rpc_url);
    
    // Create Ethereum client
    let eth_client = EthereumClient::new(&rpc_url).await?;
    
    // Test legacy gas price
    println!("\n📊 Legacy Gas Price (eth_gasPrice):");
    match eth_client.get_gas_price().await {
        Ok(gas_price) => {
            let gas_price_gwei = gas_price.to_string().parse::<u128>().unwrap_or(0) as f64 / 1e9;
            println!("   Legacy: {:.4} gwei ({} wei)", gas_price_gwei, gas_price);
        }
        Err(e) => println!("   Error: {}", e),
    }
    
    // Test current base fee
    println!("\n📊 Current Base Fee (EIP-1559):");
    match eth_client.get_current_base_fee().await {
        Ok(base_fee) => {
            println!("   Base Fee: {:.4} gwei", base_fee);
        }
        Err(e) => println!("   Error: {}", e),
    }
    
    // Test our new combined gas price
    println!("\n📊 New Dynamic Gas Price:");
    match eth_client.get_current_gas_price_wei().await {
        Ok(gas_price_wei) => {
            let gas_price_gwei = gas_price_wei as f64 / 1e9;
            println!("   Dynamic: {:.4} gwei ({} wei)", gas_price_gwei, gas_price_wei);
        }
        Err(e) => println!("   Error: {}", e),
    }
    
    // Test the fallback method
    println!("\n📊 Fallback Gas Price Method:");
    match eth_client.get_current_gas_price().await {
        Ok(gas_price) => {
            let gas_price_gwei = gas_price.to_string().parse::<u128>().unwrap_or(0) as f64 / 1e9;
            println!("   Fallback: {:.4} gwei ({} wei)", gas_price_gwei, gas_price);
        }
        Err(e) => println!("   Error: {}", e),
    }
    
    // Calculate cost estimates with different scenarios
    println!("\n💰 Gas Cost Estimates:");
    let current_gas_price = eth_client.get_current_gas_price_wei().await.unwrap_or(500_000_000); // 0.5 gwei fallback
    
    let scenarios = [
        ("Simple swap", 150_000u64),
        ("Complex swap", 180_000u64),
        ("Sandwich attack", 400_000u64),
        ("Old estimate", 550_000u64),
    ];
    
    for (name, gas_limit) in scenarios.iter() {
        let cost_wei = gas_limit * current_gas_price;
        let cost_eth = cost_wei as f64 / 1e18;
        let cost_usd = cost_eth * 3200.0; // Assume $3200 ETH
        println!("   {}: {} gas × {:.1} gwei = {:.6} ETH (${:.2})", 
                name, gas_limit, current_gas_price as f64 / 1e9, cost_eth, cost_usd);
    }
    
    println!("\n✅ Gas price testing completed!");
    Ok(())
}