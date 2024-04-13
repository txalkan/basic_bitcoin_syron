mod bitcoin_api;
mod bitcoin_wallet;
mod ecdsa_api;
mod types;

use ic_cdk::{api::management_canister::bitcoin::{
    BitcoinNetwork, GetUtxosResponse, MillisatoshiPerByte, Utxo,
}, query};
use ic_cdk_macros::{init, post_upgrade, pre_upgrade, update};
use types::SendRequest;
use std::cell::{Cell, RefCell};

use candid::Principal;
use ic_ckbtc_minter_tyron::{
    lifecycle::{
        self,
        init::MinterArg
    },
    state::{eventlog::Event, read_state},
    storage::record_event,
    tasks::{schedule_now, TaskType},
    updates::{
        self, get_btc_address::{GetBoxAddressArgs, GetBtcAddressArgs}, get_withdrawal_account::compute_subaccount, update_balance::{UpdateBalanceArgs, UpdateBalanceError, UtxoStatus}
    },
    MinterInfo
};

use icrc_ledger_types::icrc1::account::Subaccount;

thread_local! {
    // The bitcoin network to connect to.
    //
    // When developing locally this should be `Regtest`.
    // When deploying to the IC this should be `Testnet`.
    // `Mainnet` is currently unsupported.

    // @review (mainnet)
    static NETWORK: Cell<BitcoinNetwork> = Cell::new(BitcoinNetwork::Testnet);

    // The derivation path to use for ECDSA secp256k1.
    static DERIVATION_PATH: Vec<Vec<u8>> = vec![];

    // The ECDSA key name.
    static KEY_NAME: RefCell<String> = RefCell::new(String::from(""));
}

#[init]
pub fn init(network: BitcoinNetwork, args: MinterArg) {
    NETWORK.with(|n| n.set(network));

    KEY_NAME.with(|key_name| {
        key_name.replace(String::from(match network {
            // For local development, we use a special test key with dfx.
            BitcoinNetwork::Regtest => "dfx_test_key",
            // On the IC we're using a test ECDSA key.
            BitcoinNetwork::Mainnet | BitcoinNetwork::Testnet => "test_key_1",
        }))
    });
    
    match args {
        MinterArg::Init(args) => {
            record_event(&Event::Init(args.clone()));
            lifecycle::init::init(args);
            schedule_now(TaskType::ProcessLogic);
            schedule_now(TaskType::RefreshFeePercentiles);
            // schedule_now(TaskType::DistributeKytFee);

            #[cfg(feature = "self_check")]
            ok_or_die(check_invariants())
        }
        MinterArg::Upgrade(_) => {
            panic!("expected InitArgs got UpgradeArgs");
        }
    }
}

/// Returns the balance of the given bitcoin address.
#[update]
pub async fn get_balance(address: String) -> u64 {
    let network = NETWORK.with(|n| n.get());
    bitcoin_api::get_balance(network, address).await
}

/// Returns the UTXOs of the given bitcoin address.
#[update]
pub async fn get_utxos(address: String) -> GetUtxosResponse {
    let network = NETWORK.with(|n| n.get());
    bitcoin_api::get_utxos(network, address).await
}

/// Returns the 100 fee percentiles measured in millisatoshi/byte.
/// Percentiles are computed from the last 10,000 transactions (if available).
#[update]
pub async fn get_current_fee_percentiles() -> Vec<MillisatoshiPerByte> {
    let network = NETWORK.with(|n| n.get());
    bitcoin_api::get_current_fee_percentiles(network).await
}

/// Returns the P2PKH address of this canister at a specific derivation path.
#[update]
pub async fn get_p2pkh_address() -> String {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let network = NETWORK.with(|n| n.get());
    bitcoin_wallet::get_p2pkh_address(network, key_name, derivation_path).await
}

#[update]
pub async fn get_p2wpkh_address() -> String {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    bitcoin_wallet::get_p2wpkh_address(key_name, derivation_path).await
}

/// Send the given amount of bitcoin from this canister to the given address.
/// Return the transaction ID.

/// 1. Using P2PKH
#[update]
pub async fn send(request: types::SendRequest) -> String {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let tx_id = bitcoin_wallet::send(
        network,
        derivation_path,
        key_name,
        request.destination_address,
        request.amount_in_satoshi,
    )
    .await;

    tx_id.to_string()
}

/// 2. Using P2WPKH
#[update]
pub async fn mint(address: String, txid: String) -> String {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let tx_id = bitcoin_wallet::mint_p2wpkh(
        network,
        derivation_path,
        key_name,
        address,
        txid,
    )
    .await;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    let txid_hex = hex::encode(txid_bytes);
    txid_hex
}

#[update]
pub async fn test() -> Vec<String> {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let res = bitcoin_wallet::test_utxos(
        network,
        derivation_path,
        key_name,
    )
    .await;
    res
}

#[pre_upgrade]
fn pre_upgrade() {
    let network = NETWORK.with(|n| n.get());
    ic_cdk::storage::stable_save((network,)).expect("Saving network to stable store must succeed.");
}

#[post_upgrade]
fn post_upgrade(minter_arg: MinterArg) {
    let network = ic_cdk::storage::stable_restore::<(BitcoinNetwork,)>()
        .expect("Failed to read network from stable memory.")
        .0;

    //@review 
    init(network, minter_arg);
}

// Tyron's stablecoin metaprotocol

// fn check_anonymous_caller() {
//     ic_cdk::println!("caller: {}", ic_cdk::caller());
//     if ic_cdk::caller() == Principal::anonymous() {
//         panic!("anonymous caller not allowed")
//     }
// }

fn check_postcondition<T>(t: T) -> T {
    #[cfg(feature = "self_check")]
    ok_or_die(check_invariants());
    t
}

#[update]
async fn get_box_address(args: GetBoxAddressArgs) -> String {
    // check_anonymous_caller();
    updates::get_btc_address::get_box_address(args).await
}

#[update]
async fn update_balance(args: UpdateBalanceArgs) -> Result<Vec<UtxoStatus>, UpdateBalanceError> {
    // check_anonymous_caller();
    check_postcondition(updates::update_balance::update_balance(args).await)
}

#[update]
async fn get_susd(args: UpdateBalanceArgs, txid: String) -> String {
    let address = (&args.ssi).to_string();

    // @dev 1. Update Balance (the user's $Box MUST have BTC deposit confirmed)
    let _ = check_postcondition(updates::update_balance::update_balance(args).await);
    
    // @dev 2. Transfer stablecoin from minter to user address
    let tx_id = mint(address, txid).await;
    tx_id
}

#[update]
async fn update_ssi(args: UpdateBalanceArgs) -> String {
    let address = (&args.ssi).to_string();

    // @dev 1. Update Balance (the user's $Box MUST have BTC deposit confirmed)
    let _ = check_postcondition(updates::update_balance::update_balance(args).await);
    
    // @dev 2. Transfer stablecoin from minter to user address

    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let tx_id = bitcoin_wallet::send_p2wpkh(
        network,
        derivation_path,
        key_name,
        address,
        546,
    )
    .await;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    let txid_hex = hex::encode(txid_bytes);
    txid_hex
}

#[update]
async fn get_subaccount(ssi: String) -> Subaccount {
    compute_subaccount(1, &ssi)
}

// #[update]
// async fn get_xr() -> u64 {
//     let xr = match get_exchange_rate().await {
//         Ok(result) => result,
//         Err(_err) => {
//            return 0
//         }
//     };
//     xr.unwrap().rate
// }

#[query]
fn get_minter_info() -> MinterInfo {
    read_state(|s| MinterInfo {
        kyt_fee: s.kyt_fee,
        min_confirmations: s.min_confirmations,
        retrieve_btc_min_amount: s.retrieve_btc_min_amount,
    })
}