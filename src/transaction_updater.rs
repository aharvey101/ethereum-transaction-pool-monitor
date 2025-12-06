use tokio::sync::mpsc;
use anyhow::Result;
use crate::eth_client::{EthereumClient, MempoolTransaction};
use crate::pool_db::PoolDatabase;

#[derive(Debug)]
pub enum TransactionUpdateMessage {
    NewTransactions(Vec<MempoolTransaction>),
    Error(String),
}

pub struct BackgroundTransactionUpdater {
    _tx: mpsc::UnboundedSender<TransactionUpdateMessage>,
}

impl BackgroundTransactionUpdater {
    pub fn spawn(
        rpc_url: String,
        db_path: String,
        chain_id: u32,
    ) -> (Self, mpsc::UnboundedReceiver<TransactionUpdateMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::update_transactions_loop(&rpc_url, &db_path, chain_id, tx_clone).await {
                tracing::error!("Transaction updater error: {}", e);
            }
        });

        (
            BackgroundTransactionUpdater { _tx: tx },
            rx
        )
    }

    async fn update_transactions_loop(
        rpc_url: &str,
        db_path: &str,
        chain_id: u32,
        tx: mpsc::UnboundedSender<TransactionUpdateMessage>,
    ) -> Result<()> {
        // Initialize client and database
        let client = EthereumClient::new(rpc_url).await?;
        let pool_db = PoolDatabase::new(db_path)?;
        
        tracing::info!("Background transaction updater started");
        
        loop {
            match client.get_pending_transactions(&pool_db, chain_id).await {
                Ok(transactions) => {
                    tracing::debug!("Background: Got {} pending transactions", transactions.len());
                    let _ = tx.send(TransactionUpdateMessage::NewTransactions(transactions));
                }
                Err(e) => {
                    tracing::warn!("Background: Error fetching transactions: {}", e);
                    let _ = tx.send(TransactionUpdateMessage::Error(format!("Error fetching transactions: {}", e)));
                }
            }
            
            // Wait 5 seconds before next update (same as original interval)
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}