#!/bin/bash
# Quick Start Guide for Ethereum Mempool Monitor

echo "================================================"
echo "Ethereum Mempool Monitor - Quick Start"
echo "================================================"
echo ""
echo "This script will help you get started with the monitor."
echo ""

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "Rust not found. Please install it from https://rustup.rs/"
    exit 1
fi

echo "✓ Rust is installed"
echo ""
echo "To run the monitor, choose an option:"
echo ""
echo "1. Default (localhost:8545):"
echo "   cargo run --release"
echo ""
echo "2. Custom RPC URL:"
echo "   ETH_RPC_URL=http://your-rpc-url:8545 cargo run --release"
echo ""
echo "3. Quick build and run:"
echo "   cargo build --release"
echo "   ./target/release/ethereum-transaction-pool-monitor"
echo ""
echo "Make sure your Ethereum node is running!"
echo ""
echo "Example local node setup:"
echo ""
echo "Geth:"
echo "  geth --http --http.port 8545 --http.api eth,web3,net"
echo ""
echo "Erigon:"
echo "  erigon --http --http.port 8545 --http.api eth,web3,net"
echo ""
echo "================================================"
