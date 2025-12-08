//! Test DEX Subgraphs
//! 
//! This binary allows testing different subgraph IDs for various DEX protocols
//! to find the correct ones for SushiSwap, Curve, and other DEXs.

use anyhow::Result;
use ethereum_transaction_pool_monitor::{graph_client::GraphClient, pool_db::PoolDatabase};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("🧪 DEX Subgraph Testing");
    println!("======================");
    println!("Testing various subgraph IDs for SushiSwap and Curve");
    println!();

    let db_path = "test_dex.db";
    let pool_db = PoolDatabase::new(db_path)?;
    
    // Create Graph client with API key
    let api_key = "79942a724597827e4cb8972667c0a355".to_string();
    let mut graph_client = GraphClient::new(api_key);
    
    // Test SushiSwap subgraph IDs (common patterns)
    let sushiswap_test_ids = vec![
        "4bb33bea5ac4c4098902bcfc53a09da46b3a0e9e5c3e1b98ac21cf9da2bb8d8a", // Example ID
        "Ce5d2C78d7D4F1d5ca453b89D6A4e0d8e8B1D3F5d8e8a2c1f7a9B1E4D8C3A6F2", // Example ID
        "0x4bb33bea5ac4c4098902bcfc53a09da46b3a0e9e5c3e1b98ac21cf9da2bb8d8a", // Example ID with 0x
        // Add more candidate IDs here
    ];
    
    // Test Curve subgraph IDs (common patterns)
    let curve_test_ids = vec![
        "D8F1F8f5b8E2A1c9B7e5D4c8F2e9B1A5c8E6D9F3c1A8b5E2d9C4f8A1B3E6D9", // Example ID
        "3c1A8b5E2d9C4f8A1B3E6D9F1F8f5b8E2A1c9B7e5D4c8F2e9B1A5c8E6D9F3", // Example ID
        // Add more candidate IDs here
    ];

    println!("🍣 Testing SushiSwap subgraph IDs...");
    println!("===================================");
    
    for (i, subgraph_id) in sushiswap_test_ids.iter().enumerate() {
        println!("🔍 Testing SushiSwap ID {}/{}: {}", i + 1, sushiswap_test_ids.len(), subgraph_id);
        
        // Test basic query to see if subgraph exists and responds
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/{}",
            graph_client.api_key, subgraph_id
        );
        
        let test_query = r#"
            query {
                pairs(first: 1) {
                    id
                    token0 { id symbol }
                    token1 { id symbol }
                }
            }
        "#;
        
        match graph_client.execute_query::<serde_json::Value>(&endpoint, test_query).await {
            Ok(result) => {
                println!("✅ SushiSwap subgraph {} WORKS! Found data: {:?}", subgraph_id, result);
                println!("🎉 Use this subgraph ID for SushiSwap: {}", subgraph_id);
                break;
            }
            Err(e) => {
                println!("❌ SushiSwap subgraph {} failed: {}", subgraph_id, e);
            }
        }
        
        // Small delay between tests
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    
    println!();
    println!("🌀 Testing Curve subgraph IDs...");
    println!("================================");
    
    for (i, subgraph_id) in curve_test_ids.iter().enumerate() {
        println!("🔍 Testing Curve ID {}/{}: {}", i + 1, curve_test_ids.len(), subgraph_id);
        
        let endpoint = format!(
            "https://gateway.thegraph.com/api/{}/subgraphs/id/{}",
            graph_client.api_key, subgraph_id
        );
        
        let test_query = r#"
            query {
                pools(first: 1) {
                    id
                    coins { id symbol }
                    name
                }
            }
        "#;
        
        match graph_client.execute_query::<serde_json::Value>(&endpoint, test_query).await {
            Ok(result) => {
                println!("✅ Curve subgraph {} WORKS! Found data: {:?}", subgraph_id, result);
                println!("🎉 Use this subgraph ID for Curve: {}", subgraph_id);
                break;
            }
            Err(e) => {
                println!("❌ Curve subgraph {} failed: {}", subgraph_id, e);
            }
        }
        
        // Small delay between tests
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    
    println!();
    println!("🔍 Testing completed!");
    println!("💡 If you find working subgraph IDs, add them to the main collection process.");
    println!("📚 You can also search for subgraphs at: https://thegraph.com/explorer/");

    Ok(())
}