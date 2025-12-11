//! ABI Decoder Debug Tool
//!
//! This binary helps debug ABI decoding issues in router transaction parsing.
//! It analyzes transaction input data and shows detailed parsing steps.

use alloy_primitives::{hex::FromHex, Address, Bytes};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::collections::HashMap;

#[derive(Parser)]
#[command(name = "debug-abi-decoder")]
#[command(about = "Debug ABI decoding for router transactions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a raw transaction input hex string
    AnalyzeInput {
        /// Raw transaction input data (hex string with or without 0x prefix)
        #[arg(long)]
        input: String,
        /// Router address for context
        #[arg(long)]
        router: Option<String>,
    },
    /// Test with known Uniswap V2 swap examples
    TestKnownSwaps,
    /// Fetch and analyze a real transaction from RPC
    FetchTransaction {
        /// Transaction hash to fetch and analyze
        #[arg(long)]
        tx_hash: String,
        /// RPC URL to use
        #[arg(long, default_value = "http://192.168.0.14:8545")]
        rpc_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::AnalyzeInput { input, router } => {
            analyze_input_data(&input, router.as_deref()).await?;
        }
        Commands::TestKnownSwaps => {
            test_known_swaps().await?;
        }
        Commands::FetchTransaction { tx_hash, rpc_url } => {
            fetch_and_analyze_transaction(&tx_hash, &rpc_url).await?;
        }
    }

    Ok(())
}

/// Analyze raw input data and show detailed parsing steps
async fn analyze_input_data(input_hex: &str, router_address: Option<&str>) -> Result<()> {
    println!("🔍 ABI Decoder Analysis");
    println!("=======================");

    // Clean and parse hex input
    let clean_hex = input_hex.trim_start_matches("0x");
    let input_bytes = match hex::decode(clean_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            println!("❌ Failed to decode hex input: {}", e);
            return Ok(());
        }
    };

    println!("📊 Input Data Overview:");
    println!("   Length: {} bytes", input_bytes.len());
    if let Some(router) = router_address {
        println!("   Router: {}", router);
    }
    println!("   Raw Hex: 0x{}", hex::encode(&input_bytes));

    if input_bytes.len() < 4 {
        println!("❌ Input too short for function selector");
        return Ok(());
    }

    // Extract and identify function selector
    let function_selector = &input_bytes[0..4];
    let function_name = identify_function_selector(function_selector);
    
    println!("\n🎯 Function Analysis:");
    println!("   Selector: 0x{}", hex::encode(function_selector));
    println!("   Function: {}", function_name);

    // Show hex dump for easier analysis
    println!("\n📋 Hex Dump (first 200 bytes):");
    hex_dump(&input_bytes[..std::cmp::min(200, input_bytes.len())]);

    // Try to parse based on function selector
    let params_data = &input_bytes[4..];
    match function_selector {
        // swapExactTokensForTokens(uint256,uint256,address[],address,uint256)
        [0x38, 0xed, 0x17, 0x39] => {
            println!("\n🔧 Parsing swapExactTokensForTokens...");
            parse_swap_exact_tokens_for_tokens(params_data)?;
        }
        // swapExactETHForTokens(uint256,address[],address,uint256)
        [0x7f, 0xf3, 0x6a, 0xb5] => {
            println!("\n🔧 Parsing swapExactETHForTokens...");
            parse_swap_exact_eth_for_tokens(params_data)?;
        }
        // swapExactTokensForETH(uint256,uint256,address[],address,uint256)
        [0x18, 0xcb, 0xaf, 0xe5] => {
            println!("\n🔧 Parsing swapExactTokensForETH...");
            parse_swap_exact_tokens_for_eth(params_data)?;
        }
        _ => {
            println!("\n⚠️  Unknown function selector - attempting generic ABI parsing...");
            attempt_generic_abi_parsing(params_data)?;
        }
    }

    Ok(())
}

/// Parse swapExactTokensForTokens function parameters
fn parse_swap_exact_tokens_for_tokens(params_data: &[u8]) -> Result<()> {
    println!("   Function: swapExactTokensForTokens(uint256,uint256,address[],address,uint256)");
    
    if params_data.len() < 160 {
        println!("   ❌ Insufficient data: {} bytes (need at least 160)", params_data.len());
        return Ok(());
    }

    // Parameter 0: amountIn (bytes 0-31)
    let amount_in = parse_uint256_hex(&params_data[0..32]);
    println!("   📊 AmountIn: {} wei", amount_in);

    // Parameter 1: amountOutMin (bytes 32-63)
    let amount_out_min = parse_uint256_hex(&params_data[32..64]);
    println!("   📊 AmountOutMin: {} wei", amount_out_min);

    // Parameter 2: path (dynamic array) - offset at bytes 64-95
    let path_offset_bytes = &params_data[64..96];
    println!("   📊 Path offset full word: 0x{}", hex::encode(path_offset_bytes));
    let path_offset = u32::from_be_bytes([
        path_offset_bytes[28], path_offset_bytes[29], path_offset_bytes[30], path_offset_bytes[31]
    ]) as usize;
    println!("   📊 Path offset: {} bytes", path_offset);
    println!("   📊 Path offset last 4 bytes: 0x{}", hex::encode(&path_offset_bytes[28..32]));

    // Parameter 3: to address (bytes 96-127)
    let to_address = parse_address(&params_data[96..128]);
    println!("   📊 To address: {}", to_address);

    // Parameter 4: deadline (bytes 128-159)
    let deadline = parse_uint256_hex(&params_data[128..160]);
    println!("   📊 Deadline: {}", deadline);

    // Parse the path array
    if path_offset < params_data.len() {
        println!("\n   🛣️  Parsing token path...");
        parse_address_array(&params_data[path_offset..])?;
    } else {
        println!("   ❌ Path offset {} is beyond data length {}", path_offset, params_data.len());
    }

    Ok(())
}

/// Parse swapExactETHForTokens function parameters
fn parse_swap_exact_eth_for_tokens(params_data: &[u8]) -> Result<()> {
    println!("   Function: swapExactETHForTokens(uint256,address[],address,uint256)");
    
    if params_data.len() < 128 {
        println!("   ❌ Insufficient data: {} bytes (need at least 128)", params_data.len());
        return Ok(());
    }

    // Parameter 0: amountOutMin (bytes 0-31)
    let amount_out_min = parse_uint256_hex(&params_data[0..32]);
    println!("   📊 AmountOutMin: {} wei", amount_out_min);

    // Parameter 1: path (dynamic array) - offset at bytes 32-63
    let path_offset_bytes = &params_data[32..64];
    let path_offset = u32::from_be_bytes([
        path_offset_bytes[28], path_offset_bytes[29], path_offset_bytes[30], path_offset_bytes[31]
    ]) as usize;
    println!("   📊 Path offset: {} bytes", path_offset);
    println!("   📊 Path offset hex: 0x{}", hex::encode(&path_offset_bytes[28..32]));

    // Parameter 2: to address (bytes 64-95)
    let to_address = parse_address(&params_data[64..96]);
    println!("   📊 To address: {}", to_address);

    // Parameter 3: deadline (bytes 96-127)
    let deadline = parse_uint256_hex(&params_data[96..128]);
    println!("   📊 Deadline: {}", deadline);

    // Parse the path array
    if path_offset < params_data.len() {
        println!("\n   🛣️  Parsing token path...");
        parse_address_array(&params_data[path_offset..])?;
    } else {
        println!("   ❌ Path offset {} is beyond data length {}", path_offset, params_data.len());
    }

    Ok(())
}

/// Parse swapExactTokensForETH function parameters
fn parse_swap_exact_tokens_for_eth(params_data: &[u8]) -> Result<()> {
    println!("   Function: swapExactTokensForETH(uint256,uint256,address[],address,uint256)");
    
    if params_data.len() < 160 {
        println!("   ❌ Insufficient data: {} bytes (need at least 160)", params_data.len());
        return Ok(());
    }

    // Same structure as swapExactTokensForTokens
    parse_swap_exact_tokens_for_tokens(params_data)
}

/// Parse an address array from ABI encoded data
fn parse_address_array(data: &[u8]) -> Result<()> {
    if data.len() < 32 {
        println!("   ❌ Not enough data for array length");
        return Ok(());
    }

    // First 32 bytes contain array length - take last 4 bytes
    let array_length = u32::from_be_bytes([
        data[28], data[29], data[30], data[31]
    ]) as usize;
    println!("   📊 Array length: {}", array_length);
    println!("   📊 Array length hex: 0x{}", hex::encode(&data[28..32]));

    if array_length == 0 {
        println!("   ⚠️  Empty array");
        return Ok(());
    }

    // Each address takes 32 bytes (20 bytes address + 12 bytes padding)
    if array_length > 100 {
        println!("   ❌ Array length {} seems too large - likely parsing error", array_length);
        return Ok(());
    }
    
    let required_length = 32 + (array_length * 32);
    if data.len() < required_length {
        println!("   ❌ Not enough data for {} addresses: need {}, have {}", 
                 array_length, required_length, data.len());
        return Ok(());
    }

    println!("   🎯 Token addresses:");
    for i in 0..array_length {
        let offset = 32 + (i * 32);
        let address = parse_address(&data[offset..offset + 32]);
        println!("      [{}] {}", i, address);
    }

    Ok(())
}

/// Attempt generic ABI parsing for unknown functions
fn attempt_generic_abi_parsing(params_data: &[u8]) -> Result<()> {
    println!("   📊 Data length: {} bytes", params_data.len());
    
    if params_data.len() % 32 != 0 {
        println!("   ⚠️  Data length not multiple of 32 bytes - may be packed encoding");
    }

    // Try to identify potential parameters
    let num_words = params_data.len() / 32;
    println!("   📊 Number of 32-byte words: {}", num_words);

    for i in 0..std::cmp::min(num_words, 10) { // Show first 10 parameters
        let offset = i * 32;
        let word = &params_data[offset..offset + 32];
        
        let as_uint = parse_uint256_hex(word);
        let as_address = parse_address(word);
        
        println!("   Word {}: 0x{} (uint: {}, addr: {})", 
                 i, hex::encode(word), as_uint, as_address);
    }

    Ok(())
}

/// Parse 32-byte word as uint256 and return as u64 for practical use
fn parse_uint256(data: &[u8]) -> u64 {
    if data.len() < 32 {
        return 0;
    }
    
    // Convert big-endian bytes to u64 (taking last 8 bytes for practical values)
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[24..32]); // Last 8 bytes of the 32-byte word
    u64::from_be_bytes(bytes)
}

/// Parse 32-byte word as uint256 and return as hex string for display
fn parse_uint256_hex(data: &[u8]) -> String {
    if data.len() < 32 {
        return "Invalid".to_string();
    }
    
    let mut result = String::new();
    let mut started = false;
    
    for byte in data {
        if *byte != 0 || started {
            if !started {
                started = true;
            }
            result.push_str(&format!("{:02x}", byte));
        }
    }
    
    if result.is_empty() {
        "0".to_string()
    } else {
        format!("0x{}", result)
    }
}

/// Parse 32-byte word as address (last 20 bytes)
fn parse_address(data: &[u8]) -> String {
    if data.len() < 32 {
        return "Invalid".to_string();
    }
    
    let address_bytes = &data[12..32]; // Last 20 bytes
    format!("0x{}", hex::encode(address_bytes))
}

/// Print hex dump of data
fn hex_dump(data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        print!("   {:04x}: ", i * 16);
        
        // Hex bytes
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        
        // Padding for incomplete lines
        for _ in chunk.len()..16 {
            print!("   ");
        }
        
        print!("  ");
        
        // ASCII representation
        for byte in chunk {
            let c = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            print!("{}", c);
        }
        
        println!();
    }
}

/// Identify function selector and return function name
fn identify_function_selector(selector: &[u8]) -> &'static str {
    let mut known_functions = HashMap::new();
    
    // Uniswap V2 Router functions
    known_functions.insert([0x38, 0xed, 0x17, 0x39], "swapExactTokensForTokens(uint256,uint256,address[],address,uint256)");
    known_functions.insert([0x7f, 0xf3, 0x6a, 0xb5], "swapExactETHForTokens(uint256,address[],address,uint256)");
    known_functions.insert([0x18, 0xcb, 0xaf, 0xe5], "swapExactTokensForETH(uint256,uint256,address[],address,uint256)");
    known_functions.insert([0x8f, 0x0e, 0x15, 0xa4], "swapTokensForExactTokens(uint256,uint256,address[],address,uint256)");
    known_functions.insert([0x4a, 0x25, 0xa9, 0x4a], "swapTokensForExactETH(uint256,uint256,address[],address,uint256)");
    known_functions.insert([0xfb, 0x3b, 0xdb, 0x41], "swapETHForExactTokens(uint256,address[],address,uint256)");
    
    // Uniswap V3 Router functions
    known_functions.insert([0x41, 0x4b, 0xf3, 0x89], "exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))");
    known_functions.insert([0xb8, 0x58, 0x18, 0x3f], "exactInput((bytes,address,uint256,uint256,uint256))");
    known_functions.insert([0xdb, 0x3e, 0x21, 0x98], "exactOutputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))");
    known_functions.insert([0xf2, 0x8c, 0x04, 0x98], "exactOutput((bytes,address,uint256,uint256,uint256))");
    
    if selector.len() >= 4 {
        let selector_array = [selector[0], selector[1], selector[2], selector[3]];
        known_functions.get(&selector_array).unwrap_or(&"Unknown function")
    } else {
        "Invalid selector"
    }
}

/// Test with known swap examples
async fn test_known_swaps() -> Result<()> {
    println!("🧪 Testing Known Swap Examples");
    println!("==============================");

    // Example 1: swapExactETHForTokens
    let example1 = "7ff36ab5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000001234567890123456789012345678901234567890000000000000000000000000000000000000000000000000000000065a4c800000000000000000000000000000000000000000000000000000000000000000200000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2000000000000000000000000a0b86a33e6e6cd12bbea3c80e5b4e4e2b6c5e987";
    
    println!("\n🔍 Example 1: swapExactETHForTokens");
    analyze_input_data(example1, Some("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")).await?;

    // Example 2: swapExactTokensForTokens  
    let example2 = "38ed1739000000000000000000000000000000000000000000000000de0b6b3a7640000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a000000000000000000000000012345678901234567890123456789012345678900000000000000000000000000000000000000000000000000000000065a4c8000000000000000000000000000000000000000000000000000000000000000002000000000000000000000000a0b86a33e6e6cd12bbea3c80e5b4e4e2b6c5e987000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc2";

    println!("\n🔍 Example 2: swapExactTokensForTokens");
    analyze_input_data(example2, Some("0x7a250d5630b4cf539739df2c5dacb4c659f2488d")).await?;

    Ok(())
}

/// Fetch and analyze a real transaction
async fn fetch_and_analyze_transaction(tx_hash: &str, rpc_url: &str) -> Result<()> {
    println!("🌐 Fetching Transaction: {}", tx_hash);
    println!("======================================");

    // TODO: Implement RPC call to fetch transaction
    // For now, just show the structure
    println!("📝 Note: RPC fetching not yet implemented");
    println!("   To analyze a real transaction:");
    println!("   1. Fetch transaction data using: curl -X POST {} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionByHash\",\"params\":[\"{}\"],\"id\":1}}'", rpc_url, tx_hash);
    println!("   2. Extract the 'input' field from the response");
    println!("   3. Use: {} analyze-input --input <INPUT_DATA>", env!("CARGO_PKG_NAME"));

    Ok(())
}