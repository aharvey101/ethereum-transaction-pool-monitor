use anyhow::Result;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let rpc_url = "http://192.168.0.14:8545";
    let client = reqwest::Client::new();
    
    // Get pending transactions
    let request_body = json!({
        "jsonrpc": "2.0",
        "method": "txpool_content",
        "params": [],
        "id": 1
    });

    let response = client
        .post(rpc_url)
        .json(&request_body)
        .send()
        .await?;

    let response_body: serde_json::Value = response.json().await?;
    
    if let Some(pending) = response_body
        .get("result")
        .and_then(|r| r.get("pending"))
        .and_then(|p| p.as_object())
    {
        let mut to_addresses = std::collections::HashSet::new();
        for (_from, nonce_map) in pending {
            if let Some(txs) = nonce_map.as_object() {
                for (_nonce, tx) in txs {
                    if let Some(to) = tx.get("to").and_then(|t| t.as_str()) {
                        to_addresses.insert(to.to_lowercase());
                    }
                }
            }
        }
        
        println!("Sample 'to' addresses from pending transactions:");
        for (i, addr) in to_addresses.iter().take(10).enumerate() {
            println!("{}: {}", i + 1, addr);
        }
    }
    
    Ok(())
}
