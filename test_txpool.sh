#!/bin/bash

echo "Testing txpool_content method..."
echo ""

curl -s -X POST http://192.168.0.14:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"txpool_content","params":[],"id":1}' | jq '{
    pending_count: (.result.pending | keys | length),
    queued_count: (.result.queued | keys | length),
    first_pending_address: (.result.pending | keys[0]),
    first_pending_tx: (
      .result.pending 
      | to_entries[0].value 
      | to_entries[0].value
    )
  }'
