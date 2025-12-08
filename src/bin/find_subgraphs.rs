//! Subgraph ID finder utility
//! Tests various subgraph IDs to find working ones for SushiSwap and Curve

use anyhow::Result;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = "79942a724597827e4cb8972667c0a355";
    
    // Known SushiSwap subgraph IDs to try - these are researched candidates
    let sushiswap_candidates = vec![
        "4ayKa5TJJamZK7vm74FEMHn32fPPFGJYSy7iGbg6YMPp",      // SushiSwap Exchange candidate 1
        "ELUcwgpm14LKPLrBRuVvPvNKHQ9HvwmtKgKSH6123cr7",      // SushiSwap Exchange candidate 2
        "4p9VJUh4CKczRQRXs2kfQzZMGSsAWyB6F3mf3Ye1TZ8v",      // SushiSwap V2 candidate
    ];
    
    // Known Curve subgraph IDs to try - these are researched candidates  
    let curve_candidates = vec![
        "H9oPAbBCBfaKnmwbr3YbcAWqp35FgHMVhDNpPkDN8MQb",      // Curve Finance candidate 1
        "3fy7TkkYXxNzPa7GVV2qSNUoD11YF1LMdKJhzGZ8qWnV",      // Curve Finance candidate 2
    ];
    
    println!("🔍 Testing SushiSwap subgraph candidates...");
    for (i, subgraph_id) in sushiswap_candidates.iter().enumerate() {
        println!("📡 Testing SushiSwap candidate {}: {}", i + 1, subgraph_id);
        match test_subgraph(api_key, subgraph_id, "pairs").await {
            Ok(success) => {
                if success {
                    println!("✅ SushiSwap candidate {} WORKS: {}", i + 1, subgraph_id);
                } else {
                    println!("❌ SushiSwap candidate {} failed", i + 1);
                }
            }
            Err(e) => {
                println!("❌ SushiSwap candidate {} error: {}", i + 1, e);
            }
        }
        println!();
    }
    
    println!("🔍 Testing Curve subgraph candidates...");
    for (i, subgraph_id) in curve_candidates.iter().enumerate() {
        println!("📡 Testing Curve candidate {}: {}", i + 1, subgraph_id);
        match test_subgraph(api_key, subgraph_id, "pools").await {
            Ok(success) => {
                if success {
                    println!("✅ Curve candidate {} WORKS: {}", i + 1, subgraph_id);
                } else {
                    println!("❌ Curve candidate {} failed", i + 1);
                }
            }
            Err(e) => {
                println!("❌ Curve candidate {} error: {}", i + 1, e);
            }
        }
        println!();
    }
    
    Ok(())
}

async fn test_subgraph(api_key: &str, subgraph_id: &str, entity_type: &str) -> Result<bool> {
    let client = reqwest::Client::new();
    let endpoint = format!(
        "https://gateway.thegraph.com/api/{}/subgraphs/id/{}",
        api_key, subgraph_id
    );
    
    let query = format!(r#"
        query {{
            {}(first: 1) {{
                id
            }}
        }}
    "#, entity_type);
    
    let body = json!({ "query": query });
    
    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    
    if response.status().is_success() {
        let json: serde_json::Value = response.json().await?;
        if json["errors"].is_null() && json["data"][entity_type].is_array() {
            let results = json["data"][entity_type].as_array().unwrap();
            return Ok(!results.is_empty());
        }
    }
    
    Ok(false)
}