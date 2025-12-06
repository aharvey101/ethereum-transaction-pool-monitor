#!/bin/bash
echo "Starting performance debug..."
cd /Users/alexander/development/ethereum-transaction-pool-monitor
RUST_LOG=debug cargo run 2>&1 | grep -E "(select_next|ensure_cache_valid|took|Complete|Starting)" | tee performance_debug.log &
PID=$!
echo "Process started with PID: $PID"
echo "Logs will be saved to performance_debug.log"
echo "Use 'kill $PID' to stop"
echo "Or press Ctrl+C to exit and the process will continue in background"
wait $PID