use crate::eth_client::{EthereumClient, MempoolTransaction};
use crate::pool_db::PoolDatabase;
use anyhow::Result;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum TransactionUpdateMessage {
    NewTransactions(Vec<MempoolTransaction>),
    BlockChange {
        new_block_number: u64,
        transactions: Vec<MempoolTransaction>,
    },
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
            if let Err(e) =
                Self::update_transactions_loop(&rpc_url, &db_path, chain_id, tx_clone).await
            {
                tracing::error!("Transaction updater error: {}", e);
            }
        });

        (BackgroundTransactionUpdater { _tx: tx }, rx)
    }

    async fn update_transactions_loop(
        rpc_url: &str,
        db_path: &str,
        chain_id: u32,
        tx: mpsc::UnboundedSender<TransactionUpdateMessage>,
    ) -> Result<()> {
        tracing::info!("Background transaction updater: Initializing...");
        tracing::info!("RPC URL: {}", rpc_url);
        tracing::info!("DB Path: {}", db_path);
        tracing::info!("Chain ID: {}", chain_id);

        // Initialize client and database
        tracing::info!("Creating EthereumClient...");
        let client = match EthereumClient::new(rpc_url).await {
            Ok(client) => {
                tracing::info!("EthereumClient created successfully");
                client
            }
            Err(e) => {
                tracing::error!("Failed to create EthereumClient: {}", e);
                let _ = tx.send(TransactionUpdateMessage::Error(format!(
                    "Failed to create EthereumClient: {}",
                    e
                )));
                return Err(e);
            }
        };

        tracing::info!("Creating PoolDatabase...");
        let pool_db = match PoolDatabase::new(db_path) {
            Ok(db) => {
                tracing::info!("PoolDatabase created successfully");
                db
            }
            Err(e) => {
                tracing::error!("Failed to create PoolDatabase: {}", e);
                let _ = tx.send(TransactionUpdateMessage::Error(format!(
                    "Failed to create PoolDatabase: {}",
                    e
                )));
                return Err(e);
            }
        };

        tracing::info!("Background transaction updater started successfully");

        let mut last_block_number: Option<u64> = None;

        loop {
            tracing::debug!("Background: Starting transaction fetch...");

            // Check for new block first
            match client.get_latest_block_number().await {
                Ok(current_block) => {
                    let block_changed = last_block_number.map_or(true, |last| current_block > last);

                    // Fetch transactions
                    match client
                        .get_pending_transactions_paginated(&pool_db, chain_id, 0, 2000)
                        .await
                    {
                        Ok(transactions) => {
                            if block_changed {
                                tracing::info!("Background: New block {} detected with {} pending transactions",
                                    current_block, transactions.len());
                                let _ = tx.send(TransactionUpdateMessage::BlockChange {
                                    new_block_number: current_block,
                                    transactions,
                                });
                                last_block_number = Some(current_block);
                            } else {
                                tracing::info!(
                                    "Background: Got {} pending transactions (sorted by gas price)",
                                    transactions.len()
                                );
                                let _ = tx
                                    .send(TransactionUpdateMessage::NewTransactions(transactions));
                            }
                        }
                        Err(e) => {
                            tracing::error!("Background: Error fetching transactions: {}", e);
                            let _ = tx.send(TransactionUpdateMessage::Error(format!(
                                "Error fetching transactions: {}",
                                e
                            )));
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Background: Error fetching block number: {}", e);
                    let _ = tx.send(TransactionUpdateMessage::Error(format!(
                        "Error fetching block number: {}",
                        e
                    )));
                }
            }

            tracing::debug!("Background: Sleeping for 5 seconds...");
            // Wait 5 seconds before next update (same as original interval)
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }
}
