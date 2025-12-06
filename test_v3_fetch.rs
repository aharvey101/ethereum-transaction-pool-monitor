use alloy::providers::{ProviderBuilder, Provider};
use alloy::rpc::types::eth::Filter;
use alloy_primitives::Address;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea3113F";
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());

    println!("Testing V3 pool fetching with RPC: {}", rpc_url);

    // Create provider
    let provider = ProviderBuilder::new()
        .on_http(rpc_url.parse()?);

    // Get current block
    let current_block = provider.get_block_number().await?;
    println!("Current block: {}", current_block);

    // Test filter creation and logs query
    let from_block = current_block - 5000;
    let to_block = current_block;

    println!("Querying blocks {}-{}", from_block, to_block);

    let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
    println!("Factory address: {:?}", factory_addr);

    println!("Creating filter...");
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PoolCreated(address,address,uint24,int24,address)");

    println!("Filter created successfully");

    println!("Calling provider.get_logs()...");
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            println!("Success! Found {} logs", logs.len());
        }
        Err(e) => {
            println!("Error: {:?}", e);
            println!("Error string: {}", e);
        }
    }

    Ok(())
}
