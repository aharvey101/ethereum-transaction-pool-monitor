// Simple pool fetch test - using a simpler approach
use anyhow::Result;
use alloy::providers::{ProviderBuilder, Provider};
use alloy::rpc::types::eth::Filter;
use alloy_primitives::Address;
use std::str::FromStr;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup simple logging
    tracing_subscriber::fmt::init();
    
    println!("🔍 Testing V3 factory and events...");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    println!("📡 RPC URL: {}", rpc_url);
    
    // Create provider directly
    let provider = ProviderBuilder::new()
        .on_http(rpc_url.parse()?);
    
    // Get current block
    let current_block = provider.get_block_number().await?;
    println!("📊 Current block: {}", current_block);
    
    const UNISWAP_V3_FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
    let factory_addr = Address::from_str(UNISWAP_V3_FACTORY)?;
    
    // Check if contract exists at this address
    println!("🔍 Checking if V3 factory contract exists at {}...", UNISWAP_V3_FACTORY);
    match provider.get_code_at(factory_addr).await {
        Ok(code) => {
            if code.is_empty() {
                println!("❌ No contract found at V3 factory address!");
            } else {
                println!("✅ Contract exists at V3 factory (code length: {})", code.len());
            }
        }
        Err(e) => {
            println!("❌ Error checking contract: {}", e);
        }
    }
    
    // Try with a more recent range where we know V3 pools exist
    println!("\n🔍 Let me try a larger, more recent range...");
    let from_block = current_block.saturating_sub(100_000); // Last 100k blocks
    let to_block = current_block;
    
    println!("🟣 Testing V3 PoolCreated events in blocks {}-{}", from_block, to_block);
    
    let event_filter = Filter::new()
        .from_block(from_block)
        .to_block(to_block)
        .address(vec![factory_addr])
        .event("PoolCreated(address,address,uint24,int24,address)");
    
    println!("🔍 Querying recent logs...");
    match provider.get_logs(&event_filter).await {
        Ok(logs) => {
            println!("✅ Found {} PoolCreated events in last 100k blocks", logs.len());
            for (i, log) in logs.iter().take(3).enumerate() {
                println!("  Event {}: topics={:?}", i, log.topics());
            }
        }
        Err(e) => {
            println!("❌ Error querying logs: {}", e);
        }
    }
    
    // Also try checking existing pools in database by making a direct query
    println!("\n🔍 Let me also check if we can query a known V3 pool directly...");
    
    // ETH/USDC 0.3% pool (this should definitely exist)
    const ETH_USDC_V3_POOL: &str = "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"; 
    let pool_addr = Address::from_str(ETH_USDC_V3_POOL)?;
    
    match provider.get_code_at(pool_addr).await {
        Ok(code) => {
            if code.is_empty() {
                println!("❌ No contract at ETH/USDC V3 pool address");
            } else {
                println!("✅ ETH/USDC V3 pool exists (code length: {})", code.len());
            }
        }
        Err(e) => {
            println!("❌ Error checking ETH/USDC pool: {}", e);
        }
    }
    
    Ok(())
}