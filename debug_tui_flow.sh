#!/bin/bash

# Debug TUI Transaction Flow
# Run this in your actual terminal to debug the TUI transaction updates

echo "🔍 TUI Transaction Flow Debugging"
echo "=================================="
echo ""
echo "This script will:"
echo "1. Clear the log file"
echo "2. Start the TUI with debug logging"
echo "3. You should see transaction updates within 10 seconds"
echo "4. Press 'q' to quit when you see updates (or wait 30 seconds)"
echo "5. Check the logs for message flow"
echo ""

# Clear log
> ethereum-monitor.log

echo "Starting TUI with enhanced logging..."
echo "Watch for status changes from 'Starting transaction monitor...' to 'X pending transactions found'"
echo ""

# Start TUI with logging
RUST_LOG=info ./target/debug/ethereum-transaction-pool-monitor

echo ""
echo "🔍 Analyzing logs for transaction flow..."
echo ""

echo "=== Background Transaction Updater Initialization ==="
grep -i "background.*transaction.*initializing" ethereum-monitor.log

echo ""
echo "=== Background Updater Created Successfully ==="  
grep -i "EthereumClient created successfully" ethereum-monitor.log

echo ""
echo "=== Background Fetching Transactions ==="
grep -i "Background.*Got.*transactions" ethereum-monitor.log | head -3

echo ""
echo "=== Main Thread Message Reception ==="
grep -i "Main.*Received.*transactions" ethereum-monitor.log | head -3

echo ""
echo "=== Status Updates ==="
grep -i "Updated app status to" ethereum-monitor.log | head -3

echo ""
echo "If you see NO messages above, the background task isn't working."
echo "If you see background messages but no main thread messages, the channels aren't connected."
echo "If you see main thread messages, the issue is in the UI rendering."