# Ethereum Mempool Monitor - RPC Method Guide

## RPC Methods Supported

This monitor now supports multiple RPC methods for fetching pending transactions, depending on what your node supports.

### Primary Method: `eth_newPendingTransactionFilter` + `eth_getFilterChanges`

This is the **recommended** method and is now the default. It works with:
- **Reth** (v1.0+)
- **Geth** (most versions)
- **Erigon** (most versions)
- **Besu** (most versions)
- **Infura** (with proper setup)

**How it works:**
1. Creates a filter with `eth_newPendingTransactionFilter` to watch pending transactions
2. Polls the filter with `eth_getFilterChanges` to get new transaction hashes
3. Fetches full transaction details with `eth_getTransactionByHash` for each hash

### Alternative Methods (Not Currently Supported)

- `eth_pendingTransactions` - Only supported by some nodes (Geth with special compilation)
- WebSocket subscriptions - Better performance but requires different connection

## Configuration

### Default RPC URL
The default RPC URL is now set to `http://192.168.0.14:8545`

### Using a Different RPC URL

```bash
ETH_RPC_URL=http://your-node:8545 cargo run --release
```

### Examples

**Local Geth:**
```bash
ETH_RPC_URL=http://localhost:8545 cargo run --release
```

**Local Reth:**
```bash
ETH_RPC_URL=http://localhost:8545 cargo run --release
```

**Remote Server:**
```bash
ETH_RPC_URL=http://192.168.0.14:8545 cargo run --release
```

## Troubleshooting

### "Method not found" Error
This typically means your RPC endpoint doesn't support the required methods. Check:

1. **Your node is running:**
   ```bash
   curl http://192.168.0.14:8545 -X POST \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}'
   ```

2. **Node supports `eth_newPendingTransactionFilter`:**
   ```bash
   curl http://192.168.0.14:8545 -X POST \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_newPendingTransactionFilter","params":[],"id":1}'
   ```

3. **The RPC endpoint is properly exposed:**
   - Check firewall settings
   - Verify the port is correct (default 8545)
   - Ensure your node allows RPC connections

### No Pending Transactions Appearing

This is normal! Reasons include:
- **Low network activity** - Try during peak hours
- **Private network** - May have fewer or no transactions
- **Filter expired** - The app will create a new filter automatically
- **Your address isn't involved** - The filter shows all pending transactions

### Connection Timeout

If you get connection timeout errors:
1. Verify the RPC URL is correct
2. Check if the node is running: `curl http://192.168.0.14:8545`
3. Check network connectivity: `ping 192.168.0.14`
4. Check if port 8545 is open: `nc -zv 192.168.0.14 8545`

## Node Setup Examples

### Reth (Recommended)
```bash
reth node \
  --http \
  --http.addr 0.0.0.0 \
  --http.port 8545 \
  --http.api eth,web3,net,trace
```

### Geth
```bash
geth \
  --http \
  --http.addr 0.0.0.0 \
  --http.port 8545 \
  --http.api eth,web3,net
```

### Erigon
```bash
erigon \
  --http \
  --http.addr 0.0.0.0 \
  --http.port 8545 \
  --http.api eth,web3,net,trace
```

### Besu
```bash
besu \
  --rpc-http-enabled \
  --rpc-http-host 0.0.0.0 \
  --rpc-http-port 8545 \
  --rpc-http-api ETH,WEB3,NET
```

## How to Test Your Setup

1. **Check node is running:**
   ```bash
   curl -X POST http://192.168.0.14:8545 \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq .
   ```

2. **Create a filter:**
   ```bash
   curl -X POST http://192.168.0.14:8545 \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_newPendingTransactionFilter","params":[],"id":1}' | jq .
   ```

3. **Check for pending transactions:**
   ```bash
   FILTER_ID=$(curl -s -X POST http://192.168.0.14:8545 \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","method":"eth_newPendingTransactionFilter","params":[],"id":1}' | jq -r .result)
   
   curl -X POST http://192.168.0.14:8545 \
     -H "Content-Type: application/json" \
     -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getFilterChanges\",\"params\":[\"$FILTER_ID\"],\"id\":1}" | jq .
   ```

If all these work, the monitor should work too!

## Recent Changes

**Version 0.1.1 - Filter-Based Approach**
- Changed from `eth_pendingTransactions` (not widely supported)
- Now uses `eth_newPendingTransactionFilter` + `eth_getFilterChanges` (more compatible)
- Better error handling for filter expiration
- Default RPC URL updated to `http://192.168.0.14:8545`

This approach is:
- ✅ More widely supported across different Ethereum clients
- ✅ More stable and robust
- ✅ Better for long-running monitoring
