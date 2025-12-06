#!/bin/bash

# Test TUI mode briefly
cd /Users/alexander/development/ethereum-transaction-pool-monitor

echo "Starting TUI test - will run for 15 seconds then exit..."

# Kill any existing processes
pkill -f ethereum-transaction-pool-monitor

# Start TUI mode in background
RUST_LOG=info timeout 15s ./target/debug/ethereum-transaction-pool-monitor &
TUI_PID=$!

# Wait for the process to start
sleep 2

# Check if it's running
if ps -p $TUI_PID > /dev/null 2>&1; then
    echo "TUI process started successfully (PID: $TUI_PID)"
    
    # Let it run for a while
    sleep 10
    
    # Check if it's still running
    if ps -p $TUI_PID > /dev/null 2>&1; then
        echo "TUI process still running after 10 seconds - SUCCESS"
    else
        echo "TUI process crashed"
    fi
else
    echo "TUI process failed to start"
fi

# Clean up
kill $TUI_PID 2>/dev/null
wait $TUI_PID 2>/dev/null

echo "Test complete. Checking logs..."
echo "=== Recent log entries ==="
tail -10 ethereum-monitor.log