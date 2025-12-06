#!/bin/bash

echo "=================================================="
echo "Network & Mempool Diagnostics"
echo "=================================================="
echo ""

RPC_URL="${ETH_RPC_URL:-http://192.168.0.14:8545}"

# Check network info
echo "1. Network Information:"
CLIENT=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}' | jq -r '.result')
echo "   Client: $CLIENT"

CHAIN_ID=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | jq -r '.result')
echo "   Chain ID: $CHAIN_ID ($(printf '%d' $CHAIN_ID))"

BLOCK_NUM=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result')
BLOCK_DEC=$(printf '%d' $BLOCK_NUM)
echo "   Latest Block: $BLOCK_NUM ($BLOCK_DEC)"
echo ""

# Check block contents (recent blocks may have transactions)
echo "2. Checking Recent Blocks for Transactions:"
for i in 0 1 2 3 4; do
  BLOCK_TO_CHECK=$(printf '0x%x' $((BLOCK_DEC - i)))
  BLOCK=$(curl -s -X POST "$RPC_URL" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$BLOCK_TO_CHECK\",false],\"id\":1}")
  
  TX_COUNT=$(echo "$BLOCK" | jq '.result.transactions | length')
  BLOCK_LABEL=$(echo "$BLOCK" | jq -r '.result.number')
  
  echo "   Block $BLOCK_LABEL: $TX_COUNT transactions"
done
echo ""

# Check tx pool stats (if available)
echo "3. Checking Mempool Statistics:"
TXPOOL=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"txpool_status","params":[],"id":1}')

if echo "$TXPOOL" | jq -e '.result' > /dev/null 2>&1; then
  PENDING=$(echo "$TXPOOL" | jq -r '.result.pending')
  QUEUED=$(echo "$TXPOOL" | jq -r '.result.queued')
  echo "   Pending in pool: $(printf '%d' $PENDING)"
  echo "   Queued in pool: $(printf '%d' $QUEUED)"
else
  ERROR=$(echo "$TXPOOL" | jq -r '.error.message // "Unknown error"')
  echo "   txpool_status not available: $ERROR"
fi
echo ""

# Try alternative mempool methods
echo "4. Trying Alternative Methods:"

# eth_pendingTransactions (probably won't work but worth trying)
PENDING_TXS=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_pendingTransactions","params":[],"id":1}')

if echo "$PENDING_TXS" | jq -e '.result' > /dev/null 2>&1; then
  COUNT=$(echo "$PENDING_TXS" | jq '.result | length')
  echo "   eth_pendingTransactions: $COUNT transactions"
else
  ERROR=$(echo "$PENDING_TXS" | jq -r '.error.message // "Unknown"')
  echo "   eth_pendingTransactions: Not supported ($ERROR)"
fi

# parity_pendingTransactions (for Erigon/Parity)
PARITY_TXS=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"parity_pendingTransactions","params":[],"id":1}')

if echo "$PARITY_TXS" | jq -e '.result' > /dev/null 2>&1; then
  COUNT=$(echo "$PARITY_TXS" | jq '.result | length')
  echo "   parity_pendingTransactions: $COUNT transactions"
else
  ERROR=$(echo "$PARITY_TXS" | jq -r '.error.message // "Unknown"')
  echo "   parity_pendingTransactions: Not supported"
fi

echo ""
echo "5. Summary:"
echo "   If all pools show 0 transactions, it's normal for:"
echo "   • Private/test networks with low activity"
echo "   • Nodes that haven't synced mempool yet"
echo "   • Periods of low transaction volume"
echo ""
echo "   To generate test transactions, you could:"
echo "   • Use a testnet like Sepolia or Goerli"
echo "   • Create a simple transaction generator"
echo "   • Monitor during Ethereum mainnet peak hours"
echo ""
echo "=================================================="

