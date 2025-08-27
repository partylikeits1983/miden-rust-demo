//! Basic wallet test module

use miden_client::{
    account::{
        component::{BasicFungibleFaucet, RpoFalcon512},
        Account, AccountBuilder, AccountId, AccountStorageMode, AccountType,
    },
    asset::{FungibleAsset, TokenSymbol},
    auth::AuthSecretKey,
    builder::ClientBuilder,
    crypto::{FeltRng, RpoRandomCoin, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteTag,
        NoteType,
    },
    rpc::{Endpoint, TonicRpcClient},
    transaction::{OutputNote, TransactionRequestBuilder, TransactionScript},
    Client, ClientError, Felt,
};
use miden_core::crypto::hash::Rpo256;
use miden_mast_package::Package;
use miden_objects::{
    account::{Account as ObjectsAccount, NetworkId},
    asset::Asset,
    FieldElement,
};
use rand::{prelude::StdRng, RngCore};
use std::{collections::BTreeMap, sync::Arc};

mod helpers;

use helpers::{
    compile_rust_package, create_account_with_component, create_note_from_package,
    AccountCreationConfig, NoteCreationConfig,
};

/// Configuration for asset transfers
struct AssetTransferConfig {
    note_type: NoteType,
    tag: NoteTag,
    execution_hint: NoteExecutionHint,
    aux: Felt,
}

impl Default for AssetTransferConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::for_local_use_case(0, 0).unwrap(),
            execution_hint: NoteExecutionHint::always(),
            aux: Felt::ZERO,
        }
    }
}

/// Create a fungible faucet account
async fn create_fungible_faucet_account(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    token_symbol: TokenSymbol,
    decimals: u8,
    max_supply: Felt,
) -> Result<Account, ClientError> {
    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());
    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();
    let builder = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()))
        .with_component(BasicFungibleFaucet::new(token_symbol, decimals, max_supply).unwrap());

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore
        .add_key(&AuthSecretKey::RpoFalcon512(key_pair))
        .unwrap();

    Ok(account)
}

/// Helper function to assert that an account contains a specific fungible asset
async fn assert_account_has_fungible_asset(
    client: &mut Client,
    account_id: AccountId,
    expected_faucet_id: AccountId,
    expected_amount: u64,
) {
    let account_record = client
        .get_account(account_id)
        .await
        .expect("Failed to get account")
        .expect("Account not found");

    let account_state: ObjectsAccount = account_record.into();

    // Look for the specific fungible asset in the vault
    let found_asset = account_state.vault().assets().find_map(|asset| {
        if let Asset::Fungible(fungible_asset) = asset {
            if fungible_asset.faucet_id() == expected_faucet_id {
                Some(fungible_asset)
            } else {
                None
            }
        } else {
            None
        }
    });

    match found_asset {
        Some(fungible_asset) => {
            assert_eq!(
                fungible_asset.amount(),
                expected_amount,
                "Found asset from faucet {expected_faucet_id} but amount {} doesn't match \
                 expected {expected_amount}",
                fungible_asset.amount()
            );
        }
        None => {
            panic!("Account does not contain a fungible asset from faucet {expected_faucet_id}");
        }
    }
}

/// Helper function to send assets from one account to another using a transaction script
async fn send_asset_to_account(
    client: &mut Client,
    sender_account_id: AccountId,
    recipient_account_id: AccountId,
    asset: FungibleAsset,
    note_package: Arc<Package>,
    tx_script_package: Arc<Package>,
    config: Option<AssetTransferConfig>,
) -> Result<(miden_client::transaction::TransactionId, Note), ClientError> {
    let config = config.unwrap_or_default();

    // Create the p2id note for the recipient
    let p2id_note = create_note_from_package(
        client,
        note_package,
        sender_account_id,
        NoteCreationConfig {
            assets: NoteAssets::new(vec![asset.into()]).unwrap(),
            inputs: vec![
                recipient_account_id.prefix().as_felt(),
                recipient_account_id.suffix(),
            ],
            note_type: config.note_type,
            tag: config.tag,
            execution_hint: config.execution_hint,
            aux: config.aux,
        },
    );

    let tx_script_program = tx_script_package.unwrap_program();
    let tx_script = TransactionScript::from_parts(
        tx_script_program.mast_forest().clone(),
        tx_script_program.entrypoint(),
    );

    // Prepare note recipient
    let program_hash = tx_script_program.hash();
    let serial_num = RpoRandomCoin::new(program_hash.into()).draw_word();
    let inputs = NoteInputs::new(vec![
        recipient_account_id.prefix().as_felt(),
        recipient_account_id.suffix(),
    ])
    .unwrap();
    let note_recipient = NoteRecipient::new(serial_num, p2id_note.script().clone(), inputs);

    // Prepare commitment data
    let mut input: Vec<Felt> = vec![
        config.tag.into(),
        config.aux,
        config.note_type.into(),
        config.execution_hint.into(),
    ];
    let recipient_digest: [Felt; 4] = note_recipient.digest().into();
    input.extend(recipient_digest);

    let asset_arr: [Felt; 4] = asset.into();
    input.extend(asset_arr);

    let mut commitment: [Felt; 4] = Rpo256::hash_elements(&input).into();

    assert_eq!(input.len() % 4, 0, "input needs to be word-aligned");

    // Prepare advice map
    let mut advice_map = BTreeMap::new();
    advice_map.insert(commitment.into(), input.clone());

    let recipients = vec![note_recipient.clone()];

    // NOTE: passed on the stack reversed
    commitment.reverse();

    let tx_request = TransactionRequestBuilder::new()
        .custom_script(tx_script)
        .script_arg(commitment)
        .expected_output_recipients(recipients)
        .extend_advice_map(advice_map)
        .build()
        .unwrap();

    let tx = client
        .new_transaction(sender_account_id, tx_request)
        .await?;
    let tx_id = tx.executed_transaction().id();

    client.submit_transaction(tx).await?;

    // Create the Note that the recipient will consume
    let assets = NoteAssets::new(vec![asset.into()]).unwrap();
    let metadata = NoteMetadata::new(
        sender_account_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();
    let recipient_note = Note::new(assets, metadata, note_recipient);

    Ok((tx_id, recipient_note))
}

/// Tests the basic-wallet contract deployment and p2id note consumption workflow.
#[tokio::main]
async fn main() -> Result<(), ClientError> {
    println!("=== Miden Basic Wallet P2ID Example ===");
    println!("This script demonstrates the full workflow of:");
    println!("1. Compiling basic wallet, p2id note, and transaction script packages");
    println!("2. Creating fungible faucet and wallet accounts");
    println!("3. Minting tokens to Alice's wallet");
    println!("4. Transferring tokens from Alice to Bob using p2id notes");
    println!("5. Verifying asset transfers");
    println!();

    // Initialize client & keystore
    let endpoint = Endpoint::testnet();
    let timeout_ms = 10_000;
    let rpc_api = Arc::new(TonicRpcClient::new(&endpoint, timeout_ms));

    let mut client = ClientBuilder::new()
        .rpc(rpc_api)
        .filesystem_keystore("./keystore")
        .in_debug_mode(true)
        .build()
        .await?;

    let sync_summary = client.sync_state().await.unwrap();
    println!("✓ Connected to Miden testnet");
    println!("  Latest block: {}", sync_summary.block_num);

    let keystore: FilesystemKeyStore<StdRng> =
        FilesystemKeyStore::new("./keystore".into()).unwrap();

    // Compile the contracts first (before creating any runtime)
    println!("\n[STEP 1] Compiling Rust packages...");
    let wallet_package = compile_rust_package("../basic-wallet", true);
    let note_package = compile_rust_package("../p2id-note", true);
    let tx_script_package = compile_rust_package("../basic-wallet-tx-script", true);
    println!("✓ Compiled basic wallet package");
    println!("✓ Compiled p2id note package");
    println!("✓ Compiled basic wallet transaction script package");

    // Create a fungible faucet account
    println!("\n[STEP 2] Creating fungible faucet account...");
    let token_symbol = TokenSymbol::new("TEST").unwrap();
    let decimals = 8u8;
    let max_supply = Felt::new(1_000_000_000); // 1 billion tokens

    let faucet_account = create_fungible_faucet_account(
        &mut client,
        Arc::new(keystore.clone()),
        token_symbol,
        decimals,
        max_supply,
    )
    .await
    .unwrap();

    println!("✓ Faucet account created successfully!");
    println!(
        "  Faucet ID: {}",
        faucet_account.id().to_bech32(NetworkId::Testnet)
    );

    // Create Alice's account with basic-wallet component
    println!("\n[STEP 3] Creating Alice's wallet account...");
    let alice_config = AccountCreationConfig {
        with_basic_wallet: false,
        ..Default::default()
    };
    let alice_account = create_account_with_component(
        &mut client,
        Arc::new(keystore.clone()),
        wallet_package.clone(),
        alice_config,
    )
    .await
    .unwrap();
    println!("✓ Alice's account created successfully!");
    println!(
        "  Alice ID: {}",
        alice_account.id().to_bech32(NetworkId::Testnet)
    );

    println!("\n[STEP 4] Minting tokens from faucet to Alice...");

    let mint_amount = 100_000u64; // 100,000 tokens
    let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

    // Create the p2id note from faucet to Alice
    let p2id_note_mint = create_note_from_package(
        &mut client,
        note_package.clone(),
        faucet_account.id(),
        NoteCreationConfig {
            assets: NoteAssets::new(vec![fungible_asset.into()]).unwrap(),
            inputs: vec![
                alice_account.id().prefix().as_felt(),
                alice_account.id().suffix(),
            ],
            ..Default::default()
        },
    );
    println!("✓ P2ID mint note created");
    println!("  Note hash: {:?}", p2id_note_mint.id().to_hex());

    let mint_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
        .build()
        .unwrap();

    let mint_tx_result = client
        .new_transaction(faucet_account.id(), mint_request)
        .await
        .unwrap();
    let mint_tx_id = mint_tx_result.executed_transaction().id();
    println!("✓ Mint transaction created");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        mint_tx_id
    );

    client.submit_transaction(mint_tx_result).await.unwrap();
    println!("✓ Mint transaction submitted");

    println!("\n[STEP 5] Alice consuming mint note...");

    let consume_request = TransactionRequestBuilder::new()
        .unauthenticated_input_notes([(p2id_note_mint, None)])
        .build()
        .unwrap();

    let consume_tx = client
        .new_transaction(alice_account.id(), consume_request)
        .await
        .map_err(|e| {
            eprintln!("Alice consume transaction error: {e}");
            e
        })
        .unwrap();

    let alice_consume_tx_id = consume_tx.executed_transaction().id();
    client.submit_transaction(consume_tx).await.unwrap();
    println!("✓ Alice consumed mint note");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        alice_consume_tx_id
    );

    // Sync state to get latest updates
    println!("\n[STEP 6] Syncing state and verifying Alice's balance...");
    let sync_result = client.sync_state().await.unwrap();
    println!("✓ Synced to block: {}", sync_result.block_num);

    assert_account_has_fungible_asset(
        &mut client,
        alice_account.id(),
        faucet_account.id(),
        mint_amount,
    )
    .await;
    println!(
        "✓ Alice's account has the minted asset: {} tokens",
        mint_amount
    );

    println!("\n[STEP 7] Creating Bob's wallet account...");

    let bob_config = AccountCreationConfig {
        with_basic_wallet: false,
        ..Default::default()
    };
    let bob_account = create_account_with_component(
        &mut client,
        Arc::new(keystore.clone()),
        wallet_package,
        bob_config,
    )
    .await
    .unwrap();
    println!("✓ Bob's account created successfully!");
    println!(
        "  Bob ID: {}",
        bob_account.id().to_bech32(NetworkId::Testnet)
    );

    println!("\n[STEP 8] Alice creating p2id note for Bob...");

    let transfer_amount = 10_000u64; // 10,000 tokens
    let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

    // Use the send_asset_to_account helper function like in the original test
    let (alice_tx_id, bob_note) = send_asset_to_account(
        &mut client,
        alice_account.id(),
        bob_account.id(),
        transfer_asset,
        note_package.clone(),
        tx_script_package,
        None, // Use default configuration
    )
    .await
    .unwrap();

    println!("✓ Alice created p2id note for Bob");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        alice_tx_id
    );

    println!("\n[STEP 9] Bob consuming p2id note...");

    let bob_consume_request = TransactionRequestBuilder::new()
        .unauthenticated_input_notes([(bob_note, None)])
        .build()
        .unwrap();

    let bob_consume_tx = client
        .new_transaction(bob_account.id(), bob_consume_request)
        .await
        .unwrap();
    let bob_consume_tx_id = bob_consume_tx.executed_transaction().id();
    println!("✓ Bob consume transaction created");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        bob_consume_tx_id
    );

    client.submit_transaction(bob_consume_tx).await.unwrap();
    println!("✓ Bob consumed p2id note");

    println!("\n[STEP 10] Final verification...");
    let sync_result = client.sync_state().await.unwrap();
    println!("✓ Synced to block: {}", sync_result.block_num);

    assert_account_has_fungible_asset(
        &mut client,
        bob_account.id(),
        faucet_account.id(),
        transfer_amount,
    )
    .await;
    println!(
        "✓ Bob's account has the transferred asset: {} tokens",
        transfer_amount
    );

    assert_account_has_fungible_asset(
        &mut client,
        alice_account.id(),
        faucet_account.id(),
        mint_amount - transfer_amount,
    )
    .await;
    println!(
        "✓ Alice's account reflects the new balance: {} tokens",
        mint_amount - transfer_amount
    );

    // Final summary
    println!("\n=== SUCCESS: Basic Wallet P2ID Workflow Completed! ===");
    println!();
    println!("✓ Compiled basic wallet, p2id note, and transaction script packages");
    println!("✓ Created fungible faucet account");
    println!("✓ Created Alice's and Bob's wallet accounts");
    println!("✓ Minted {} tokens to Alice", mint_amount);
    println!("✓ Transferred {} tokens from Alice to Bob", transfer_amount);
    println!("✓ Verified final balances:");
    println!("  - Alice: {} tokens", mint_amount - transfer_amount);
    println!("  - Bob: {} tokens", transfer_amount);
    println!();
    println!("The complete basic wallet P2ID workflow has been successfully");
    println!("demonstrated using the Rust compiler and Miden client!");

    Ok(())
}
