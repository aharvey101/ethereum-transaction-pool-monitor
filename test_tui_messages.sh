#!/bin/bash

# Test TUI mode with logging
echo "Testing TUI mode message flow..."

# Clear previous logs
> ethereum-monitor.log

echo "Starting TUI mode with debug logging..."

# Start TUI in background with logs
RUST_LOG=info ./target/debug/ethereum-transaction-pool-monitor > tui_test.log 2>&1 &
TUI_PID=$!

echo "TUI started with PID: $TUI_PID"
echo "Waiting 15 seconds for message flow..."

# Wait for background tasks to start and send messages
sleep 15

# Kill the TUI
kill $TUI_PID 2>/dev/null
wait $TUI_PID 2>/dev/null

echo ""
echo "=== Checking for background transaction updater messages ==="
grep -i "background.*transaction" ethereum-monitor.log

echo ""
echo "=== Checking for message reception ==="  
grep -i "received.*transaction" ethereum-monitor.log

echo ""
echo "=== Checking for status updates ==="
grep -i "updated app status" ethereum-monitor.log

echo ""
echo "=== Latest log entries ==="
tail -20 ethereum-monitor.log