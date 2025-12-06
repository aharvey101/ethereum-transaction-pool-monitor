#!/bin/bash

# Test eth_getLogs with proper format
curl -s -X POST http://192.168.0.14:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "eth_getLogs",
    "params": [{
      "address": "0x1F98431c8aD98523631AE4a59f267346ea3113F",
      "topics": ["0x783cca1c0412dd0d695e784568c96da80eb3cc59c1b71e2e53d63fe50b912e4f"],
      "fromBlock": "0xbd18fb",
      "toBlock": "0xbd19fb"
    }],
    "id": 1
  }' | jq '.result | length'
