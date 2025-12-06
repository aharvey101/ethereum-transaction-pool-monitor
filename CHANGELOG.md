# Changelog

## [0.1.1] - 2025-12-06

### Changed
- **RPC Method Migration**: Changed from `eth_pendingTransactions` to `eth_newPendingTransactionFilter` + `eth_getFilterChanges`
  - This approach is compatible with more Ethereum clients (Reth, Geth, Erigon, Besu)
  - `eth_pendingTransactions` is not widely supported and many nodes return "method not found"
  
- **Default RPC URL**: Updated from `http://localhost:8545` to `http://192.168.0.14:8545`
  - Adjust with `ETH_RPC_URL` environment variable as needed

### Improved
- Better error handling for filter expiration
- More robust transaction fetching with individual error recovery
- More informative error messages
- New documentation on RPC methods and compatibility

### Added
- `RPC_METHODS.md` - Comprehensive guide to RPC methods, compatibility, and troubleshooting
- Detailed RPC setup examples for Geth, Erigon, Reth, and Besu
- RPC diagnostic commands to test your setup

### Fixed
- "Method not found" error for nodes that don't support `eth_pendingTransactions`
- Connection issues with more node types

## [0.1.0] - 2025-12-06

### Initial Release
- Real-time Ethereum mempool monitoring via terminal UI
- Ratatui-based terminal interface with tables
- Async/await with tokio for non-blocking I/O
- Transaction filtering using `eth_newPendingTransactionFilter`
- Keyboard navigation (↑/↓, Page Up/Down, q/ESC)
- Connection status indicator
- Auto-refresh every 2 seconds
- Hex to ETH/Gwei conversion
- Cross-platform support (macOS, Linux, Windows)
- Comprehensive documentation

### Features
- ✓ Real-time transaction display
- ✓ Beautiful terminal UI
- ✓ Connection health monitoring
- ✓ Interactive navigation
- ✓ Error handling and recovery
- ✓ Memory-efficient (max 1000 transactions)
