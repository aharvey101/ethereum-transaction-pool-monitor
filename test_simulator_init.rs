/// Test file to isolate EnhancedSandwichSimulator initialization hang
use anyhow::Result;
use tokio;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 Testing EnhancedSandwichSimulator initialization...");
    
    let rpc_url = "http://192.168.0.14:8545";
    let db_path = "./database.sqlite3";
    
    println!("1. Testing EthereumClient creation...");
    let eth_client = match ethereum_transaction_pool_monitor::eth_client::EthereumClient::new(rpc_url).await {
        Ok(client) => {
            println!("✅ EthereumClient created successfully");
            client
        }
        Err(e) => {
            println!("❌ EthereumClient failed: {}", e);
            return Err(e);
        }
    };
    
    println!("2. Testing PoolStateFetcher creation...");
    let _pool_fetcher = match ethereum_transaction_pool_monitor::pool_state_fetcher::PoolStateFetcher::new(rpc_url).await {
        Ok(fetcher) => {
            println!("✅ PoolStateFetcher created successfully");
            fetcher
        }
        Err(e) => {
            println!("❌ PoolStateFetcher failed: {}", e);
            return Err(e);
        }
    };
    
    println!("3. Testing SandwichPoolIntegration creation...");
    let _pool_integration = match ethereum_transaction_pool_monitor::sandwich_pool_integration::SandwichPoolIntegration::new(
        db_path, 
        eth_client, 
        rpc_url
    ).await {
        Ok(integration) => {
            println!("✅ SandwichPoolIntegration created successfully");
            integration
        }
        Err(e) => {
            println!("❌ SandwichPoolIntegration failed: {}", e);
            return Err(e);
        }
    };
    
    println!("4. Testing full EnhancedSandwichSimulator creation...");
    let eth_client2 = ethereum_transaction_pool_monitor::eth_client::EthereumClient::new(rpc_url).await?;
    let _simulator = match ethereum_transaction_pool_monitor::enhanced_revm_simulator::EnhancedSandwichSimulator::new(
        db_path,
        rpc_url,
        eth_client2,
        None
    ).await {
        Ok(sim) => {
            println!("✅ EnhancedSandwichSimulator created successfully!");
            sim
        }
        Err(e) => {
            println!("❌ EnhancedSandwichSimulator failed: {}", e);
            return Err(e);
        }
    };
    
    println!("🎉 All components initialized successfully!");
    Ok(())
}