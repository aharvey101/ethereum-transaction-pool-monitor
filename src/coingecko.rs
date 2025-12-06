use anyhow::Result;
use serde_json::Value;
use crate::pool_db::DexPool;

/// CoinGecko API configuration
pub struct CoinGeckoClient {
    http_client: std::sync::Arc<reqwest::Client>,
    base_url: String,
}

impl CoinGeckoClient {
    /// Create a new CoinGecko API client
    pub fn new() -> Self {
        CoinGeckoClient {
            http_client: std::sync::Arc::new(reqwest::Client::new()),
            base_url: "https://api.coingecko.com/api/v3/onchain".to_string(),
        }
    }

    /// Fetch pools from CoinGecko for a specific network and DEX
    /// Note: Free tier works without API key but has rate limits
    pub async fn fetch_pools(
        &self,
        network: &str,
        dex: &str,
        page: u32,
    ) -> Result<(Vec<DexPool>, bool)> {
        let url = format!(
            "{}/networks/{}/dexes/{}/pools?page={}&include=base_token,quote_token",
            self.base_url, network, dex, page
        );

        tracing::debug!("Fetching pools from CoinGecko: {}", url);
        let response = self.http_client.get(&url).send().await?;
        let body: Value = response.json().await?;

        // Parse pools from response
        let mut pools = Vec::new();

        if let Some(data_array) = body.get("data").and_then(|d| d.as_array()) {
            tracing::debug!("Found {} pool records for {}/{} page {}", data_array.len(), network, dex, page);
            for pool_data in data_array {
                if let Ok(pool) = parse_coingecko_pool(pool_data) {
                    pools.push(pool);
                }
            }
        } else {
            tracing::warn!("No data found in CoinGecko response for {}/{} page {}", network, dex, page);
        }

        // Check if there are more pages (max 20 per page)
        let has_more = pools.len() >= 20;

        Ok((pools, has_more))
    }

    /// Fetch pools for multiple DEXes on a network
    pub async fn fetch_all_pools(
        &self,
        network: &str,
        dexes: &[&str],
        max_pages: u32,
    ) -> Result<Vec<DexPool>> {
        let mut all_pools = Vec::new();

        for dex in dexes {
            let mut page = 1;
            loop {
                match self.fetch_pools(network, dex, page).await {
                    Ok((pools, has_more)) => {
                        tracing::info!("Fetched {} pools for {}/{} page {}", pools.len(), network, dex, page);
                        all_pools.extend(pools);
                        if !has_more || page >= max_pages {
                            break;
                        }
                        page += 1;
                    }
                    Err(e) => {
                        tracing::error!("Error fetching pools for {}/{}: {}", network, dex, e);
                        break;
                    }
                }
            }
        }

        Ok(all_pools)
    }
}

/// Parse a CoinGecko pool response into a DexPool
fn parse_coingecko_pool(pool_data: &Value) -> Result<DexPool> {
    let attributes = pool_data
        .get("attributes")
        .ok_or_else(|| anyhow::anyhow!("Missing attributes"))?;

    let address = attributes
        .get("address")
        .and_then(|a| a.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing pool address"))?
        .to_string();

    // Get protocol from relationships
    let protocol = pool_data
        .get("relationships")
        .and_then(|r| r.get("dex"))
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("id"))
        .and_then(|id| id.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Get token addresses from included data or relationships
    let base_token = pool_data
        .get("relationships")
        .and_then(|r| r.get("base_token"))
        .and_then(|bt| bt.get("data"))
        .and_then(|d| d.get("id"))
        .and_then(|id| id.as_str())
        .map(|id| {
            // Extract address from id (format: "eth_0x...")
            id.split('_').nth(1).unwrap_or(id).to_string()
        });

    let quote_token = pool_data
        .get("relationships")
        .and_then(|r| r.get("quote_token"))
        .and_then(|qt| qt.get("data"))
        .and_then(|d| d.get("id"))
        .and_then(|id| id.as_str())
        .map(|id| {
            // Extract address from id (format: "eth_0x...")
            id.split('_').nth(1).unwrap_or(id).to_string()
        });

    Ok(DexPool {
        address,
        protocol,
        token0: base_token,
        token1: quote_token,
        chain_id: 1, // Default to Ethereum mainnet (1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pool() {
        let json_str = r#"{
            "id": "eth_0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
            "type": "pool",
            "attributes": {
                "address": "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640",
                "name": "WETH / USDC 0.05%"
            },
            "relationships": {
                "base_token": {
                    "data": {
                        "id": "eth_0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                        "type": "token"
                    }
                },
                "quote_token": {
                    "data": {
                        "id": "eth_0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                        "type": "token"
                    }
                },
                "dex": {
                    "data": {
                        "id": "uniswap_v3",
                        "type": "dex"
                    }
                }
            }
        }"#;

        let value: Value = serde_json::from_str(json_str).unwrap();
        let pool = parse_coingecko_pool(&value).unwrap();

        assert_eq!(pool.address, "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640");
        assert_eq!(pool.protocol, "uniswap_v3");
        assert_eq!(
            pool.token0,
            Some("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".to_string())
        );
    }
}
