# Miden Rust Compiler Example

This repository demonstrates how to use the new Miden Rust compiler to compile Rust smart contracts and deploy them on the Miden network.

## Structure

- `counter-contract/` - A simple counter smart contract written in Rust
- `counter-contract-note/` - A note that interacts with the counter contract  
- `scripts/` - Deployment and interaction scripts using real compiler integration

## Prerequisites

- Rust toolchain with nightly support
- Miden compiler tools (`cargo install cargo-miden`)

## How to Run

1. **Install dependencies:**
   ```bash
   cargo install cargo-miden
   ```

2. **Run the complete workflow:**
   ```bash
   cd scripts
   cargo run --release
   ```

This will:
- Compile both Rust contracts using the real Miden compiler
- Connect to Miden testnet
- Create a counter account with initial storage
- Create and submit a counter note
- Demonstrate the full end-to-end workflow

The script uses **real compiler integration** (no mocking) and executes transactions on the actual Miden testnet.