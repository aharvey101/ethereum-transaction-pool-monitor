//! Advanced Ethereum MEV Bot - Production-ready sandwich attack automation
//!
//! This library provides direct mempool execution capabilities for MEV (Maximal Extractable Value)
//! sandwich attacks, similar to the arboo approach, bypassing Flashbots for immediate execution.

#![allow(dead_code)] // Allow unused code in production MEV bot - many components are API/future use

pub mod bot_runner;
pub mod dex;
pub mod enhanced_revm_simulator;
pub mod eth_client;
pub mod flash_loan_manager;
pub mod flashbots_bundle_builder;
pub mod graph_client;
pub mod mempool_monitor;
pub mod mev_bundle_builder;
pub mod pool_db;
pub mod pool_fetcher;
pub mod pool_state_fetcher;
pub mod sandwich_pool_integration;
pub mod transaction_decoder;
pub mod transaction_executor;
