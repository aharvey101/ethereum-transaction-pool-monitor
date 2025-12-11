use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing router detection...");
    
    let pool_db = PoolDatabase::new("./dex_pools.db")?;
    
    // Test known Uniswap V2 Router 
    let uniswap_v2 = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d";
    println!("Uniswap V2 Router ({}): {}", uniswap_v2, pool_db.is_dex_router(uniswap_v2));
    
    // Test known Uniswap V3 SwapRouter
    let uniswap_v3 = "0xe592427a0aece92de3edee1f18e0157c05861564";  
    println!("Uniswap V3 Router ({}): {}", uniswap_v3, pool_db.is_dex_router(uniswap_v3));
    
    // Test USDC (token, should be false)
    let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    println!("USDC Token ({}): {}", usdc, pool_db.is_dex_router(usdc));
    
    // Test 1inch Router
    let one_inch = "0x1111111254eeb25477b68fb85ed929f73a960582";
    println!("1inch Router ({}): {}", one_inch, pool_db.is_dex_router(one_inch));
    
    Ok(())
}
