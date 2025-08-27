// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
//
// extern crate alloc;
// use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

bindings::export!(BasicWalletTxScript with_types_in bindings);

mod bindings;

use bindings::{exports::miden::base::transaction_script::Guest, miden::basic_wallet::basic_wallet};
use miden::{intrinsics::advice::adv_push_mapvaln, *};

struct BasicWalletTxScript;

// Input layout constants
const TAG_INDEX: usize = 0;
const AUX_INDEX: usize = 1;
const NOTE_TYPE_INDEX: usize = 2;
const EXECUTION_HINT_INDEX: usize = 3;
const RECIPIENT_START: usize = 4;
const RECIPIENT_END: usize = 8;
const ASSET_START: usize = 8;
const ASSET_END: usize = 12;

impl Guest for BasicWalletTxScript {
    fn run(arg: Word) {
        let num_felts = adv_push_mapvaln(arg.clone());
        let num_felts_u64 = num_felts.as_u64();
        assert_eq(Felt::from_u32((num_felts_u64 % 4) as u32), felt!(0));
        let num_words = Felt::from_u64_unchecked(num_felts_u64 / 4);
        let commitment = arg;
        let input = adv_load_preimage(num_words, commitment);
        let tag = input[TAG_INDEX];
        let aux = input[AUX_INDEX];
        let note_type = input[NOTE_TYPE_INDEX];
        let execution_hint = input[EXECUTION_HINT_INDEX];
        let recipient: [Felt; 4] = input[RECIPIENT_START..RECIPIENT_END].try_into().unwrap();
        let note_idx = miden::tx::create_note(
            tag.into(),
            aux,
            note_type.into(),
            execution_hint,
            recipient.into(),
        );
        let asset: [Felt; 4] = input[ASSET_START..ASSET_END].try_into().unwrap();
        basic_wallet::move_asset_to_note(asset.into(), note_idx);
    }
}
