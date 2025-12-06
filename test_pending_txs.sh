#!/bin/bash

echo "=================================================="
echo "Testing Pending Transaction Detection"
echo "=================================================="
echo ""

RPC_URL="${ETH_RPC_URL:-http://192.168.0.14:8545}"
echo "Testing RPC URL: $RPC_URL"
echo ""

# Step 1: Create filter
echo "Step 1: Creating pending transaction filter..."
FILTER_RESPONSE=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_newPendingTransactionFilter","params":[],"id":1}')

FILTER_ID=$(echo "$FILTER_RESPONSE" | jq -r '.result')
echo "Filter ID: $FILTER_ID"
echo ""

# Step 2: Check initial filter changes
echo "Step 2: Checking filter changes (initial)..."
CHANGES=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getFilterChanges\",\"params\":[\"$FILTER_ID\"],\"id\":1}" | jq '.result | length')
echo "Found $CHANGES transactions in initial poll"
echo ""

# Step 3: Wait and poll again
echo "Step 3: Waiting 3 seconds and polling again..."
echo "(This allows time for new transactions to appear)"
sleep 3

CHANGES=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getFilterChanges\",\"params\":[\"$FILTER_ID\"],\"id\":1}")

TX_COUNT=$(echo "$CHANGES" | jq '.result | length')
echo "Found $TX_COUNT new transactions"

if [ "$TX_COUNT" -gt 0 ]; then
  echo ""
  echo "SUCCESS! Found pending transactions:"
  echo "$CHANGES" | jq '.result | .[]'
  echo ""
  
  # Fetch details for first transaction
  FIRST_TX=$(echo "$CHANGES" | jq -r '.result[0]')
  echo "Fetching details for first transaction: $FIRST_TX"
  
  TX_DETAILS=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionByHash\",\"params\":[\"$FIRST_TX\"],\"id\":1}")
  
  echo ""
  echo "Transaction Details:"
  echo "$TX_DETAILS" | jq '.result | {from, to, value, gas, gasPrice, nonce, input}'
else
  echo ""
  echo "⚠️  No pending transactions found in this poll"
  echo ""
  echo "This is normal if:"
  echo "  • Network activity is low"
  echo "  • No transactions were added in the last 3 seconds"
  echo "  • The filter expired (try again)"
  echo ""
  echo "To see transactions, you can:"
  echo "  • Wait during peak network hours"
  echo "  • Run a transaction on your network"
  echo "  • Use an external transaction generator"
  echo "  • Check with a testnet that has more activity"
fi

echo ""
echo "=================================================="
echo "Test Complete"
echo "=================================================="
