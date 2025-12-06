#!/bin/bash

# Test background transaction updater in isolation
echo "Testing background transaction updater..."

# Clear log
> ethereum-monitor.log

echo "Starting headless mode to test background transaction updates..."

# Run headless mode for 10 seconds
(DEBUG_MODE=1 RUST_LOG=info ./target/debug/ethereum-transaction-pool-monitor &); 
HEADLESS_PID=$!
sleep 10
kill $HEADLESS_PID 2>/dev/null

echo ""
echo "=== Checking transaction updates in headless mode ==="
grep -i "transaction update" ethereum-monitor.log | head -5

echo ""
echo "=== Checking if transactions were fetched ==="
grep -i "fetched.*transactions" ethereum-monitor.log | head -3