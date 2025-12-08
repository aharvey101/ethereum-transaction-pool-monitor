fn main() {
    // Let me create proper ABI encoded data for swapExactTokensForTokens
    // Function: swapExactTokensForTokens(uint256,uint256,address[],address,uint256)
    
    let mut data = String::new();
    data.push_str("0x38ed1739"); // Function signature
    
    // Parameter 1: amountIn = 200000000 (200 USDT with 6 decimals) = 0x0bebc200
    data.push_str("000000000000000000000000000000000000000000000000000000000bebc200");
    
    // Parameter 2: amountOutMin = 1234567890123 = 0x11f71fb0c7b
    data.push_str("000000000000000000000000000000000000000000000000000011f71fb0c7b");
    
    // Parameter 3: path offset = 0xa0 (160 bytes from start of parameters)
    data.push_str("00000000000000000000000000000000000000000000000000000000000000a0");
    
    // Parameter 4: to address
    data.push_str("000000000000000000000000aabbccddaabbccddaabbccddaabbccddaabbccdd");
    
    // Parameter 5: deadline
    data.push_str("00000000000000000000000000000000000000000000000000000000632ea4f8");
    
    // Now the array data at offset 0xa0:
    // Array length = 2
    data.push_str("0000000000000000000000000000000000000000000000000000000000000002");
    
    // Address 1: USDT = 0xdac17f958d2ee523a2206206994597c13d831ec7  
    data.push_str("000000000000000000000000dac17f958d2ee523a2206206994597c13d831ec7");
    
    // Address 2: WETH = 0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2
    data.push_str("000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
    
    println!("Properly formatted ABI data:");
    println!("{}", data);
    
    // Now test parsing this corrected data
    let array_start = 10 + 160 * 2; // 330
    
    println!("\nParsing corrected data:");
    println!("Array starts at: {}", array_start);
    
    let length_field = &data[array_start..array_start + 64];
    println!("Length field: {}", length_field);
    
    if let Ok(length) = u64::from_str_radix(&length_field[62..64], 16) {
        println!("Array length: {}", length);
        
        for i in 0..length {
            let addr_start = array_start + 64 + (i as usize * 64);
            let addr_end = addr_start + 64;
            if data.len() >= addr_end {
                let addr_field = &data[addr_start..addr_end];
                let addr = format!("0x{}", &addr_field[24..64]);
                println!("Address {}: {}", i, addr);
                
                match addr.as_str() {
                    "0xdac17f958d2ee523a2206206994597c13d831ec7" => println!("  -> USDT ✅"),
                    "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => println!("  -> WETH ✅"),
                    _ => println!("  -> Unknown"),
                }
            }
        }
    }
}