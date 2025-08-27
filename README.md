# Miden Rust Compiler Examples

This repository demonstrates how to use the new Miden Rust compiler to compile Rust smart contracts and deploy them on the Miden network.

## Structure

- `counter-contract/` - A simple counter smart contract written in Rust
- `counter-contract-note/` - A note that interacts with the counter contract  
- `basic-wallet/` - A basic wallet smart contract for asset management
- `p2id-note/` - A pay-to-ID note for transferring assets between accounts
- `basic-wallet-tx-script/` - Transaction script for wallet operations
- `scripts/` - Deployment and interaction scripts using real compiler integration

## Prerequisites

- **Rust nightly-2025-07-20** (specified in `rust-toolchain.toml`)
- Miden compiler tools (`cargo install cargo-miden`)

The project uses a specific Rust nightly version for compatibility with the Miden compiler. The `rust-toolchain.toml` file will automatically ensure you're using the correct version.

## Quick Start

1. **Install dependencies:**
   ```bash
   cargo install cargo-miden
   ```

2. **Choose an example to run:**

### Counter Contract Example
Demonstrates a simple counter that increments when a note is consumed:

```bash
cd scripts
cargo run --release --bin deploy_counter_with_note
```

This will:
- Compile the counter contract and note using the Miden compiler
- Connect to Miden testnet
- Create a counter account with initial storage (value: 1)
- Create and submit a counter note
- Consume the note to increment the counter (value: 1 → 2)
- Verify the counter incrementation

### Basic Wallet P2ID Example
Demonstrates a complete wallet workflow with asset transfers:

```bash
cd scripts
cargo run --release --bin wallet_p2id_example
```

This will:
- Compile basic wallet, p2id note, and transaction script packages
- Connect to Miden testnet
- Create a fungible faucet account
- Create Alice's and Bob's wallet accounts
- Mint 100,000 tokens to Alice
- Transfer 10,000 tokens from Alice to Bob using p2id notes
- Verify final balances (Alice: 90,000, Bob: 10,000)

## What These Examples Demonstrate

- **Real Rust Compilation**: Uses the actual Miden Rust compiler to compile contracts
- **Testnet Integration**: Connects to and interacts with the live Miden testnet
- **Complete Workflows**: Shows end-to-end processes from compilation to execution
- **Asset Management**: Demonstrates token creation, minting, and transfers
- **Account Management**: Shows how to create and manage different account types
- **Transaction Scripts**: Illustrates complex transaction logic with custom scripts

## Viewing Transactions

All transactions are submitted to the Miden testnet and can be viewed on [MidenScan](https://testnet.midenscan.com/). The scripts will output transaction IDs that you can use to view the transactions on the explorer.

## Troubleshooting

- Ensure you're using the correct Rust version (nightly-2025-07-20)
- Make sure `cargo-miden` is installed and available in your PATH
- Check that you have internet connectivity for testnet access