#!/bin/bash

echo "=================================================="
echo "Fetching Pending Transactions from Your Node"
echo "=================================================="
echo ""

# Fetch txpool content
RESPONSE=$(curl -s -X POST http://192.168.0.14:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"txpool_content","params":[],"id":1}')

PENDING_COUNT=$(echo "$RESPONSE" | jq '.result.pending | keys | length')
QUEUED_COUNT=$(echo "$RESPONSE" | jq '.result.queued | keys | length')

echo "Pool Statistics:"
echo "  Pending transactions: $PENDING_COUNT"
echo "  Queued transactions: $QUEUED_COUNT"
echo ""
echo "Sample of First 5 Pending Transactions:"
echo "=================================================="
echo ""

# Extract and display first 5 pending transactions
echo "$RESPONSE" | jq -r '
  .result.pending 
  | to_entries[0:5]
  | .[]
  | "\(.key) (Address: \(.key))"
  | . as $addr
  | . + (
      (. | rtrimstr("")) as $x |
      "\nTransactions:"
    )
' | while read line; do
  echo "$line"
done

# Show detailed view of first transaction
echo ""
echo "Detailed View of First Transaction:"
echo "=================================================="

FIRST_TX=$(echo "$RESPONSE" | jq -r '
  .result.pending 
  | to_entries[0]
  | .value
  | to_entries[0]
  | .value
')

echo "$FIRST_TX" | jq '{
  hash: .hash,
  from: .from,
  to: .to,
  value_wei: .value,
  gas: .gas,
  gasPrice_wei: .gasPrice,
  nonce: .nonce,
  type: .type
}'

echo ""
echo "Value in ETH: $(echo "$FIRST_TX" | jq -r '.value | tonumber' | awk '{printf "%.6f\n", $1 / 1e18}')"
echo "Gas Price in Gwei: $(echo "$FIRST_TX" | jq -r '.gasPrice | tonumber' | awk '{printf "%.2f\n", $1 / 1e9}')"
echo ""
echo "=================================================="
echo "Monitor is working! You should see these txs in the app."
echo "=================================================="

