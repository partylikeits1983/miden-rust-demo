//! Counter contract test module

use miden_client::{
    account::StorageMap,
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    rpc::{Endpoint, TonicRpcClient},
    transaction::{OutputNote, TransactionRequestBuilder},
    ClientError, Felt, Word,
};
use miden_objects::{account::NetworkId, FieldElement};
use rand::prelude::StdRng;
use std::sync::Arc;

mod helpers;

use helpers::{AccountCreationConfig, NoteCreationConfig, compile_rust_package, create_account_with_component, create_note_from_package};

fn assert_counter_storage(
    counter_account_storage: &miden_client::account::AccountStorage,
    expected: u64,
) {
    // according to `examples/counter-contract` for inner (slot, key) values
    let counter_contract_storage_key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);

    // The counter contract is in slot 1 when deployed, auth_component takes slot 0
    let word = counter_account_storage
        .get_map_item(1, counter_contract_storage_key)
        .expect("Failed to get counter value from storage slot 1");

    let val = word.last().unwrap();
    assert_eq!(
        val.as_int(),
        expected,
        "Counter value mismatch. Expected: {}, Got: {}",
        expected,
        val.as_int()
    );
}

/// Tests the counter contract deployment and note consumption workflow.
#[tokio::main]
async fn main() -> Result<(), ClientError> {
    println!("=== Miden Counter Contract Deployment and Note Consumption ===");
    println!("This script demonstrates the full workflow of:");
    println!("1. Compiling Rust packages to Miden");
    println!("2. Creating accounts with counter contract components");
    println!("3. Creating and consuming counter notes");
    println!("4. Verifying counter incrementation");
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
    let contract_package = compile_rust_package("../counter-contract", true);
    let note_package = compile_rust_package("../counter-contract-note", true);
    println!("✓ Compiled counter contract package");
    println!("✓ Compiled counter note package");

    // Create the counter account with initial storage
    println!("\n[STEP 2] Creating counter account with initial storage...");
    let key = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let value = Word::from([Felt::ZERO, Felt::ZERO, Felt::ZERO, Felt::ONE]);
    let config = AccountCreationConfig {
        storage_slots: vec![miden_client::account::StorageSlot::Map(
            StorageMap::with_entries([(key.into(), value)]).unwrap(),
        )],
        ..Default::default()
    };

    let counter_account = create_account_with_component(
        &mut client,
        Arc::new(keystore.clone()),
        contract_package,
        config,
    )
    .await
    .unwrap();
    println!("✓ Counter account created successfully!");
    println!(
        "  Account ID: {}",
        counter_account.id().to_bech32(NetworkId::Testnet)
    );

    // The counter contract storage value should be 1 after the account creation
    assert_counter_storage(
        client
            .get_account(counter_account.id())
            .await
            .unwrap()
            .unwrap()
            .account()
            .storage(),
        1,
    );
    println!("✓ Initial counter value verified: 1");

    // Create the counter note from sender to counter
    println!("\n[STEP 3] Creating counter note...");
    let counter_note = create_note_from_package(
        &mut client,
        note_package,
        counter_account.id(),
        NoteCreationConfig::default(),
    );
    println!("✓ Counter note created");
    println!("  Note hash: {:?}", counter_note.id().to_hex());

    // Submit transaction to create the note
    println!("\n[STEP 4] Submitting transaction to create the note...");
    let note_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![OutputNote::Full(counter_note.clone())])
        .build()
        .unwrap();

    let tx_result = client
        .new_transaction(counter_account.id(), note_request)
        .await
        .map_err(|e| {
            eprintln!("Transaction creation error: {e}");
            e
        })
        .unwrap();
    let executed_transaction = tx_result.executed_transaction();

    assert_eq!(executed_transaction.output_notes().num_notes(), 1);

    let executed_tx_output_note = executed_transaction.output_notes().get_note(0);
    assert_eq!(executed_tx_output_note.id(), counter_note.id());
    let create_note_tx_id = executed_transaction.id();
    client.submit_transaction(tx_result).await.unwrap();
    println!("✓ Counter note creation transaction submitted");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        create_note_tx_id
    );

    // Consume the note to increment the counter
    println!("\n[STEP 5] Consuming the note to increment the counter...");
    let consume_request = TransactionRequestBuilder::new()
        .unauthenticated_input_notes([(counter_note, None)])
        .build()
        .unwrap();

    let tx_result = client
        .new_transaction(counter_account.id(), consume_request)
        .await
        .map_err(|e| {
            eprintln!("Note consumption transaction error: {e}");
            e
        })
        .unwrap();
    let consume_tx_id = tx_result.executed_transaction().id();
    println!("✓ Counter note consumption transaction created");
    println!(
        "  View on MidenScan: https://testnet.midenscan.com/tx/{:?}",
        consume_tx_id
    );

    client.submit_transaction(tx_result).await.unwrap();
    println!("✓ Counter note consumption transaction submitted");

    // Sync state to get latest updates
    println!("\n[STEP 6] Syncing state and verifying counter incrementation...");
    let sync_result = client.sync_state().await.unwrap();
    println!("✓ Synced to block: {}", sync_result.block_num);

    // The counter contract storage value should be 2 (incremented) after the note is consumed
    assert_counter_storage(
        client
            .get_account(counter_account.id())
            .await
            .unwrap()
            .unwrap()
            .account()
            .storage(),
        2,
    );
    println!("✓ Counter value after incrementation verified: 2");

    // Final summary
    println!("\n=== SUCCESS: Counter Contract Workflow Completed! ===");
    println!();
    println!("✓ Compiled Rust packages to Miden");
    println!("✓ Created counter account with initial storage (value: 1)");
    println!("✓ Created and submitted counter note");
    println!("✓ Consumed counter note to increment counter");
    println!("✓ Verified counter incrementation (value: 1 → 2)");
    println!();
    println!("The complete counter contract deployment and note consumption");
    println!("workflow has been successfully demonstrated using the new Rust compiler!");

    Ok(())
}
