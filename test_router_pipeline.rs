// Test script to verify complete router transaction pipeline
use ethereum_transaction_pool_monitor::{
    pool_db::PoolDb,
    transaction_decoder::TransactionDecoder,
    eth_client::Transaction,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Testing Router Transaction Pipeline");
    
    // Initialize database
    let pool_db = PoolDb::new("database.sqlite3").expect("Failed to load pool database");
    let decoder = TransactionDecoder::new();
    
    // Test 1: Verify router detection
    println!("\n📍 Test 1: Router Detection");
    let uniswap_v2_router = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d";
    let uniswap_v3_router = "0xe592427a0aece92de3edee1f18e0157c05861564";
    let random_address = "0x1234567890123456789012345678901234567890";
    
    println!("   Uniswap V2 Router: {}", pool_db.is_dex_router(uniswap_v2_router));
    println!("   Uniswap V3 Router: {}", pool_db.is_dex_router(uniswap_v3_router));
    println!("   Random Address: {}", pool_db.is_dex_router(random_address));
    
    // Test 2: Create a mock Uniswap V2 swap transaction
    println!("\n📍 Test 2: Mock Router Transaction Processing");
    
    // Mock transaction data for swapExactTokensForTokens
    // Function signature: swapExactTokensForTokens(uint256,uint256,address[],address,uint256)
    // Selector: 0x38ed1739
    let mock_input = "0x38ed1739000000000000000000000000000000000000000000000000de0b6b3a7640000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000012345678901234567890123456789012345678900000000000000000000000000000000000000000000000000000000065a4c8000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000a0b86a33e6e6cd12bbea3c80e5b4e4e2b6c5e987000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2";
    
    let mock_transaction = Transaction {
        hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        to: Some(uniswap_v2_router.to_string()),
        from: "0xabcd1234567890abcdef1234567890abcdef1234".to_string(),
        input: mock_input.to_string(),
        value: "0x0".to_string(),
        gas: "0x5208".to_string(),
        gas_price: "0x3b9aca00".to_string(),
        nonce: "0x1".to_string(),
    };
    
    // Test router detection
    let is_router = if let Some(ref to) = mock_transaction.to {
        pool_db.is_dex_router(to)
    } else {
        false
    };
    println!("   Router detected: {}", is_router);
    
    if is_router {
        // Test input parsing
        if let Some(pool_info) = decoder.parse_router_target_pool(&mock_transaction.input) {
            println!("   ✅ Router input parsed successfully!");
            println!("   Token A: {}", pool_info.token_a);
            println!("   Token B: {}", pool_info.token_b);
            
            // Test pool lookup
            match pool_db.find_pool_by_tokens(&pool_info.token_a, &pool_info.token_b, 1) {
                Ok(Some(pool)) => {
                    println!("   ✅ Pool found in database!");
                    println!("   Pool Address: {}", pool.address);
                    println!("   DEX: {}", pool.dex_name);
                    println!("   Fee: {}", pool.fee);
                },
                Ok(None) => {
                    println!("   ⚠️  No pool found for token pair");
                    
                    // Show some available pools for reference
                    let conn = pool_db.conn.lock().unwrap();
                    let mut stmt = conn.prepare("SELECT address, token_a, token_b, dex_name FROM pools LIMIT 5")?;
                    let pool_iter = stmt.query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })?;
                    
                    println!("   Sample pools in database:");
                    for pool in pool_iter {
                        let (addr, token_a, token_b, dex) = pool?;
                        println!("   - {} ({}) {} <-> {}", addr, dex, token_a, token_b);
                    }
                },
                Err(e) => println!("   ❌ Error looking up pool: {}", e),
            }
        } else {
            println!("   ❌ Failed to parse router input");
        }
    }
    
    // Test 3: Check database statistics
    println!("\n📍 Test 3: Database Statistics");
    let conn = pool_db.conn.lock().unwrap();
    
    let pool_count: i32 = conn.query_row("SELECT COUNT(*) FROM pools", [], |row| {
        Ok(row.get(0)?)
    })?;
    println!("   Total pools in database: {}", pool_count);
    
    let dex_breakdown: Result<Vec<(String, i32)>, _> = conn.prepare("SELECT dex_name, COUNT(*) FROM pools GROUP BY dex_name ORDER BY COUNT(*) DESC")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    
    match dex_breakdown {
        Ok(dexes) => {
            println!("   DEX breakdown:");
            for (dex, count) in dexes {
                println!("   - {}: {} pools", dex, count);
            }
        },
        Err(e) => println!("   Error getting DEX breakdown: {}", e),
    }
    
    println!("\n🎉 Pipeline test complete!");
    Ok(())
}