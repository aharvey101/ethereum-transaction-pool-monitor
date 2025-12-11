fn main() {
    use ethereum_transaction_pool_monitor::pool_db::PoolDatabase;
    
    let pool_db = PoolDatabase::new("./dex_pools.db").unwrap();
    
    // Test Uniswap V2 Router
    let uniswap_v2 = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d";
    println!("Uniswap V2 Router: {}", pool_db.is_dex_router(uniswap_v2));
    
    // Test USDC (should be false)
    let usdc = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
    println!("USDC: {}", pool_db.is_dex_router(usdc));
    
    // Test 1inch Router  
    let one_inch = "0x1111111254fb6c44bac0bed2854e76f90643097d";
    println!("1inch Router: {}", pool_db.is_dex_router(one_inch));
}
