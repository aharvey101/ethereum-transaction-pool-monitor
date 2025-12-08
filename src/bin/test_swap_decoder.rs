use ethereum_transaction_pool_monitor::transaction_decoder::TransactionDecoder;

fn main() {
    let decoder = TransactionDecoder::new();
    
    // Test swapExactTokensForTokens with properly formatted ABI data
    // This represents: USDT (200 tokens) -> WETH (minimum expected output)
    let mut swap_data = String::new();
    swap_data.push_str("0x38ed1739"); // swapExactTokensForTokens signature
    
    // amountIn: 200000000 (200 USDT with 6 decimals)
    swap_data.push_str("000000000000000000000000000000000000000000000000000000000bebc200");
    
    // amountOutMin: some minimum WETH amount
    swap_data.push_str("000000000000000000000000000000000000000000000000000de0b6b3a7640000");
    
    // path offset: 0xa0 (160 bytes)  
    swap_data.push_str("00000000000000000000000000000000000000000000000000000000000000a0");
    
    // to address
    swap_data.push_str("000000000000000000000000aabbccddaabbccddaabbccddaabbccddaabbccdd");
    
    // deadline
    swap_data.push_str("00000000000000000000000000000000000000000000000000000000632ea4f8");
    
    // Array length: 2
    swap_data.push_str("0000000000000000000000000000000000000000000000000000000000000002");
    
    // USDT address
    swap_data.push_str("000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7");
    
    // WETH address  
    swap_data.push_str("000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    
    println!("Testing swap decoder with proper ABI data...");
    
    // Test with known router address (Uniswap V2)
    let router_addr = "0x7a250d5630b4cf539739df2c5dacb4c659f2488d";
    let value_hex = "0x0"; // No ETH value for token-to-token swap
    
    if let Some(swap_info) = decoder.decode_swap(router_addr, &swap_data, value_hex) {
        println!("✅ Swap decoded successfully:");
        println!("  Function: {}", swap_info.function_name);
        println!("  Token In: {}", swap_info.token_in);
        println!("  Token Out: {}", swap_info.token_out);
        if let Some(amount_in) = swap_info.amount_in {
            println!("  Amount In: {}", amount_in);
        }
        if let Some(amount_out_min) = swap_info.amount_out_min {
            println!("  Min Amount Out: {}", amount_out_min);
        }
    } else {
        println!("❌ Failed to decode swap");
    }
    
    println!("\n🧪 Testing individual token resolution:");
    println!("  USDT: {}", decoder.get_token_symbol("0xdac17f958d2ee523a2206206994597c13d831ec7"));
    println!("  WETH: {}", decoder.get_token_symbol("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));
    println!("  USDC: {}", decoder.get_token_symbol("0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d"));
}