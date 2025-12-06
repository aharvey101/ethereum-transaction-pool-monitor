# Ethereum Mempool Monitor

A real-time Ethereum mempool monitor built with Rust using the ratatui terminal UI library and alloy.

## Features

- **Real-time Monitoring**: Displays pending transactions from your local Ethereum node
- **Terminal UI**: Beautiful terminal interface built with ratatui
- **Transaction Details**: Shows from address, to address, value, gas price, and nonce
- **Multi-DEX Pool Detection**: Automatically discovers pools from major DEXs
- **Navigation**: Scroll through pending transactions with keyboard controls
- **Connection Status**: Displays connection health status to the Ethereum node
- **Auto-refresh**: Updates transaction list every 2 seconds

### DEX Pool Support

The monitor automatically detects and tracks liquidity pools from major decentralized exchanges:

- **UniswapV2**: Official Uniswap V2 pools (9,199+ pools)
- **UniswapV3**: Official Uniswap V3 pools (2,412+ pools)  
- **SushiSwap**: SushiSwap AMM pools (V2 compatible)
- **PancakeSwap**: PancakeSwap V2 pools on Ethereum
- **Real-time Detection**: Monitors new pool creations across all supported DEXs
- **30x Performance**: Parallel scanning completes full blockchain scan in ~30 seconds

## Prerequisites

- Rust 1.56 or later (install from https://rustup.rs/)
- A running Ethereum node with JSON-RPC enabled (e.g., Geth, Erigon, Reth, Besu)
- The node must support `eth_newPendingTransactionFilter` (most nodes do)

**Note:** The monitor uses `eth_newPendingTransactionFilter` + `eth_getFilterChanges` for maximum compatibility. See [RPC_METHODS.md](RPC_METHODS.md) for details on supported methods and RPC endpoint setup.

## Installation

1. Clone or download this project
2. Navigate to the project directory:
   ```bash
   cd ethereum-transaction-pool-monitor
   ```

3. Build the project:
   ```bash
   cargo build --release
   ```

## Running

### Using default local node (localhost:8545)

```bash
cargo run --release
```

### Using a custom Ethereum node RPC URL

```bash
ETH_RPC_URL=http://your-rpc-url:8545 cargo run --release
```

### Examples

**Local Geth node:**
```bash
ETH_RPC_URL=http://localhost:8545 cargo run --release
```

**Local Erigon node:**
```bash
ETH_RPC_URL=http://localhost:8545 cargo run --release
```

**Infura endpoint:**
```bash
ETH_RPC_URL=https://mainnet.infura.io/v3/YOUR_PROJECT_ID cargo run --release
```

**Alchemy endpoint:**
```bash
ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY cargo run --release
```

## Configuration

### Environment Variables

The application supports several environment variables for configuration:

| Variable | Default | Description |
|----------|---------|-------------|
| `ETH_RPC_URL` | `http://localhost:8545` | Ethereum RPC endpoint |
| `FORCE_POOL_REFRESH` | Not set | Force complete pool scan on startup |
| `USE_SEQUENTIAL_SCAN` | Not set | Use old sequential scanning (slower but more compatible) |
| `RUST_LOG` | `info` | Logging level (`debug`, `info`, `warn`, `error`) |

### Pool Scanning Methods

**Default - Multi-DEX Parallel Scanning (Recommended):**
- ⚡ **30x faster** than sequential scanning
- 🔄 Scans all supported DEXs simultaneously in 50K block chunks
- 🚀 Uses 20 concurrent tasks per batch for maximum speed
- 🎯 **4 DEX protocols**: UniswapV2, UniswapV3, SushiSwap, PancakeSwap
- ✅ Same accuracy as sequential method but covers more pools

```bash
# Default behavior - no environment variables needed
cargo run --release
```

**Sequential Scanning (Fallback):**
- 🐌 Original method, slower but maximum compatibility
- 📦 Scans Uniswap pools only in 100K block windows
- 🔒 Use if parallel scanning has issues with your RPC endpoint

```bash
# Enable sequential scanning (Uniswap only)
USE_SEQUENTIAL_SCAN=1 cargo run --release
```

**Force Pool Refresh:**
```bash
# Force complete blockchain scan (ignores existing database)
FORCE_POOL_REFRESH=1 cargo run --release
```

## Usage

Once running, you'll see:

- **Header**: Application name, connection status, and last update time
- **Main Area**: Table of pending transactions with columns:
  - From: Sender address (truncated)
  - To: Recipient address or "Contract Creation"
  - Value: Transaction value in ETH
  - Gas Price: Current gas price in Gwei
  - Nonce: Transaction nonce

## Keyboard Controls

| Key | Action |
|-----|--------|
| ↑ / ↓ | Navigate through transactions |
| Page Up / Page Down | Scroll faster |
| q / ESC | Quit the application |

## How It Works

1. **Connection**: Connects to your Ethereum node via JSON-RPC
2. **Fetching**: Calls `eth_pendingTransactions` to get mempool transactions
3. **Parsing**: Converts hex-formatted transaction data to readable formats
4. **Display**: Shows transactions in a sortable table
5. **Refresh**: Updates every 2 seconds

## Architecture

```
src/
├── main.rs          # Main application loop and event handling
├── eth_client.rs    # Ethereum node connection and transaction fetching
├── app.rs           # Application state management
└── ui.rs            # Terminal UI rendering with ratatui
```

### Key Components

- **EthereumClient**: Handles JSON-RPC communication with the Ethereum node
- **AppState**: Manages UI state, transactions, and navigation
- **UI Module**: Renders the terminal interface using ratatui

## Limitations

- The default Ethereum RPC doesn't typically expose `eth_pendingTransactions` on public endpoints (Infura, Alchemy, etc.)
- You need a local node or private RPC that supports this method
- Only displays up to 1000 most recent transactions

## Setting Up a Local Node

### Geth

```bash
geth --http --http.port 8545 --http.api eth,web3,net
```

### Erigon

```bash
erigon --http --http.port 8545 --http.api eth,web3,net
```

### Besu

```bash
besu --rpc-http-enabled --rpc-http-port 8545 --rpc-http-api ETH,WEB3,NET
```

## Troubleshooting

**"Failed to connect to Ethereum node"**
- Check that your Ethereum node is running on the specified RPC URL
- Verify the JSON-RPC endpoint is accessible

**"No pending transactions found"**
- This is normal during low network activity
- Try monitoring for longer or use a busier network
- Some RPC endpoints may not support `eth_pendingTransactions`

**Terminal rendering issues**
- Try resizing your terminal window
- Ensure your terminal supports 256 colors
- Check that crossterm backend is properly supported

## Development

To contribute or modify:

```bash
# Run in development mode
cargo run

# Check for issues
cargo clippy

# Format code
cargo fmt
```

## Dependencies

- **ratatui**: Terminal UI framework
- **crossterm**: Terminal abstraction
- **ethers-rs**: Ethereum utilities and types
- **tokio**: Async runtime
- **serde/serde_json**: JSON serialization

## License

MIT

## Future Enhancements

- [ ] Transaction details popup with full information
- [ ] Filtering by address or value
- [ ] Sorting by gas price, value, or timestamp
- [ ] Export transaction data to CSV
- [ ] Transaction history graph
- [ ] WebSocket support for real-time updates
- [ ] Multi-chain support
