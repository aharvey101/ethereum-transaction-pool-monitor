use anyhow::Result;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("🔍 Debug: Block Number Detection Test...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    println!("📡 RPC URL: {}", rpc_url);
    
    let client = EthereumClient::new(&rpc_url).await?;
    
    println!("🔢 Testing block number fetching...\n");
    
    for i in 1..=5 {
        let start = std::time::Instant::now();
        match client.get_latest_block_number().await {
            Ok(block_number) => {
                let duration = start.elapsed();
                println!("✅ Attempt {}: Block #{} (fetched in {:?})", i, block_number, duration);
            }
            Err(e) => {
                println!("❌ Attempt {}: Error - {}", i, e);
            }
        }
        
        if i < 5 {
            println!("   ⏳ Waiting 3 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }
    
    println!("\n🔍 Observations:");
    println!("   • If all attempts show the same block number, no new blocks were mined during test");
    println!("   • If there are errors, there's an RPC connectivity issue");  
    println!("   • If block numbers increase, block detection is working");
    
    Ok(())
}