#!/bin/bash
cd /Users/alexander/development/ethereum-transaction-pool-monitor
echo "Building and running with debug output..."
cargo build --release
echo "Starting application (will output debug info)..."
echo "Press arrow keys to test selection, 'q' to quit"
cargo run --release