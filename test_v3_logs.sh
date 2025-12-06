#!/bin/bash

RPC="http://192.168.0.14:8545"

# Get current block
CURRENT_BLOCK=$(curl -s -X POST "$RPC" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result')

echo "Current block: $CURRENT_BLOCK"

# Convert to decimal
BLOCK_DEC=$((16#${CURRENT_BLOCK:2}))
echo "Block decimal: $BLOCK_DEC"

# Query V3 PoolCreated logs from last 5000 blocks
FROM_BLOCK=$((BLOCK_DEC - 5000))
echo "Querying blocks $FROM_BLOCK to $BLOCK_DEC"

curl -s -X POST "$RPC" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getLogs",
    "params":[{
      "fromBlock":"0x'$(printf '%x' $FROM_BLOCK)'",
      "toBlock":"0x'$(printf '%x' $BLOCK_DEC)'",
      "address":"0x1F98431c8aD98523631AE4a59f267346ea3113F",
      "topics":["0x783ccc54c1f35d4b5ecec96408a16c643c61bd02f36e6e4df17e8e0e27f3e6ab"]
    }],
    "id":1
  }' | jq '.result | length'

