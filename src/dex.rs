use std::collections::HashSet;
use lazy_static::lazy_static;

lazy_static! {
    static ref DEX_ADDRESSES: HashSet<&'static str> = {
        let mut set = HashSet::new();
        
        // Uniswap V3
        set.insert("0x1F98431c8aD98523631AE4a59f267346ea3113F");
        set.insert("0x68b3465833fb72B5A828cCEDA3187CF6cc380C86");
        set.insert("0xE592427A0AEce92De3Edee1F18E0157C05861564");
        set.insert("0x6E0E6b3A27B26F1fa9e66f18cE4bEe510a0F7eA7");
        
        // Uniswap V2 Router & Factory
        set.insert("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D");
        set.insert("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
        
        // Curve Finance
        set.insert("0x99a58482BD7490Cf8E3bfcA92e2A6b5F7e36c009");
        set.insert("0xDC24316b9AE028E5614BFa16D19dC5c08421f535");
        set.insert("0x06da148504c8ac6e11f10a21ee58f17a733f56e5");
        
        // 0x Protocol
        set.insert("0x61935CbD94231ADA75A0A2129e5dFC7d36eDA5d7");
        set.insert("0xDef1C0ded9bef7B1AcB7b8f6Ce78ffe3D5B11BAa");
        
        // 1inch
        set.insert("0x1111111254fb6c44bac0bed2854e76f90643097d");
        set.insert("0x111111111117dC0aa78b770fA6A738034120C302");
        
        // SushiSwap
        set.insert("0xd9e1cE17f2641f24aE9f7FFe6ff87D78ef7B26C1");
        set.insert("0xC0AEe478e3B480f1DFF3EA3199A02A6aA7Fa05eA");
        
        // Balancer
        set.insert("0xBA12222222228d8Ba445958a75a0704d566BF2C8");
        set.insert("0x9424B1412450D0f8Fc2255FAf6046b98213B76Bd");
        
        // Bancor
        set.insert("0xeEF417e1D5CC832e619ae0d397F7E407ac3906cc");
        set.insert("0xc0aEe478e3B480f1DFF3EA3199A02A6aA7Fa05eA");
        
        // Dodo
        set.insert("0x6B4712AE9797C199dc25EFa26D51df20d32e2b1f");
        set.insert("0x1B7A0291b1a9eeF8F2b5a0ee3Cfd36a6D9b62B1");
        
        // Kyber
        set.insert("0x714050623414B63975dA7B0D2E7b0fEfBB7cc856");
        set.insert("0xAA34bE99eFcc0474f45ecAa4475af735BFC15dA3");
        
        // Paraswap
        set.insert("0xDEF171Fe48CF0115B1d80b88dc8eAB59176FEe57");
        set.insert("0x216B4B4ba9F3e719726886d34a6e97dD1f9F6Ba");
        
        set
    };
}

/// Check if an address is a known DEX
#[allow(dead_code)]
pub fn is_dex(address: &str) -> bool {
    if address.len() < 42 {
        return false;
    }
    
    // Normalize address to lowercase for comparison
    let normalized = address.to_lowercase();
    
    // Check exact matches (case-insensitive)
    DEX_ADDRESSES.iter().any(|dex| {
        normalized == dex.to_lowercase()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniswap_v3_detection() {
        assert!(is_dex("0x1F98431c8aD98523631AE4a59f267346ea3113F"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_dex("0x1f98431c8ad98523631ae4a59f267346ea3113f"));
        assert!(is_dex("0x1F98431C8AD98523631AE4A59F267346EA3113F"));
    }

    #[test]
    fn test_non_dex() {
        assert!(!is_dex("0x0000000000000000000000000000000000000000"));
    }
}
