//! Helper functions for counter contract deployment

use std::sync::Arc;

use miden_client::{
    account::{
        component::{BasicFungibleFaucet, BasicWallet, RpoFalcon512},
        Account, AccountId, AccountStorageMode, AccountType, StorageSlot,
    },
    asset::TokenSymbol,
    auth::AuthSecretKey,
    crypto::{FeltRng, SecretKey},
    keystore::FilesystemKeyStore,
    note::{
        Note, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript, NoteTag,
        NoteType,
    },
    Client, ClientError, Felt,
};
use miden_lib::utils::Deserializable;
use miden_mast_package::Package;
use miden_objects::{
    account::{
        AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
    },
    assembly::Assembler,
    asset::Asset,
    FieldElement,
};
use rand::{rngs::StdRng, RngCore};
use std::collections::BTreeSet;

/// Configuration for creating an account with a custom component
pub struct AccountCreationConfig {
    pub account_type: AccountType,
    pub storage_mode: AccountStorageMode,
    pub storage_slots: Vec<StorageSlot>,
    pub supported_types: Option<Vec<AccountType>>,
    pub with_basic_wallet: bool,
}

impl Default for AccountCreationConfig {
    fn default() -> Self {
        Self {
            account_type: AccountType::RegularAccountUpdatableCode,
            storage_mode: AccountStorageMode::Public,
            storage_slots: vec![],
            supported_types: None,
            with_basic_wallet: true,
        }
    }
}

/// Helper to create an account with a custom component from a package
pub async fn create_account_with_component(
    client: &mut Client,
    keystore: Arc<FilesystemKeyStore<StdRng>>,
    package: Arc<Package>,
    config: AccountCreationConfig,
) -> Result<Account, ClientError> {
    let account_component = match package.account_component_metadata_bytes.as_deref() {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();

            let template =
                AccountComponentTemplate::new(metadata, package.unwrap_library().as_ref().clone());

            let component =
                AccountComponent::new(template.library().clone(), config.storage_slots).unwrap();

            // Use supported types from config if provided, otherwise default to RegularAccountUpdatableCode
            let supported_types = if let Some(types) = config.supported_types {
                BTreeSet::from_iter(types)
            } else {
                BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode])
            };

            component.with_supported_types(supported_types)
        }
    };

    let mut init_seed = [0_u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = SecretKey::with_rng(client.rng());

    // Sync client state to get latest block info
    let _sync_summary = client.sync_state().await.unwrap();

    let mut builder = AccountBuilder::new(init_seed)
        .account_type(config.account_type)
        .storage_mode(config.storage_mode)
        .with_auth_component(RpoFalcon512::new(key_pair.public_key()));

    if config.with_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder = builder.with_component(account_component);

    let (account, seed) = builder.build().unwrap();
    client.add_account(&account, Some(seed), false).await?;
    keystore
        .add_key(&AuthSecretKey::RpoFalcon512(key_pair))
        .unwrap();

    Ok(account)
}

/// Configuration for creating a note
pub struct NoteCreationConfig {
    pub note_type: NoteType,
    pub tag: NoteTag,
    pub assets: miden_client::note::NoteAssets,
    pub inputs: Vec<Felt>,
    pub execution_hint: NoteExecutionHint,
    pub aux: Felt,
}

impl Default for NoteCreationConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::for_local_use_case(0, 0).unwrap(),
            assets: Default::default(),
            inputs: Default::default(),
            execution_hint: NoteExecutionHint::always(),
            aux: Felt::ZERO,
        }
    }
}

/// Helper to create a note from a compiled package
/// For now, this creates a simple note since we have version compatibility issues
pub fn create_note_from_package(
    client: &mut Client,
    _package: Arc<Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
) -> Note {
    // Create a simple note script for demonstration using the correct assembler
    let assembler = Assembler::default();
    let note_script = NoteScript::compile("begin push.1 end", assembler).unwrap();

    let serial_num = client.rng().draw_word();
    let note_inputs = NoteInputs::new(config.inputs).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

    let metadata = NoteMetadata::new(
        sender_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();

    Note::new(config.assets, metadata, recipient)
}

/// Helper to compile a Rust package to Miden using the real compiler
pub fn compile_rust_package(package_path: &str, release: bool) -> Arc<Package> {
    use midenc_frontend_wasm::WasmTranslationConfig;

    println!("  Compiling Rust package at: {}", package_path);

    // Run the compilation in a blocking thread to avoid runtime conflicts
    let package_path = package_path.to_string();
    let handle = std::thread::spawn(move || {
        // Use the exact same approach as CompilerTestBuilder::rust_source_cargo_miden
        let config = WasmTranslationConfig::default();
        let mut builder = CompilerTestBuilder::rust_source_cargo_miden(&package_path, config, []);

        if release {
            builder.with_release(true);
        }

        let mut test = builder.build();
        test.compiled_package()
    });

    let package = handle.join().expect("Compilation thread panicked");
    println!("  ✓ Successfully compiled package");
    package
}

/// CompilerTestBuilder implementation copied from integration tests
pub struct CompilerTestBuilder {
    config: midenc_frontend_wasm::WasmTranslationConfig,
    source: CompilerTestInputType,
    entrypoint: Option<midenc_hir::FunctionIdent>,
    link_masm_modules: Vec<(miden_assembly::LibraryPath, String)>,
    midenc_flags: Vec<String>,
    rustflags: Vec<std::borrow::Cow<'static, str>>,
    workspace_dir: String,
}

pub enum CompilerTestInputType {
    CargoMiden(CargoTest),
}

pub struct CargoTest {
    project_dir: std::path::PathBuf,
    name: std::borrow::Cow<'static, str>,
    release: bool,
}

impl CargoTest {
    pub fn new(
        name: impl Into<std::borrow::Cow<'static, str>>,
        project_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            project_dir,
            name: name.into(),
            release: true,
        }
    }
}

impl CompilerTestBuilder {
    pub fn new(source: CompilerTestInputType) -> Self {
        let workspace_dir = get_workspace_dir();
        let _name = match &source {
            CompilerTestInputType::CargoMiden(config) => config.name.as_ref(),
        };
        let rustflags = vec![
            "-C".into(),
            "target-feature=+bulk-memory".into(),
            "--remap-path-prefix".into(),
            format!("{workspace_dir}=../../").into(),
        ];
        let midenc_flags = vec!["--verbose".into()];

        Self {
            config: Default::default(),
            source,
            entrypoint: None,
            link_masm_modules: vec![],
            midenc_flags,
            rustflags,
            workspace_dir,
        }
    }

    pub fn rust_source_cargo_miden(
        cargo_project_folder: impl AsRef<std::path::Path>,
        config: midenc_frontend_wasm::WasmTranslationConfig,
        midenc_flags: impl IntoIterator<Item = String>,
    ) -> Self {
        let name = cargo_project_folder
            .as_ref()
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or("".to_string());
        let mut builder = CompilerTestBuilder::new(CompilerTestInputType::CargoMiden(
            CargoTest::new(name, cargo_project_folder.as_ref().to_path_buf()),
        ));
        builder.config = config;
        builder.midenc_flags.extend(midenc_flags);
        builder
    }

    pub fn with_release(&mut self, release: bool) -> &mut Self {
        match &mut self.source {
            CompilerTestInputType::CargoMiden(config) => config.release = release,
        }
        self
    }

    pub fn build(mut self) -> CompilerTest {
        use midenc_session::{InputFile, InputType};
        use std::ffi::OsStr;
        use std::process::{Command, Stdio};

        // Set up the command used to compile the test inputs (Rust -> Wasm)
        let mut command = Command::new("cargo");
        command.arg("miden").arg("build");

        // Extract the directory in which source code exists
        let project_dir = match &self.source {
            CompilerTestInputType::CargoMiden(config) => &config.project_dir,
        };

        // Cargo-based configuration
        let CompilerTestInputType::CargoMiden(config) = &self.source;
        {
            let manifest_path = project_dir.join("Cargo.toml");
            command.arg("--manifest-path").arg(manifest_path);
            if config.release {
                command.arg("--release");
            }
        }

        // Set RUSTFLAGS
        if !self.rustflags.is_empty() {
            let mut flags = String::with_capacity(
                self.rustflags.iter().map(|flag| flag.len()).sum::<usize>() + self.rustflags.len(),
            );
            for (i, flag) in self.rustflags.iter().enumerate() {
                if i > 0 {
                    flags.push(' ');
                }
                flags.push_str(flag.as_ref());
            }
            command.env("RUSTFLAGS", flags);
        }

        command.stdout(Stdio::piped());

        // Build using cargo-miden
        let mut args = vec![command.get_program().to_str().unwrap().to_string()];
        let cmd_args: Vec<String> = command
            .get_args()
            .collect::<Vec<&OsStr>>()
            .iter()
            .map(|s| s.to_str().unwrap().to_string())
            .collect();
        args.extend(cmd_args);

        let build_output = cargo_miden::run(args.into_iter(), cargo_miden::OutputType::Wasm)
            .unwrap()
            .expect("'cargo miden build' should return Some(CommandOutput)")
            .unwrap_build_output();

        let (wasm_artifact_path, mut extra_midenc_flags) = match build_output {
            cargo_miden::BuildOutput::Wasm {
                artifact_path,
                midenc_flags,
            } => (artifact_path, midenc_flags),
            other => panic!("Expected Wasm output, got {:?}", other),
        };

        self.midenc_flags.append(&mut extra_midenc_flags);
        let artifact_name = wasm_artifact_path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let input_file = InputFile::from_path(wasm_artifact_path).unwrap();
        let mut inputs = vec![input_file];
        inputs.extend(self.link_masm_modules.into_iter().map(|(path, content)| {
            let path = path.to_string();
            InputFile::new(
                midenc_session::FileType::Masm,
                InputType::Stdin {
                    name: path.into(),
                    input: content.into_bytes(),
                },
            )
        }));

        let context = default_context(inputs, &self.midenc_flags);
        let session = context.session_rc();
        CompilerTest {
            config: self.config,
            session,
            context,
            artifact_name: artifact_name.into(),
            entrypoint: self.entrypoint,
            hir: None,
            masm_src: None,
            ir_masm_program: None,
            package: None,
        }
    }
}

pub struct CompilerTest {
    pub config: midenc_frontend_wasm::WasmTranslationConfig,
    pub session: std::rc::Rc<midenc_session::Session>,
    pub context: std::rc::Rc<midenc_hir::Context>,
    artifact_name: std::borrow::Cow<'static, str>,
    entrypoint: Option<midenc_hir::FunctionIdent>,
    hir: Option<midenc_compile::LinkOutput>,
    masm_src: Option<String>,
    ir_masm_program: Option<Result<Arc<midenc_codegen_masm::MasmComponent>, String>>,
    package: Option<Result<Arc<miden_mast_package::Package>, String>>,
}

impl CompilerTest {
    pub fn compiled_package(&mut self) -> Arc<miden_mast_package::Package> {
        if self.package.is_none() {
            self.compile_wasm_to_masm_program().unwrap();
        }
        match self.package.as_ref().unwrap().as_ref() {
            Ok(prog) => prog.clone(),
            Err(msg) => panic!("{msg}"),
        }
    }

    fn link_output(&mut self) -> &midenc_compile::LinkOutput {
        use midenc_compile::compile_to_optimized_hir;

        if self.hir.is_none() {
            let link_output = compile_to_optimized_hir(self.context.clone())
                .map_err(format_report)
                .expect("failed to translate wasm to hir component");
            self.hir = Some(link_output);
        }
        self.hir.as_ref().unwrap()
    }

    fn compile_wasm_to_masm_program(&mut self) -> Result<(), String> {
        use midenc_compile::{compile_link_output_to_masm_with_pre_assembly_stage, CodegenOutput};
        use midenc_hir::Context;

        let mut src = None;
        let mut masm_program = None;
        let mut stage = |output: CodegenOutput, _context: std::rc::Rc<Context>| {
            src = Some(output.component.to_string());
            if output.component.entrypoint.is_some() {
                masm_program = Some(Arc::clone(&output.component));
            }
            Ok(output)
        };

        let link_output = self.link_output().clone();
        let package = compile_link_output_to_masm_with_pre_assembly_stage(link_output, &mut stage)
            .map_err(format_report)?
            .unwrap_mast();

        assert!(src.is_some(), "failed to pretty print masm artifact");
        self.masm_src = src;
        self.ir_masm_program = masm_program.map(Ok);
        self.package = Some(Ok(Arc::new(package)));
        Ok(())
    }
}

/// Create a valid [Context] for `inputs` with `argv`, with useful defaults.
pub fn default_context<S, I>(inputs: I, argv: &[S]) -> std::rc::Rc<midenc_hir::Context>
where
    I: IntoIterator<Item = midenc_session::InputFile>,
    S: AsRef<str>,
{
    let session = default_session(inputs, argv);
    let context = std::rc::Rc::new(midenc_hir::Context::new(session));
    midenc_codegen_masm::register_dialect_hooks(&context);
    context
}

/// Create a valid [Session] for compiling `inputs` with `argv`, with useful defaults.
pub fn default_session<S, I>(inputs: I, argv: &[S]) -> std::rc::Rc<midenc_session::Session>
where
    I: IntoIterator<Item = midenc_session::InputFile>,
    S: AsRef<str>,
{
    use midenc_session::diagnostics::reporting::{self, ReportHandlerOpts};

    let result = reporting::set_hook(Box::new(|_| {
        let wrapping_width = 300; // avoid wrapped file paths in the backtrace
        Box::new(ReportHandlerOpts::new().width(wrapping_width).build())
    }));
    if result.is_ok() {
        reporting::set_panic_hook();
    }

    let argv = argv.iter().map(|arg| arg.as_ref());
    let session = midenc_compile::Compiler::new_session(inputs, None, argv);
    std::rc::Rc::new(session)
}

/// Get the directory for the top-level workspace
fn get_workspace_dir() -> String {
    // Get the directory for the integration test suite project
    let cargo_manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(
        std::env::current_dir()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
    );
    let cargo_manifest_dir_path = std::path::Path::new(&cargo_manifest_dir);
    // "Exit" the integration test suite project directory to the compiler workspace directory
    // i.e. out of the `tests/integration` directory
    let compiler_workspace_dir = cargo_manifest_dir_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_str()
        .unwrap();
    compiler_workspace_dir.to_string()
}

fn format_report(err: impl std::fmt::Display) -> String {
    format!("{}", err)
}

pub async fn create_fungible_faucet_account(
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
pub async fn assert_account_has_fungible_asset(
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

    let account_state: miden_objects::account::Account = account_record.into();

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
