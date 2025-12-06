#!/bin/bash

# Complete DEX Pool Scanning Script
# This will scan ~25.5 million blocks for ALL Uniswap V2 and V3 pools

echo "🚀 Starting Complete DEX Pool Scan"
echo "======================================"
echo "This process will:"
echo "• Scan 13,934,859 blocks for Uniswap V2 pools (May 2020 → now)"
echo "• Scan 11,565,955 blocks for Uniswap V3 pools (May 2021 → now)"
echo "• Total: ~25.5 million blocks"
echo "• Estimated time: 15-20 minutes"
echo "• Progress updates every ~30 seconds"
echo ""

# Get current pool count
CURRENT_POOLS=$(sqlite3 dex_pools.db "SELECT COUNT(*) FROM pools;" 2>/dev/null || echo "0")
echo "Current pools in database: $CURRENT_POOLS"
echo ""

# Confirm with user
read -p "Continue with complete scan? [y/N]: " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "❌ Scan cancelled"
    exit 1
fi

echo "✅ Starting comprehensive pool scan..."
echo "Monitor progress with: tail -f ethereum-monitor.log"
echo ""

# Run the complete scan
FORCE_POOL_REFRESH=1 DEBUG_MODE=1 RUST_LOG=info ./target/debug/ethereum-transaction-pool-monitor

echo ""
echo "🎉 Scan completed!"

# Show final results
FINAL_POOLS=$(sqlite3 dex_pools.db "SELECT COUNT(*) FROM pools;" 2>/dev/null || echo "0")
V2_POOLS=$(sqlite3 dex_pools.db "SELECT COUNT(*) FROM pools WHERE protocol='UniswapV2';" 2>/dev/null || echo "0")
V3_POOLS=$(sqlite3 dex_pools.db "SELECT COUNT(*) FROM pools WHERE protocol='UniswapV3';" 2>/dev/null || echo "0")

echo "======================================"
echo "📊 Final Pool Count:"
echo "• UniswapV2 pools: $V2_POOLS"
echo "• UniswapV3 pools: $V3_POOLS"
echo "• Total pools: $FINAL_POOLS"
echo "======================================"