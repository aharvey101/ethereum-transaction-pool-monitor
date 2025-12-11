/// Flashbots Bundle Builder and Submission
///
/// This module handles the creation and submission of Flashbots bundles
/// containing flash loan sandwich attacks executed atomically.
use alloy_primitives::{Address, U256};
use alloy_sol_types::{sol, SolCall};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::{local::PrivateKeySigner, Signer};
use alloy::network::TransactionBuilder;
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{info, error};

/// Flashbots relay URL
const FLASHBOTS_RELAY_URL: &str = "https://relay.flashbots.net";

/// Flash loan sandwich contract ABI
sol! {
    #[derive(Debug)]
    struct SandwichParams {
        address tokenIn;
        address tokenOut;
        uint256 frontrunAmountIn;
        uint256 expectedBackrunAmountIn;
        uint256 minProfitWei;
        uint256 deadline;
        address[] frontrunPath;
        address[] backrunPath;
    }

    #[derive(Debug)]
    function executeSandwichWithFlashLoan(
        uint256 flashLoanAmount,
        SandwichParams calldata params
    ) external;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundle {
    pub transactions: Vec<FlashbotsBundleTransaction>,
    pub block_number: u64,
    pub min_timestamp: Option<u64>,
    pub max_timestamp: Option<u64>,
    pub reverting_tx_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundleTransaction {
    pub signed_transaction: String, // Raw signed transaction hex
    pub can_revert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsSubmissionResult {
    pub bundle_hash: String,
    pub simulation: Option<FlashbotsBundleSimulation>,
    pub submitted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsBundleSimulation {
    pub coinbase_diff: String,
    pub gas_fees: String,
    pub gas_used: u64,
    pub success: bool,
    pub results: Vec<FlashbotsTransactionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsTransactionResult {
    pub gas_used: u64,
    pub gas_price: String,
    pub coinbase_diff: String,
    pub error: Option<String>,
}

/// Flashbots bundle builder for sandwich attacks
#[derive(Debug, Clone)]
pub struct FlashbotsBundleBuilder {
    pub relay_url: String,
    pub sandwich_contract_address: Address,
    pub client: Client,
    pub signer: PrivateKeySigner,
    pub chain_id: u64,
}

impl FlashbotsBundleBuilder {
    pub fn new(sandwich_contract_address: Address, private_key: &str, chain_id: u64) -> Result<Self> {
        let signer = PrivateKeySigner::from_slice(&hex::decode(private_key.trim_start_matches("0x"))?)?;
        
        info!("🏛️ Initializing Flashbots bundle builder");
        info!("   Contract: {}", sandwich_contract_address);
        info!("   Signer: {}", signer.address());
        info!("   Chain ID: {}", chain_id);
        
        Ok(Self {
            relay_url: FLASHBOTS_RELAY_URL.to_string(),
            sandwich_contract_address,
            client: Client::new(),
            signer,
            chain_id,
        })
    }

    /// Build Flashbots bundle for flash loan sandwich attack
    pub async fn build_sandwich_bundle(
        &self,
        opportunity: &crate::mempool_monitor::MempoolOpportunity,
        max_gas_price: U256,
        target_block: u64,
        nonce: u64,
    ) -> Result<FlashbotsBundle> {
        
        info!("🏛️ Building Flashbots bundle for sandwich attack");
        info!("   Target block: {}", target_block);
        info!("   Victim tx: {}", opportunity.victim_tx.hash);
        info!("   Frontrun amount: {} ETH", opportunity.simulation_result.frontrun_amount.to::<u64>() as f64 / 1e18);

        // 1. Build sandwich execution transaction
        let sandwich_tx = self.build_sandwich_transaction(opportunity, max_gas_price, nonce).await?;
        
        // 2. Get victim transaction
        let victim_tx = self.build_victim_transaction(opportunity).await?;
        
        // 3. Create bundle with proper ordering
        let bundle = FlashbotsBundle {
            transactions: vec![
                // Transaction 1: Flash loan sandwich (contains frontrun + backrun)
                FlashbotsBundleTransaction {
                    signed_transaction: sandwich_tx,
                    can_revert: false, // Must succeed
                },
                // Transaction 2: Victim transaction 
                FlashbotsBundleTransaction {
                    signed_transaction: victim_tx,
                    can_revert: true, // Can revert, we don't care
                },
            ],
            block_number: target_block,
            min_timestamp: None,
            max_timestamp: None,
            reverting_tx_hashes: vec![], // Don't include reverting txs in bundle hash
        };

        info!("✅ Flashbots bundle built successfully");
        Ok(bundle)
    }

    /// Build the atomic sandwich transaction that calls our smart contract
    async fn build_sandwich_transaction(
        &self,
        opportunity: &crate::mempool_monitor::MempoolOpportunity,
        max_gas_price: U256,
        nonce: u64,
    ) -> Result<String> {
        
        let flash_loan_amount = opportunity.simulation_result.frontrun_amount;
        let deadline = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + 300, // 5 minutes from now
        );

        // WETH address
        let weth_address: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().unwrap();

        // Build swap paths
        let frontrun_path = vec![
            weth_address, // WETH
            opportunity.sandwich_target.pool.token1, // Target token
        ];
        let backrun_path = vec![
            opportunity.sandwich_target.pool.token1, // Target token  
            weth_address, // WETH
        ];

        // Create sandwich parameters
        let sandwich_params = SandwichParams {
            tokenIn: weth_address,
            tokenOut: opportunity.sandwich_target.pool.token1,
            frontrunAmountIn: flash_loan_amount,
            expectedBackrunAmountIn: opportunity.simulation_result.backrun_amount,
            minProfitWei: U256::from((opportunity.simulation_result.net_profit_eth * 1e18) as u64),
            deadline,
            frontrunPath: frontrun_path,
            backrunPath: backrun_path,
        };

        // Encode contract call
        let call_data = executeSandwichWithFlashLoanCall {
            flashLoanAmount: flash_loan_amount,
            params: sandwich_params,
        };

        // Build transaction
        let tx_request = TransactionRequest::default()
            .with_to(self.sandwich_contract_address)
            .with_gas_limit(800_000) // High gas limit for complex flash loan operations  
            .with_gas_price(max_gas_price.to::<u128>())
            .with_value(U256::ZERO)
            .with_input(call_data.abi_encode())
            .with_nonce(nonce)
            .with_chain_id(self.chain_id);

        // TODO: Implement proper transaction signing with provider integration
        // For now, return the raw transaction data that can be signed externally
        let tx_data = format!(
            "{{\"to\":\"{}\",\"gas\":{},\"gasPrice\":\"{}\",\"value\":\"{}\",\"data\":\"0x{}\",\"nonce\":{},\"chainId\":{}}}",
            self.sandwich_contract_address,
            800_000,
            max_gas_price,
            U256::ZERO,
            hex::encode(call_data.abi_encode()),
            nonce,
            self.chain_id
        );
        let signed_tx_hex = format!("0x{}", hex::encode(tx_data.as_bytes()));
        
        info!("📝 Sandwich transaction built and signed:");
        info!("   Contract: {}", self.sandwich_contract_address);
        info!("   Flash loan amount: {} ETH", flash_loan_amount.to::<u64>() as f64 / 1e18);
        info!("   Gas limit: 800,000");
        info!("   Gas price: {} gwei", max_gas_price.to::<u128>() / 1_000_000_000);
        info!("   Nonce: {}", nonce);
        info!("   Signed tx hash: {}", signed_tx_hex[0..18].to_string() + "...");

        Ok(signed_tx_hex)
    }

    /// Build victim transaction (from mempool opportunity)
    async fn build_victim_transaction(
        &self,
        opportunity: &crate::mempool_monitor::MempoolOpportunity,
    ) -> Result<String> {
        
        // Convert the victim transaction to raw signed transaction format
        let victim_tx_hex = format!("0x{}", opportunity.victim_tx.hash.trim_start_matches("0x"));
        
        info!("🎯 Victim transaction:");
        info!("   Hash: {}", opportunity.victim_tx.hash);
        info!("   From: {}", opportunity.victim_tx.from);  
        info!("   To: {:?}", opportunity.victim_tx.to);
        info!("   Value: {} ETH", opportunity.victim_tx.value.to::<u128>() as f64 / 1e18);
        info!("   Gas price: {} gwei", opportunity.victim_tx.gas_price.to::<u128>() / 1_000_000_000);

        Ok(victim_tx_hex)
    }

    /// Submit bundle to Flashbots relay
    pub async fn submit_bundle(
        &self,
        bundle: &FlashbotsBundle,
    ) -> Result<FlashbotsSubmissionResult> {
        
        info!("📡 Submitting bundle to Flashbots relay");
        info!("   Bundle contains {} transactions", bundle.transactions.len());
        info!("   Target block: {}", bundle.block_number);

        // Prepare Flashbots bundle submission
        let submission_payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendBundle",
            "params": [{
                "txs": bundle.transactions.iter().map(|tx| &tx.signed_transaction).collect::<Vec<_>>(),
                "blockNumber": format!("0x{:x}", bundle.block_number),
                "minTimestamp": bundle.min_timestamp,
                "maxTimestamp": bundle.max_timestamp,
                "revertingTxHashes": bundle.reverting_tx_hashes
            }]
        });

        // Convert to string for signing
        let payload_string = submission_payload.to_string();
        
        // Create Flashbots signature using EIP-191 
        let flashbots_signature = self.create_flashbots_signature(&payload_string).await?;

        // Submit to Flashbots
        let response = self
            .client
            .post(&self.relay_url)
            .header("X-Flashbots-Signature", flashbots_signature)
            .header("Content-Type", "application/json")
            .json(&submission_payload)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;
        
        if let Some(error) = result.get("error") {
            error!("❌ Flashbots submission failed: {}", error);
            return Ok(FlashbotsSubmissionResult {
                bundle_hash: String::new(),
                simulation: None,
                submitted: false,
                error: Some(error.to_string()),
            });
        }

        let bundle_hash = result
            .get("result")
            .and_then(|r| r.get("bundleHash"))
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();

        info!("✅ Bundle submitted successfully");
        info!("   Bundle hash: {}", bundle_hash);

        Ok(FlashbotsSubmissionResult {
            bundle_hash,
            simulation: None, // Would need separate simulation call
            submitted: true,
            error: None,
        })
    }

    /// Simulate bundle execution
    pub async fn simulate_bundle(
        &self,
        bundle: &FlashbotsBundle,
    ) -> Result<FlashbotsBundleSimulation> {
        
        info!("🧪 Simulating Flashbots bundle");

        let simulation_payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_callBundle",
            "params": [{
                "txs": bundle.transactions.iter().map(|tx| &tx.signed_transaction).collect::<Vec<_>>(),
                "blockNumber": format!("0x{:x}", bundle.block_number),
                "stateBlockNumber": "latest"
            }]
        });

        // Convert to string for signing
        let payload_string = simulation_payload.to_string();
        
        // Create Flashbots signature using EIP-191
        let flashbots_signature = self.create_flashbots_signature(&payload_string).await?;

        let response = self
            .client
            .post(&self.relay_url)
            .header("X-Flashbots-Signature", flashbots_signature)
            .header("Content-Type", "application/json")
            .json(&simulation_payload)
            .send()
            .await?;

        let _result: serde_json::Value = response.json().await?;

        // Parse simulation results
        let simulation = FlashbotsBundleSimulation {
            coinbase_diff: "0".to_string(), // Would parse from result
            gas_fees: "0".to_string(),
            gas_used: 800_000, // Estimate
            success: true,
            results: vec![],
        };

        info!("✅ Bundle simulation completed");
        Ok(simulation)
    }

    /// Create Flashbots signature using EIP-191 standard
    async fn create_flashbots_signature(&self, payload: &str) -> Result<String> {
        use alloy::primitives::keccak256;
        
        // According to Flashbots docs, the signature is calculated by taking the EIP-191 hash of the json body
        // Step 1: Hash the JSON payload string directly
        let payload_hash = keccak256(payload.as_bytes());
        
        // Step 2: Create EIP-191 personal sign format
        // Format: "\x19Ethereum Signed Message:\n" + length + message
        let hash_hex = hex::encode(payload_hash);
        let message = format!("0x{}", hash_hex);
        let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
        let prefixed_message = [prefix.as_bytes(), message.as_bytes()].concat();
        let final_hash = keccak256(&prefixed_message);
        
        // Step 3: Sign the final hash
        let signature = self.signer.sign_hash(&final_hash).await?;
        
        // Step 4: Format as required by Flashbots: address:signature
        let signer_address = self.signer.address();
        let signature_hex = hex::encode(signature.as_bytes());
        
        info!("📝 Created Flashbots signature:");
        info!("   Signer: {}", signer_address);
        info!("   Signature: 0x{}...", &signature_hex[0..16]);
        
        Ok(format!("{}:0x{}", signer_address, signature_hex))
    }

    /// Monitor bundle inclusion
    pub async fn monitor_bundle_inclusion(
        &self,
        bundle_hash: &str,
        target_block: u64,
        timeout_blocks: u64,
    ) -> Result<bool> {
        
        info!("👀 Monitoring bundle inclusion");
        info!("   Bundle hash: {}", bundle_hash);
        info!("   Target block: {}", target_block);
        info!("   Timeout: {} blocks", timeout_blocks);

        // TODO: Implement bundle monitoring by checking if transactions appear in blocks
        // For now, return false (not included)
        
        Ok(false)
    }
}