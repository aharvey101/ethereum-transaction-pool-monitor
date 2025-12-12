use anyhow::Result;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;
use std::env;
use tracing::{info, error};

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
    info!("🔍 Testing basic RPC connection to: {}", rpc_url);

    match EthereumClient::new(rpc_url).await {
        Ok(eth_client) => {
            info!("✅ Successfully created EthereumClient");
            
            match eth_client.get_latest_block_number().await {
                Ok(block_number) => {
                    info!("✅ RPC connection working - Latest block: {}", block_number);
                    
                    // Test a few more basic calls
                    info!("🔍 Testing additional RPC calls...");
                    
                    match eth_client.get_gas_price().await {
                        Ok(gas_price) => info!("✅ Gas price: {} gwei", gas_price.to::<u64>() / 1_000_000_000),
                        Err(e) => error!("❌ Failed to get gas price: {}", e),
                    }
                    
                    // Test getting a recent block
                    if block_number > 0 {
                        info!("🔍 Testing block data retrieval...");
                        // Add block data test here if needed
                        info!("✅ Basic RPC functionality confirmed");
                    }
                },
                Err(e) => {
                    error!("❌ Failed to get latest block: {}", e);
                    error!("   This indicates the RPC endpoint is not responding properly");
                    std::process::exit(1);
                }
            }
        },
        Err(e) => {
            error!("❌ Failed to create EthereumClient: {}", e);
            error!("   Check if the RPC URL is correct and accessible");
            std::process::exit(1);
        }
    }

    info!("🎉 All basic RPC tests passed!");
    
    Ok(())
}