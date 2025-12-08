use ethereum_transaction_pool_monitor::transaction_decoder::TransactionDecoder;

fn main() {
    let decoder = TransactionDecoder::new();
    
    println!("🧪 Testing Token Transfer Decoding...\n");
    
    // Test ERC-20 transfer function with proper ABI data
    // transfer(address to, uint256 amount)
    // Function signature: 0xa9059cbb
    
    // Let me construct proper ABI data
    let mut transfer_data = String::new();
    transfer_data.push_str("0xa9059cbb"); // transfer function signature
    
    // Parameter 1: to address (32 bytes padded)
    transfer_data.push_str("000000000000000000000000abcdefabcdefabcdefabcdefabcdefabcdefabcd");
    
    // Parameter 2: amount = 2000000000 (2B USDT with 6 decimals) = 0x77359400
    transfer_data.push_str("0000000000000000000000000000000000000000000000000000000077359400");
    
    let usdt_contract = "0xdac17f958d2ee523a2206206994597c13d831ec7";
    
    println!("Testing USDT transfer with corrected data...");
    println!("Transfer data: {}", transfer_data);
    
    if let Some(transfer_info) = decoder.decode_token_transfer(usdt_contract, &transfer_data) {
        println!("✅ Token transfer decoded:");
        println!("  Function: {}", transfer_info.function_name);
        println!("  Token: {}", transfer_info.token_in);
        println!("  Recipient: {}", transfer_info.token_out);
        if let Some(amount) = transfer_info.amount_in {
            println!("  Amount: {}", amount);
        }
    } else {
        println!("❌ Failed to decode token transfer");
    }
    
    // Test with a larger WETH transfer: 1.5 WETH = 1500000000000000000 wei = 0x14d1120d7b160000
    let mut weth_transfer = String::new();
    weth_transfer.push_str("0xa9059cbb");
    weth_transfer.push_str("000000000000000000000000ddeeffddeffddeffddeffddeffddeffddeffdddd"); // Fixed: removed extra 'f'
    weth_transfer.push_str("00000000000000000000000000000000000000000000000014d1120d7b160000");
    
    let weth_contract = "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2";
    
    println!("\nTesting WETH transfer...");
    println!("WETH data length: {}", weth_transfer.len());
    println!("WETH data: {}", weth_transfer);
    if let Some(transfer_info) = decoder.decode_token_transfer(weth_contract, &weth_transfer) {
        println!("✅ WETH transfer decoded:");
        println!("  Function: {}", transfer_info.function_name);
        println!("  Token: {}", transfer_info.token_in);
        println!("  Recipient: {}", transfer_info.token_out);
        if let Some(amount) = transfer_info.amount_in {
            println!("  Amount: {}", amount);
        }
    } else {
        println!("❌ Failed to decode WETH transfer");
    }
    
    println!("\n🎯 Now you should see proper amounts!");
}