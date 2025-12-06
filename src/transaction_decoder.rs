use anyhow::Result;
use std::collections::HashMap;

/// Decoded swap information
#[derive(Clone, Debug)]
pub struct SwapInfo {
    pub function_name: String,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: Option<String>,
    pub amount_out_min: Option<String>,
    pub raw_data: String,
}

/// Transaction decoder for Uniswap and other DEX protocols
pub struct TransactionDecoder {
    /// Known token symbols by address
    token_symbols: HashMap<String, String>,
}

impl TransactionDecoder {
    pub fn new() -> Self {
        let mut token_symbols = HashMap::new();
        
        // Popular tokens with their symbols
        let tokens = [
            // Stablecoins
            ("0xdac17f958d2ee523a2206206994597c13d831ec7", "USDT"),
            ("0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", "USDC"),
            ("0x6b175474e89094c44da98b954eedeac495271d0f", "DAI"),
            ("0x4fabb145d64652a948d72533023f6e7a623c7c53", "BUSD"),
            ("0x853d955acef822db058eb8505911ed77f175b99e", "FRAX"),
            ("0x5f98805a4e8be255a32880fdec7f6728c6568ba0", "LUSD"),
            
            // Major tokens
            ("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "WETH"),
            ("0x1f9840a85d5af5bf1d1762f925bdaddc4201f984", "UNI"),
            ("0x7d1afa7b718fb893db30a3abc0cfc608aacfebb0", "MATIC"),
            ("0x6b3595068778dd592e39a122f4f5a5cf09c90fe2", "SUSHI"),
            ("0xc00e94cb662c3520282e6f5717214004a7f26888", "COMP"),
            ("0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2", "MKR"),
            ("0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9", "AAVE"),
            ("0xc011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f", "SNX"),
            ("0x0bc529c00c6401aef6d220be8c6ea1667f6ad93e", "YFI"),
            ("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", "WBTC"),
            ("0x514910771af9ca656af840dff83e8264ecf986ca", "LINK"),
            ("0xae7ab96520de3a18e5e111b5eaab095312d7fe84", "stETH"),
            ("0x95ad61b0a150d79219dcf64e1e6cc01f0b64c4ce", "SHIB"),
            
            // Special addresses
            ("0x0000000000000000000000000000000000000000", "ETH"),
            ("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "ETH"), // 1inch uses this for ETH
        ];
        
        for (address, symbol) in tokens {
            token_symbols.insert(address.to_lowercase(), symbol.to_string());
        }
        
        TransactionDecoder { token_symbols }
    }
    
    /// Get token symbol by address, fallback to shortened address if unknown
    pub fn get_token_symbol(&self, address: &str) -> String {
        let normalized = address.to_lowercase();
        if let Some(symbol) = self.token_symbols.get(&normalized) {
            symbol.clone()
        } else if normalized == "0x0000000000000000000000000000000000000000" || normalized.is_empty() {
            "ETH".to_string()
        } else {
            // Return shortened address for unknown tokens
            format!("{}...{}", &address[0..6], &address[address.len()-4..])
        }
    }
    
    /// Decode transaction data to extract swap information
    pub fn decode_swap(&self, to_address: &str, data: &str) -> Option<SwapInfo> {
        if data.len() < 10 {
            return None; // Not enough data for function signature
        }
        
        let function_sig = &data[0..10]; // First 4 bytes (8 hex chars + 0x)
        
        match function_sig {
            // Uniswap V2 Router functions
            "0x38ed1739" => self.decode_swap_exact_tokens_for_tokens(data),           // swapExactTokensForTokens
            "0x8803dbee" => self.decode_swap_tokens_for_exact_tokens(data),           // swapTokensForExactTokens
            "0x7ff36ab5" => self.decode_swap_exact_eth_for_tokens(data),              // swapExactETHForTokens
            "0x18cbafe5" => self.decode_swap_exact_tokens_for_eth(data),              // swapExactTokensForETH
            "0x4a25d94a" => self.decode_swap_exact_tokens_for_eth_supporting_fee(data), // swapExactTokensForETHSupportingFeeOnTransferTokens
            "0xb6f9de95" => self.decode_swap_exact_eth_for_tokens_supporting_fee(data), // swapExactETHForTokensSupportingFeeOnTransferTokens
            
            // Uniswap V3 Router functions  
            "0x414bf389" => self.decode_exact_input_single(data),                     // exactInputSingle
            "0xc04b8d59" => self.decode_exact_input(data),                            // exactInput
            "0xdb3e2198" => self.decode_exact_output_single(data),                   // exactOutputSingle
            "0x09b81346" => self.decode_exact_output(data),                          // exactOutput
            
            // 1inch Router functions
            "0x7c025200" => self.decode_1inch_swap(data),                            // swap
            "0xe449022e" => self.decode_1inch_unoswap(data),                         // unoswap
            
            _ => None,
        }
    }
    
    /// Decode swapExactTokensForTokens function call
    fn decode_swap_exact_tokens_for_tokens(&self, data: &str) -> Option<SwapInfo> {
        if data.len() < 202 { return None; } // Need at least amountIn + amountOutMin + path start + deadline
        
        // Parse parameters (each is 32 bytes = 64 hex chars)
        let amount_in = &data[10..74];   // Skip function sig (10 chars), take next 64
        let amount_out_min = &data[74..138];
        
        // Path is more complex to decode properly, but we can try to get first and last tokens
        // For now, let's just show the function name
        Some(SwapInfo {
            function_name: "Swap Tokens".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: Some(hex_to_readable_amount(amount_in)),
            amount_out_min: Some(hex_to_readable_amount(amount_out_min)),
            raw_data: data.to_string(),
        })
    }
    
    // Placeholder implementations for other functions
    fn decode_swap_tokens_for_exact_tokens(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "Swap Tokens (Exact Out)".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_swap_exact_eth_for_tokens(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "Buy with ETH".to_string(),
            token_in: "ETH".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_swap_exact_tokens_for_eth(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "Sell for ETH".to_string(),
            token_in: "Token".to_string(),
            token_out: "ETH".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_swap_exact_tokens_for_eth_supporting_fee(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "Sell for ETH (Fee)".to_string(),
            token_in: "Token".to_string(),
            token_out: "ETH".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_swap_exact_eth_for_tokens_supporting_fee(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "Buy with ETH (Fee)".to_string(),
            token_in: "ETH".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_exact_input_single(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "V3 Swap Single".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_exact_input(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "V3 Swap Multi".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_exact_output_single(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "V3 Swap Single (Exact Out)".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_exact_output(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "V3 Swap Multi (Exact Out)".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_1inch_swap(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "1inch Swap".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    fn decode_1inch_unoswap(&self, data: &str) -> Option<SwapInfo> {
        Some(SwapInfo {
            function_name: "1inch UnoSwap".to_string(),
            token_in: "Token".to_string(),
            token_out: "Token".to_string(),
            amount_in: None,
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
}

/// Convert hex amount to a more readable format
fn hex_to_readable_amount(hex_str: &str) -> String {
    if let Ok(amount) = u128::from_str_radix(hex_str, 16) {
        if amount == 0 {
            "0".to_string()
        } else if amount > 1_000_000_000_000_000_000 { // > 1 ETH worth in wei
            format!("{:.2}E", amount as f64 / 1e18)
        } else if amount > 1_000_000 {
            format!("{:.0}K", amount as f64 / 1e3)
        } else {
            amount.to_string()
        }
    } else {
        "?".to_string()
    }
}