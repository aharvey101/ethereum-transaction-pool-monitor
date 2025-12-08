use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool_db = PoolDatabase::new("dex_pools.db")?;
    
    println!("🔍 Analyzing transaction classification...\n");
    
    // Test some common addresses that appear in mempool
    let test_addresses = [
        ("0xdac17f958d2ee523a2206206994597c13d831ec7", "USDT Token Contract"),
        ("0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", "USDC Token Contract"), 
        ("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "WETH Token Contract"),
        ("0x7a250d5630b4cf539739df2c5dacb4c659f2488d", "Uniswap V2 Router"),
        ("0xe592427a0aece92de3edee1f18e0157c05861564", "Uniswap V3 Router"),
        ("0x1111111254eeb25477b68fb85ed929f73a960582", "1inch Router"),
        ("0xdef1c0ded9bec7f1a1670819833240f027b25eff", "0x Protocol Router"),
    ];
    
    for (address, description) in test_addresses {
        let is_token = pool_db.is_token_contract(address);
        let is_router = pool_db.is_dex_router(address);
        let activity_type = pool_db.get_defi_activity_type(address, 1).unwrap_or_else(|_| 
            ethereum_transaction_pool_monitor::eth_client::DefiActivityType::None
        );
        
        println!("📍 {}", description);
        println!("   Address: {}", address);
        println!("   Token Contract: {}", is_token);
        println!("   Router: {}", is_router);
        println!("   Activity Type: {:?}", activity_type);
        println!("   🔄 Will attempt swap decode: {}", is_router);
        println!();
    }
    
    println!("📊 Summary:");
    println!("• Token transfers (USDT/USDC/WETH) → No swap decoding");
    println!("• Router swaps (Uniswap/1inch) → Swap decoding attempted");
    println!("• Most mempool traffic = token transfers");
    println!("• Only router transactions will show amounts");
    
    Ok(())
}