#!/bin/bash

echo "=================================================="
echo "Testing Ethereum Node Connection"
echo "=================================================="
echo ""

RPC_URL="${ETH_RPC_URL:-http://192.168.0.14:8545}"
echo "Testing RPC URL: $RPC_URL"
echo ""

# Test 1: Basic connectivity
echo "Test 1: Basic connectivity..."
if curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}' | grep -q "result"; then
  echo "✓ Node is responding"
  VERSION=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}' | jq -r '.result')
  echo "  Client: $VERSION"
else
  echo "✗ Node is not responding. Check:"
  echo "  - Is the node running?"
  echo "  - Is the RPC URL correct? ($RPC_URL)"
  echo "  - Is the port open?"
  exit 1
fi
echo ""

# Test 2: eth_blockNumber
echo "Test 2: Checking block number..."
if BLOCK=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result'); then
  echo "✓ Latest block: $BLOCK ($(printf '%d' $BLOCK) in decimal)"
else
  echo "✗ Failed to get block number"
  exit 1
fi
echo ""

# Test 3: eth_newPendingTransactionFilter
echo "Test 3: Creating pending transaction filter..."
if FILTER=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_newPendingTransactionFilter","params":[],"id":1}' | jq -r '.result'); then
  echo "✓ Filter created: $FILTER"
  
  # Test 4: eth_getFilterChanges
  echo ""
  echo "Test 4: Checking for pending transactions..."
  if TX_HASHES=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getFilterChanges\",\"params\":[\"$FILTER\"],\"id\":1}" | jq '.result | length'); then
    echo "✓ Filter changes retrieved"
    echo "  Pending transactions found: $TX_HASHES"
  else
    echo "✗ Failed to get filter changes"
    exit 1
  fi
else
  echo "✗ Failed to create pending transaction filter"
  echo "  Your node may not support eth_newPendingTransactionFilter"
  echo "  Supported nodes: Geth, Erigon, Reth, Besu"
  exit 1
fi
echo ""

# Test 5: eth_getTransactionByHash
if [ "$TX_HASHES" -gt 0 ]; then
  echo "Test 5: Fetching transaction details..."
  FIRST_HASH=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getFilterChanges\",\"params\":[\"$FILTER\"],\"id\":1}" | jq -r '.result[0]')
  
  if TX_DATA=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionByHash\",\"params\":[\"$FIRST_HASH\"],\"id\":1}"); then
    FROM=$(echo "$TX_DATA" | jq -r '.result.from')
    TO=$(echo "$TX_DATA" | jq -r '.result.to')
    echo "✓ Transaction fetched successfully"
    echo "  From: $FROM"
    echo "  To: $TO"
  else
    echo "✗ Failed to fetch transaction details"
    exit 1
  fi
else
  echo "Test 5: Skipped (no pending transactions at the moment)"
fi
echo ""

echo "=================================================="
echo "✓ All tests passed! Your setup is ready."
echo "=================================================="
echo ""
echo "You can now run the monitor:"
echo "  cargo run --release"
echo ""
