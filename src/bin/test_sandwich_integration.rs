/// Test binary for sandwich pool integration
/// 
/// This demonstrates how to use the 545k+ pool database for sandwich attack
/// simulation and analysis. Shows real pool data integration capabilities.

use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🥪 Sandwich Pool Integration Test");
    println!("===============================================");

    // Initialize pool database directly for basic functionality
    let pool_db = PoolDatabase::new("./dex_pools.db")?;
    
    // Get database statistics
    println!("\n📊 Pool Database Statistics:");
    let total_pools = pool_db.pool_count()?;
    let uniswap_v2_count = pool_db.get_pool_count_by_protocol("UniswapV2")?;
    let uniswap_v3_count = pool_db.get_pool_count_by_protocol("UniswapV3")?;
    let sushiswap_count = pool_db.get_pool_count_by_protocol("SushiSwap")?;
    let curve_count = pool_db.get_pool_count_by_protocol("Curve")?;
    
    println!("   Total Pools: {}", total_pools);
    println!("   Uniswap V2:  {}", uniswap_v2_count);
    println!("   Uniswap V3:  {}", uniswap_v3_count);
    println!("   SushiSwap:   {}", sushiswap_count);
    println!("   Curve:       {}", curve_count);
    
    // Get sample pools from each protocol
    println!("\n🎯 Sample High-Liquidity Pools by Protocol:");
    
    for protocol in &["UniswapV2", "UniswapV3", "SushiSwap", "Curve"] {
        let pools = pool_db.get_pools_by_protocol(protocol, 1)?;
        println!("\n   {} Pools (showing first 3):", protocol);
        
        for (i, pool) in pools.iter().take(3).enumerate() {
            println!("     {}. {}", i + 1, pool.address);
            if let (Some(token0), Some(token1)) = (&pool.token0, &pool.token1) {
                println!("        Pair: {} / {}", token0, token1);
            }
        }
        
        if pools.len() > 3 {
            println!("        ... and {} more", pools.len() - 3);
        }
    }
    
    // Demonstrate pool lookup functionality
    println!("\n🔍 Pool Lookup Examples:");
    
    // Test with known Uniswap V2 pools
    let test_pools = [
        "0x0d4a11d5eeaac28ec3f61d100daf4d40471f1852", // ETH/USDT V2
        "0xb4e16d0168e52d35cacd2c6185b44281ec28c9dc", // USDC/ETH V2  
        "0xa478c2975ab1ea89e8196811f51a7b7ade33eb11", // DAI/ETH V2
    ];
    
    for pool_addr in &test_pools {
        if let Some(pool) = pool_db.get_pool(pool_addr, 1)? {
            println!("   ✅ Found: {} ({})", pool.address, pool.protocol);
        } else {
            println!("   ❌ Not found: {}", pool_addr);
        }
    }
    
    // Show how this data can be used for sandwich analysis
    println!("\n🥪 Sandwich Attack Analysis Potential:");
    println!("   ✅ Pool identification: {} total pools available", total_pools);
    println!("   ✅ Protocol support: UniswapV2, UniswapV3, SushiSwap, Curve");
    println!("   ✅ Token pair information available for liquidity analysis");
    println!("   ✅ Pool addresses can be used for real-time reserve queries");
    
    // Demonstrate DeFi activity detection
    println!("\n🔍 DeFi Activity Detection:");
    let test_addresses = [
        ("0x7a250d5630b4cf539739df2c5dacb4c659f2488d", "Uniswap V2 Router"),
        ("0xe592427a0aece92de3edee1f18e0157c05861564", "Uniswap V3 SwapRouter"), 
        ("0xd9e1ce17f2641f24ae9f7ffe6ff87d78ef7b26c1", "SushiSwap Router"),
        ("0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", "USDC Token"),
    ];
    
    for (addr, name) in &test_addresses {
        let activity_type = pool_db.get_defi_activity_type(addr, 1)?;
        println!("   {}: {:?}", name, activity_type);
    }

    // Test real-time pool state fetching (if network available)
    println!("\n🌐 Real-time Pool State Integration Test:");
    match test_real_time_fetching().await {
        Ok(_) => {
            println!("   ✅ Real-time pool state fetching successful!");
            println!("   ✅ Live reserve data available for REVM simulation");
        },
        Err(e) => {
            println!("   ⚠️  Real-time fetching unavailable: {}", e);
            println!("   ℹ️  Integration framework ready, requires network connection");
        }
    }
    
    println!("\n🚀 Integration Foundation Complete!");
    println!("   This database provides the foundation for:");
    println!("   → Real-time pool state querying with Alloy");
    println!("   → Liquidity analysis for sandwich targeting");
    println!("   → Multi-DEX protocol support (V2, V3, SushiSwap, Curve)");
    println!("   → Transaction classification for mempool monitoring");
    println!("   → Price impact calculations for profit estimation");
    println!("\n   Next: Integrate with REVM simulation for accurate profit calculation");

    Ok(())
}

/// Test real-time pool state fetching functionality
async fn test_real_time_fetching() -> Result<()> {
    use ethereum_transaction_pool_monitor::pool_state_fetcher::PoolStateFetcher;
    
    // Try to connect to the user's local Ethereum node
    let fetcher = PoolStateFetcher::new("http://192.168.0.14:8545").await?;
    
    // Test with a known Uniswap V2 ETH/USDT pool
    let pool_address = "0x0d4a11d5eeaac28ec3f61d100daf4d40471f1852".parse()?;
    let pool_state = fetcher.fetch_pool_state(pool_address, "UniswapV2").await?;
    
    println!("   📊 Live Pool Data:");
    println!("      Pool: {}", pool_state.address);
    println!("      Protocol: {}", pool_state.protocol);
    println!("      Token0: {}", pool_state.token0);
    println!("      Token1: {}", pool_state.token1);
    println!("      Reserve0: {}", pool_state.reserve0);
    println!("      Reserve1: {}", pool_state.reserve1);
    println!("      Liquidity: ${:.0}", pool_state.total_liquidity_usd);
    println!("      Block: {}", pool_state.block_number);
    
    Ok(())
}