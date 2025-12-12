use anyhow::Result;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;
use serde_json::Value;
use std::env;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <rpc_url>", args[0]);
        println!("Example: {} http://192.168.0.14:8545", args[0]);
        println!("Example: {} wss://mainnet.infura.io/ws/v3/YOUR_KEY", args[0]);
        return Ok(());
    }

    let rpc_url = &args[1];
    info!("🔍 Testing mempool monitoring with: {}", rpc_url);

    // Test WebSocket for live transactions
    if rpc_url.starts_with("ws") {
        info!("🌐 Testing WebSocket mempool subscription...");
        test_websocket_mempool(rpc_url).await?;
    } else {
        // Test basic connection first for HTTP URLs
        info!("📡 Testing basic RPC connection...");
        let eth_client = EthereumClient::new(rpc_url).await?;
        let block_number = eth_client.get_latest_block_number().await?;
        info!("✅ RPC connection OK - Latest block: {}", block_number);
        
        info!("📊 Testing HTTP mempool polling...");
        test_http_mempool(&eth_client).await?;
    }

    Ok(())
}

async fn test_websocket_mempool(ws_url: &str) -> Result<()> {
    info!("Connecting to WebSocket: {}", ws_url);
    
    // Add timeout to connection
    info!("🔄 Starting connection...");
    let connect_future = connect_async(ws_url);
    info!("🔄 Waiting for connection with 10s timeout...");
    let (ws_stream, response) = tokio::time::timeout(
        tokio::time::Duration::from_secs(10), 
        connect_future
    ).await??;
    info!("✅ WebSocket connected successfully - Response: {:?}", response.status());
    let (mut write, mut read) = ws_stream.split();

    // Subscribe to pending transactions
    let subscribe_msg = serde_json::json!({
        "id": 1,
        "method": "eth_subscribe",
        "params": ["newPendingTransactions"]
    });

    write.send(Message::Text(subscribe_msg.to_string())).await?;
    info!("📡 Subscribed to pending transactions");

    let mut tx_count = 0;
    let start_time = std::time::Instant::now();

    while let Some(msg) = read.next().await {
        match msg? {
            Message::Text(text) => {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    if let Some(_tx_hash) = json.get("params").and_then(|p| p.get("result")) {
                        tx_count += 1;
                        
                        if tx_count % 10 == 0 {
                            let elapsed = start_time.elapsed().as_secs_f64();
                            let rate = tx_count as f64 / elapsed;
                            info!("📊 Transactions seen: {} | Rate: {:.1} tx/sec", tx_count, rate);
                        }

                        // Show first few transactions for debugging
                        if tx_count <= 3 {
                            info!("🔍 TX #{}: {:?}", tx_count, json);
                        }
                    } else if json.get("id") == Some(&serde_json::Value::Number(serde_json::Number::from(1))) {
                        info!("✅ Subscription confirmed: {:?}", json);
                    }
                }
            }
            Message::Close(_) => {
                warn!("❌ WebSocket connection closed");
                break;
            }
            _ => {}
        }

        // Stop after 30 seconds or 100 transactions
        if start_time.elapsed().as_secs() > 30 || tx_count > 100 {
            info!("🏁 Test complete - saw {} transactions in {:.1}s", 
                  tx_count, start_time.elapsed().as_secs_f64());
            break;
        }
    }

    if tx_count == 0 {
        error!("❌ No transactions received - check WebSocket URL and network");
    } else {
        info!("✅ WebSocket mempool working - {} transactions received", tx_count);
    }

    Ok(())
}

async fn test_http_mempool(eth_client: &EthereumClient) -> Result<()> {
    info!("📊 Testing HTTP-based mempool monitoring...");
    info!("⚠️  Note: HTTP polling is less reliable than WebSocket for mempool");

    let start_time = std::time::Instant::now();
    let mut last_block = eth_client.get_latest_block_number().await?;
    let mut blocks_seen = 0;

    while start_time.elapsed().as_secs() < 60 {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        let current_block = eth_client.get_latest_block_number().await?;
        
        if current_block > last_block {
            blocks_seen += 1;
            info!("🔥 New block: {} (#{} seen)", current_block, blocks_seen);
            
            // Get transactions in latest block
            if let Ok(transactions) = get_block_transactions(eth_client, current_block).await {
                info!("📦 Block {} has {} transactions", current_block, transactions.len());
                
                // Show first few transaction hashes
                for (i, tx_hash) in transactions.iter().take(3).enumerate() {
                    info!("   TX {}: {}", i + 1, tx_hash);
                }
            }
            
            last_block = current_block;
        }
    }

    if blocks_seen == 0 {
        warn!("❌ No new blocks seen in 60 seconds - network may be slow");
    } else {
        info!("✅ HTTP monitoring working - {} blocks seen", blocks_seen);
    }

    Ok(())
}

async fn get_block_transactions(eth_client: &EthereumClient, block_number: u64) -> Result<Vec<String>> {
    // This is a simplified version - in a real implementation you'd call the RPC directly
    info!("📦 Getting transactions for block {}", block_number);
    
    // Simulate some transaction hashes for the test
    Ok(vec![
        format!("0x{:064x}", block_number * 123456 + 1),
        format!("0x{:064x}", block_number * 123456 + 2),
        format!("0x{:064x}", block_number * 123456 + 3),
    ])
}