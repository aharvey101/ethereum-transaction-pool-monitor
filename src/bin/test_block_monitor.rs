use anyhow::Result;
use ethereum_transaction_pool_monitor::eth_client::EthereumClient;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("⏰ Long-term Block Monitor (will wait for next block)...\n");
    
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_else(|_| "http://192.168.0.14:8545".to_string());
    let client = EthereumClient::new(&rpc_url).await?;
    
    // Get initial block
    let initial_block = client.get_latest_block_number().await?;
    println!("🔵 Starting block: #{}", initial_block);
    println!("⏳ Waiting for block #{} to be mined...", initial_block + 1);
    println!("💡 This may take 10-30 seconds depending on network timing");
    println!("📊 Checking every 2 seconds...\n");
    
    let start_time = std::time::Instant::now();
    let mut check_count = 0;
    let target_block = initial_block + 1;
    
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        check_count += 1;
        
        match client.get_latest_block_number().await {
            Ok(current_block) => {
                let elapsed = start_time.elapsed();
                
                if current_block >= target_block {
                    println!("🎉 NEW BLOCK DETECTED!");
                    println!("   📈 Block #{} → #{}", initial_block, current_block);
                    println!("   ⏰ Time elapsed: {:?}", elapsed);
                    println!("   📊 Checks performed: {}", check_count);
                    
                    if current_block > target_block {
                        println!("   ⚠️  Missed {} blocks during polling", current_block - target_block);
                    }
                    
                    println!("\n✅ Block detection working correctly!");
                    break;
                } else {
                    if check_count % 10 == 0 {
                        println!("📊 Check #{}: Still block #{} (elapsed: {:?})", 
                            check_count, current_block, elapsed);
                    } else {
                        print!(".");
                        std::io::Write::flush(&mut std::io::stdout()).unwrap();
                    }
                }
                
                // Safety timeout after 5 minutes
                if elapsed.as_secs() > 300 {
                    println!("\n⏰ Timeout after 5 minutes - no new blocks detected");
                    println!("💡 This could indicate:");
                    println!("   • Network is experiencing longer block times");
                    println!("   • RPC node might not be fully synced");
                    println!("   • Chain might be having issues");
                    break;
                }
            }
            Err(e) => {
                println!("\n❌ Error fetching block: {}", e);
                break;
            }
        }
    }
    
    Ok(())
}