use std::collections::HashMap;

/// Token metadata for proper decimal formatting
#[derive(Clone, Debug)]
struct TokenInfo {
    symbol: String,
    decimals: u8,
}

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
    /// Known token information by address
    token_info: HashMap<String, TokenInfo>,
}

impl TransactionDecoder {
    pub fn new() -> Self {
        let mut token_info = HashMap::new();
        
        // Popular tokens with their symbols and decimals
        let tokens = [
            // Stablecoins (mostly 6 decimals except DAI)
            ("0xdac17f958d2ee523a2206206994597c13d831ec7", TokenInfo { symbol: "USDT".to_string(), decimals: 6 }),
            ("0xa0b86991c431c8ba3b80e36c4b5f6b4b3c4f6e5d", TokenInfo { symbol: "USDC".to_string(), decimals: 6 }),
            ("0x6b175474e89094c44da98b954eedeac495271d0f", TokenInfo { symbol: "DAI".to_string(), decimals: 18 }),
            ("0x4fabb145d64652a948d72533023f6e7a623c7c53", TokenInfo { symbol: "BUSD".to_string(), decimals: 18 }),
            ("0x853d955acef822db058eb8505911ed77f175b99e", TokenInfo { symbol: "FRAX".to_string(), decimals: 18 }),
            ("0x5f98805a4e8be255a32880fdec7f6728c6568ba0", TokenInfo { symbol: "LUSD".to_string(), decimals: 18 }),
            
            // Major tokens (18 decimals)
            ("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", TokenInfo { symbol: "WETH".to_string(), decimals: 18 }),
            ("0x1f9840a85d5af5bf1d1762f925bdaddc4201f984", TokenInfo { symbol: "UNI".to_string(), decimals: 18 }),
            ("0x7d1afa7b718fb893db30a3abc0cfc608aacfebb0", TokenInfo { symbol: "MATIC".to_string(), decimals: 18 }),
            ("0x6b3595068778dd592e39a122f4f5a5cf09c90fe2", TokenInfo { symbol: "SUSHI".to_string(), decimals: 18 }),
            ("0xc00e94cb662c3520282e6f5717214004a7f26888", TokenInfo { symbol: "COMP".to_string(), decimals: 18 }),
            ("0x9f8f72aa9304c8b593d555f12ef6589cc3a579a2", TokenInfo { symbol: "MKR".to_string(), decimals: 18 }),
            ("0x7fc66500c84a76ad7e9c93437bfc5ac33e2ddae9", TokenInfo { symbol: "AAVE".to_string(), decimals: 18 }),
            ("0xc011a73ee8576fb46f5e1c5751ca3b9fe0af2a6f", TokenInfo { symbol: "SNX".to_string(), decimals: 18 }),
            ("0x0bc529c00c6401aef6d220be8c6ea1667f6ad93e", TokenInfo { symbol: "YFI".to_string(), decimals: 18 }),
            ("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599", TokenInfo { symbol: "WBTC".to_string(), decimals: 8 }),
            ("0x514910771af9ca656af840dff83e8264ecf986ca", TokenInfo { symbol: "LINK".to_string(), decimals: 18 }),
            ("0xae7ab96520de3a18e5e111b5eaab095312d7fe84", TokenInfo { symbol: "stETH".to_string(), decimals: 18 }),
            ("0x95ad61b0a150d79219dcf64e1e6cc01f0b64c4ce", TokenInfo { symbol: "SHIB".to_string(), decimals: 18 }),
            
            // Special addresses
            ("0x0000000000000000000000000000000000000000", TokenInfo { symbol: "ETH".to_string(), decimals: 18 }),
            ("0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", TokenInfo { symbol: "ETH".to_string(), decimals: 18 }), // 1inch uses this for ETH
        ];
        
        for (address, info) in tokens {
            token_info.insert(address.to_lowercase(), info);
        }
        
        TransactionDecoder { token_info }
    }
    
    /// Get token symbol by address, fallback to shortened address if unknown
    pub fn get_token_symbol(&self, address: &str) -> String {
        let normalized = address.to_lowercase();
        if let Some(token) = self.token_info.get(&normalized) {
            token.symbol.clone()
        } else if normalized == "0x0000000000000000000000000000000000000000" || normalized.is_empty() {
            "ETH".to_string()
        } else {
            // Return shortened address for unknown tokens
            format!("{}...{}", &address[0..6], &address[address.len()-4..])
        }
    }
    
    /// Extract a 32-byte parameter from ABI encoded data at a given offset
    fn extract_param(&self, data: &str, offset: usize) -> Option<String> {
        let start = 10 + offset * 64; // 10 for function sig + offset * 64 hex chars
        let end = start + 64;
        if data.len() >= end {
            Some(data[start..end].to_string())
        } else {
            None
        }
    }
    
    /// Extract address from 32-byte ABI parameter (addresses are padded to 32 bytes)
    fn extract_address(&self, param: &str) -> String {
        if param.len() >= 40 {
            format!("0x{}", &param[param.len()-40..])
        } else {
            param.to_string()
        }
    }
    
    /// Parse dynamic array offset and extract token addresses from path
    fn extract_token_path(&self, data: &str, path_offset_param: &str) -> (String, String) {
        // Parse the offset to find where the path array starts
        if let Ok(offset) = u64::from_str_radix(path_offset_param, 16) {
            let array_start = 10 + (offset as usize) * 2; // offset is in bytes, convert to hex position
            
            if data.len() > array_start + 64 {
                // First 32 bytes at offset is array length
                let length_hex = &data[array_start..array_start + 64];
                // Parse the full 64-char hex string as a number (it represents a 256-bit integer)
                if let Ok(length) = u64::from_str_radix(length_hex, 16) {
                    if length >= 2 && length <= 100 { // Reasonable bounds for path array
                        // In ABI encoding, each address is padded to 32 bytes (64 hex chars)
                        let first_token_start = array_start + 64; // Skip length (32 bytes = 64 hex)
                        let first_token_end = first_token_start + 64; // 32 bytes = 64 hex
                        
                        let last_token_start = array_start + 64 + ((length as usize - 1) * 64);
                        let last_token_end = last_token_start + 64;
                        
                        if data.len() >= last_token_end {
                            // Extract address from the last 40 hex chars (20 bytes) of the 64-char field
                            let first_addr_hex = &data[first_token_start..first_token_end];
                            let last_addr_hex = &data[last_token_start..last_token_end];
                            
                            let first_addr = format!("0x{}", &first_addr_hex[24..]);
                            let last_addr = format!("0x{}", &last_addr_hex[24..]);
                            
                            return (
                                self.get_token_symbol(&first_addr),
                                self.get_token_symbol(&last_addr)
                            );
                        }
                    }
                }
            }
        }
        
        ("Token".to_string(), "Token".to_string())
    }
    
    /// Format amount with proper decimals and human-readable units
    fn format_amount(&self, hex_amount: &str, token_symbol: &str, default_decimals: u8) -> String {
        if let Ok(amount) = u128::from_str_radix(hex_amount, 16) {
            if amount == 0 {
                return "0".to_string();
            }
            
            // Get token decimals, fallback to default
            let decimals = if let Some(token) = self.token_info.values().find(|t| t.symbol == token_symbol) {
                token.decimals
            } else {
                default_decimals
            };
            
            let divisor = 10_u128.pow(decimals as u32);
            let formatted_amount = amount as f64 / divisor as f64;
            
            // Format with appropriate units
            if formatted_amount >= 1_000_000.0 {
                format!("{:.2}M {}", formatted_amount / 1_000_000.0, token_symbol)
            } else if formatted_amount >= 1_000.0 {
                format!("{:.2}K {}", formatted_amount / 1_000.0, token_symbol)
            } else if formatted_amount >= 1.0 {
                format!("{:.4} {}", formatted_amount, token_symbol)
            } else if formatted_amount >= 0.01 {
                format!("{:.6} {}", formatted_amount, token_symbol)
            } else if formatted_amount > 0.0 {
                format!("{:.8} {}", formatted_amount, token_symbol)
            } else {
                format!("0 {}", token_symbol)
            }
        } else {
            format!("? {}", token_symbol)
        }
    }
    
    /// Decode transaction data to extract swap information
    pub fn decode_swap(&self, _to_address: &str, data: &str, value_hex: &str) -> Option<SwapInfo> {
        if data.len() < 10 {
            return None; // Not enough data for function signature
        }
        
        let function_sig = &data[0..10]; // First 4 bytes (8 hex chars + 0x)
        
        // Handle ETH-related swaps differently since they need the value
        match function_sig {
            // Uniswap V2 Router functions that use ETH
            "0x7ff36ab5" => self.decode_swap_exact_eth_for_tokens(data, value_hex),   // swapExactETHForTokens
            "0xb6f9de95" => self.decode_swap_exact_eth_for_tokens_supporting_fee(data, value_hex), // swapExactETHForTokensSupportingFeeOnTransferTokens
            
            // All other functions don't need the value
            _ => match function_sig {
                // Uniswap V2 Router functions
                "0x38ed1739" => self.decode_swap_exact_tokens_for_tokens(data),           // swapExactTokensForTokens
                "0x8803dbee" => self.decode_swap_tokens_for_exact_tokens(data),           // swapTokensForExactTokens
                "0x18cbafe5" => self.decode_swap_exact_tokens_for_eth(data),              // swapExactTokensForETH
                "0x4a25d94a" => self.decode_swap_exact_tokens_for_eth_supporting_fee(data), // swapExactTokensForETHSupportingFeeOnTransferTokens
                
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
    }
    
    /// Decode ERC-20 token transfer to show transfer amounts
    pub fn decode_token_transfer(&self, to_address: &str, data: &str) -> Option<SwapInfo> {
        if data.len() < 10 {
            return None;
        }
        
        let function_sig = &data[0..10];
        
        match function_sig {
            "0xa9059cbb" => self.decode_erc20_transfer(to_address, data),        // transfer(address,uint256)
            "0x23b872dd" => self.decode_erc20_transfer_from(to_address, data),   // transferFrom(address,address,uint256)
            _ => None,
        }
    }
    
    /// Decode ERC-20 transfer function
    fn decode_erc20_transfer(&self, token_address: &str, data: &str) -> Option<SwapInfo> {
        // transfer(address to, uint256 amount)
        // Parameters: to (0), amount (1)
        
        let to_param = self.extract_param(data, 0)?;
        let amount_param = self.extract_param(data, 1)?;
        
        let to_address = self.extract_address(&to_param);
        let token_symbol = self.get_token_symbol(token_address);
        
        Some(SwapInfo {
            function_name: format!("Transfer {}", token_symbol),
            token_in: token_symbol.clone(),
            token_out: format!("to {}", self.format_address(&to_address)),
            amount_in: Some(self.format_amount(&amount_param, &token_symbol, 18)),
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    /// Decode ERC-20 transferFrom function
    fn decode_erc20_transfer_from(&self, token_address: &str, data: &str) -> Option<SwapInfo> {
        // transferFrom(address from, address to, uint256 amount)
        // Parameters: from (0), to (1), amount (2)
        
        let from_param = self.extract_param(data, 0)?;
        let to_param = self.extract_param(data, 1)?;
        let amount_param = self.extract_param(data, 2)?;
        
        let from_address = self.extract_address(&from_param);
        let to_address = self.extract_address(&to_param);
        let token_symbol = self.get_token_symbol(token_address);
        
        Some(SwapInfo {
            function_name: format!("Transfer {} from {}", token_symbol, self.format_address(&from_address)),
            token_in: token_symbol.clone(),
            token_out: format!("to {}", self.format_address(&to_address)),
            amount_in: Some(self.format_amount(&amount_param, &token_symbol, 18)),
            amount_out_min: None,
            raw_data: data.to_string(),
        })
    }
    
    /// Format address to be more readable
    fn format_address(&self, address: &str) -> String {
        if address.len() >= 10 {
            format!("{}...{}", &address[0..6], &address[address.len()-4..])
        } else {
            address.to_string()
        }
    }
    
    /// Decode swapExactTokensForTokens function call
    fn decode_swap_exact_tokens_for_tokens(&self, data: &str) -> Option<SwapInfo> {
        // swapExactTokensForTokens(uint256 amountIn, uint256 amountOutMin, address[] path, address to, uint256 deadline)
        // Parameters: amountIn (0), amountOutMin (1), path offset (2), to (3), deadline (4)
        
        let amount_in = self.extract_param(data, 0)?;
        let amount_out_min = self.extract_param(data, 1)?;
        let path_offset = self.extract_param(data, 2)?;
        
        let (token_in, token_out) = self.extract_token_path(data, &path_offset);
        
        Some(SwapInfo {
            function_name: format!("Swap: {} → {}", token_in, token_out),
            token_in: token_in.clone(),
            token_out: token_out.clone(),
            amount_in: Some(self.format_amount(&amount_in, &token_in, 18)),
            amount_out_min: Some(self.format_amount(&amount_out_min, &token_out, 18)),
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
    
    fn decode_swap_exact_eth_for_tokens(&self, data: &str, value_hex: &str) -> Option<SwapInfo> {
        // swapExactETHForTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline)
        // Parameters: amountOutMin (0), path offset (1), to (2), deadline (3)
        // ETH amount comes from transaction value
        
        let amount_out_min = self.extract_param(data, 0)?;
        let path_offset = self.extract_param(data, 1)?;
        
        let (token_in, token_out) = self.extract_token_path(data, &path_offset);
        
        // Format ETH amount from transaction value
        let eth_amount = if let Ok(amount_wei) = u128::from_str_radix(value_hex.trim_start_matches("0x"), 16) {
            let eth_value = amount_wei as f64 / 1e18;
            format!("{:.4} ETH", eth_value)
        } else {
            "? ETH".to_string()
        };
        
        Some(SwapInfo {
            function_name: format!("Buy with ETH: {} → {}", token_in, token_out),
            token_in: "ETH".to_string(),
            token_out: token_out.clone(),
            amount_in: Some(eth_amount),
            amount_out_min: Some(self.format_amount(&amount_out_min, &token_out, 18)),
            raw_data: data.to_string(),
        })
    }
    
    fn decode_swap_exact_tokens_for_eth(&self, data: &str) -> Option<SwapInfo> {
        // swapExactTokensForETH(uint256 amountIn, uint256 amountOutMin, address[] path, address to, uint256 deadline)
        // Parameters: amountIn (0), amountOutMin (1), path offset (2), to (3), deadline (4)
        
        let amount_in = self.extract_param(data, 0)?;
        let amount_out_min = self.extract_param(data, 1)?;
        let path_offset = self.extract_param(data, 2)?;
        
        let (token_in, token_out) = self.extract_token_path(data, &path_offset);
        
        Some(SwapInfo {
            function_name: format!("Sell for ETH: {} → {}", token_in, token_out),
            token_in: token_in.clone(),
            token_out: "ETH".to_string(),
            amount_in: Some(self.format_amount(&amount_in, &token_in, 18)),
            amount_out_min: Some(self.format_amount(&amount_out_min, "ETH", 18)),
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
    
    fn decode_swap_exact_eth_for_tokens_supporting_fee(&self, data: &str, value_hex: &str) -> Option<SwapInfo> {
        // swapExactETHForTokensSupportingFeeOnTransferTokens(uint256 amountOutMin, address[] path, address to, uint256 deadline)
        // Parameters: amountOutMin (0), path offset (1), to (2), deadline (3)
        // ETH amount comes from transaction value
        
        let amount_out_min = self.extract_param(data, 0)?;
        let path_offset = self.extract_param(data, 1)?;
        
        let (token_in, token_out) = self.extract_token_path(data, &path_offset);
        
        // Format ETH amount from transaction value
        let eth_amount = if let Ok(amount_wei) = u128::from_str_radix(value_hex.trim_start_matches("0x"), 16) {
            let eth_value = amount_wei as f64 / 1e18;
            format!("{:.4} ETH", eth_value)
        } else {
            "? ETH".to_string()
        };
        
        Some(SwapInfo {
            function_name: format!("Buy with ETH (Fee): {} → {}", token_in, token_out),
            token_in: "ETH".to_string(),
            token_out: token_out.clone(),
            amount_in: Some(eth_amount),
            amount_out_min: Some(self.format_amount(&amount_out_min, &token_out, 18)),
            raw_data: data.to_string(),
        })
    }
    
    fn decode_exact_input_single(&self, data: &str) -> Option<SwapInfo> {
        // exactInputSingle(ExactInputSingleParams params)
        // struct ExactInputSingleParams {
        //     address tokenIn;      (offset 0)
        //     address tokenOut;     (offset 1) 
        //     uint24 fee;           (offset 2)
        //     address recipient;    (offset 3)
        //     uint256 deadline;     (offset 4)
        //     uint256 amountIn;     (offset 5)
        //     uint256 amountOutMinimum; (offset 6)
        //     uint160 sqrtPriceLimitX96; (offset 7)
        // }
        
        let token_in_param = self.extract_param(data, 0)?;
        let token_out_param = self.extract_param(data, 1)?;
        let amount_in = self.extract_param(data, 5)?;
        let amount_out_min = self.extract_param(data, 6)?;
        
        let token_in_addr = self.extract_address(&token_in_param);
        let token_out_addr = self.extract_address(&token_out_param);
        
        let token_in_symbol = self.get_token_symbol(&token_in_addr);
        let token_out_symbol = self.get_token_symbol(&token_out_addr);
        
        Some(SwapInfo {
            function_name: format!("V3 Swap: {} → {}", token_in_symbol, token_out_symbol),
            token_in: token_in_symbol.clone(),
            token_out: token_out_symbol.clone(),
            amount_in: Some(self.format_amount(&amount_in, &token_in_symbol, 18)),
            amount_out_min: Some(self.format_amount(&amount_out_min, &token_out_symbol, 18)),
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