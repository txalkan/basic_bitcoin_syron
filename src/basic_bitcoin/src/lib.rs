mod bitcoin_api;
mod bitcoin_wallet;
mod ecdsa_api;
mod constants;
mod types;
mod provider;
mod http;

pub use crate::constants::*;
pub use crate::types::*;
pub use crate::provider::*;
pub use crate::http::*;

use bitcoin::Network;
use bitcoin::OutPoint;
use ic_cdk::api::management_canister::http_request::HttpResponse;
use ic_cdk::api::management_canister::http_request::TransformArgs;
use ic_cdk::{api::management_canister::bitcoin::{
    BitcoinNetwork, GetUtxosResponse, MillisatoshiPerByte,
}, query};
use ic_cdk_macros::{init, post_upgrade, pre_upgrade, update};
use ic_ckbtc_minter_tyron::address::public_key_to_p2wpkh;
use ic_ckbtc_minter_tyron::lifecycle::init::BtcNetwork;
use ic_ckbtc_minter_tyron::updates::retrieve_btc::balance_of;
use ic_ckbtc_minter_tyron::updates::retrieve_btc::SyronLedger;
use ic_ckbtc_minter_tyron::updates::update_balance::syron_update;
use serde_json::Value;
use std::cell::{Cell, RefCell};

use ic_ckbtc_minter_tyron::{
    lifecycle::{
        self,
        init::MinterArg
    },
    state::{eventlog::Event, read_state},
    storage::record_event,
    tasks::{schedule_now, TaskType},
    updates::{
        self, get_btc_address::{self, GetBoxAddressArgs, SyronOperation}, get_withdrawal_account::compute_subaccount, update_balance::{UpdateBalanceError, UtxoStatus}
    },
    MinterInfo
};

use icrc_ledger_types::icrc1::account::Subaccount;

use candid::candid_method;

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
            // BitcoinNetwork::Signet => "sig_key_1",
            // On the IC we're using a test ECDSA key.
            BitcoinNetwork::Testnet => "test_key_1",
            BitcoinNetwork::Mainnet => "main_key_1",
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

    init_service_provider()
}

#[update]
pub async fn get_network() -> BitcoinNetwork {
    let network = NETWORK.with(|n| n.get());
    let network_ = read_state(|s| (s.btc_network));

    network
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
pub async fn send(request: SendRequest) -> String {
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
pub async fn mint(ssi: String, txid: String, cycles_cost: u128, provider: u64) -> Result<String, UpdateBalanceError> {
    // @dev Check BRC-20 transfer inscription.
    let outcall = call_indexer_inscription(provider, txid.clone(), cycles_cost).await?;

    let outcall_json: Value = serde_json::from_str(&outcall).unwrap();

    // Access the "amt" field in the "brc20" object
    let receiver_address: String = outcall_json.pointer("/data/address")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Access the "amt" field in the "brc20" object
    let syron_inscription: String = outcall_json.pointer("/data/brc20/amt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // The Syron inscribed amount must be less than the limit or throw UpdateBalanceError::GenericError
    let syron_f64: f64 = syron_inscription.parse().unwrap_or(0.0);
    let syron_u64: u64 = (syron_f64 * 100_000_000 as f64) as u64;
    
    // @dev Read SU$D available balance (nonce #2)
    let balance = balance_of(SyronLedger::SUSD, &ssi, 2).await.unwrap();

    // Set a limit so that users cannot withdraw more than 2 cents above their balance, being 1 cent = 1_000_000
    let limit = balance + 2_000_000;

    if syron_u64 > limit {
        return Err(UpdateBalanceError::GenericError{
            error_code: 300,
            error_message: "Insufficient SUSD balance to withdraw the inscribed amount of stablecoin".to_string(),

        });
    }

    // @dev Get Syron Bitcoin address (The receiver of the transfer inscription must be equal to the Syron address)
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;
    
    let syron_address = public_key_to_p2wpkh(&own_public_key);

    if receiver_address != syron_address {
        return Err(UpdateBalanceError::GenericError{
            error_code: 301,
            error_message: format!("Receiver address ({}) must be equal to the Syron address ({})", receiver_address, syron_address),
        });
    }

    // @dev Send SU$D to user's address

    let network = NETWORK.with(|n| n.get());
    
    let tx_id = bitcoin_wallet::mint_p2wpkh(
        network,
        derivation_path,
        key_name,
        syron_address,
        &ssi,
        txid,
    )
    .await;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    let txid_hex = hex::encode(txid_bytes);

    // Update Syron SU$D Ledger
    // @dev Compute the new balance amount as the limit less the syron inscription
    let new_balance = limit - syron_u64;

    // do not consider new balance below 0.003 SU$D
    if new_balance < 3_000_000 {
        // withdraw full balance
        syron_update(&ssi, 2, 3, balance).await.unwrap(); // @doc 2 is the nonce of the balance subaccount, and 3 the BRC-20 subaccount.
    } else {
        syron_update(&ssi, 2, 3, syron_u64).await.unwrap();
    }

    Ok(txid_hex)
}

#[query(hidden = true)]
fn transform_request(args: TransformArgs) -> HttpResponse {
    do_transform_request(args)
}

#[query(hidden = true)]
fn transform_unisat_request(args: TransformArgs) -> HttpResponse {
    do_transform_unisat_request(args)
}

#[query(hidden = true)]
fn transform_bis_request(args: TransformArgs) -> HttpResponse {
    do_transform_bis_request(args)
}

#[update]
pub async fn get_inscription(txid: String, cycles_cost: u64, provider: u64) -> Result<String, UpdateBalanceError> {
    call_indexer_inscription(provider, txid.clone(), cycles_cost as u128).await
}

#[update]
pub async fn withdraw_susd(args: GetBoxAddressArgs, txid: String, cycles_cost: u64, provider: u64) -> Result<String, UpdateBalanceError> {
    mint(args.ssi, txid, cycles_cost as u128, provider).await
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
    get_btc_address::get_box_address(args).await
}

// @dev Testnet only: Check bitcoin deposit and mint OR burn stablecoin
#[update]
async fn update_ssi_balance(args: GetBoxAddressArgs) -> Result<Vec<UtxoStatus>, UpdateBalanceError> {
    // check_anonymous_caller();
    check_postcondition(updates::update_balance::update_ssi_balance(args).await)
}

#[update]
async fn get_susd(args: GetBoxAddressArgs, txid: String) -> Result<String, UpdateBalanceError> {
    let ssi = (&args.ssi).to_string();

    // @dev 1. Update Balance (the user's $Box MUST have BTC deposit confirmed)
    // @review (mint) syron operation
    let _ = check_postcondition(updates::update_balance::update_ssi_balance(args).await);
    
    // @dev 2. Transfer stablecoin from minter to user address
    // let res = mint(ssi, txid, cycles_cost as u128, 1).await; // @review (mainnet) provider ID
    
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;
    
    let syron_address = public_key_to_p2wpkh(&own_public_key);

    // @dev Send SU$D to user's address

    let network = NETWORK.with(|n| n.get());
    
    let tx_id = bitcoin_wallet::mint_p2wpkh(
        network,
        derivation_path,
        key_name,
        syron_address,
        &ssi,
        txid,
    )
    .await;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    let txid_hex = hex::encode(txid_bytes);

    Ok(txid_hex)
}

// @test
#[update]
async fn update_ssi(args: GetBoxAddressArgs) -> String {
    let address = (&args.ssi).to_string();

    // @dev 1. Update Balance (the user's $Box MUST have BTC deposit confirmed)
    let _ = check_postcondition(updates::update_balance::update_ssi_balance(args).await);
    
    // @dev 2. Transfer stablecoin from minter to the user's wallet
    
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

#[query]
async fn get_subaccount(nonce: u64, ssi: String) -> Subaccount {
    compute_subaccount(nonce, &ssi)
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

#[update(name = "addServiceProvider")]// @review (mainnet),, guard = "require_add_provider")]
#[candid_method(rename = "addServiceProvider")]
fn add_service_provider(args: RegisterProviderArgs) -> u64 {
    register_provider(args)
}

#[query(name = "getServiceProviderMap")]// @review (mainnet), guard = "require_manage_or_controller")]
#[candid_method(query, rename = "getServiceProviderMap")]
fn get_service_provider_map() -> Vec<(ServiceProvider, u64)> {
    SERVICE_PROVIDER_MAP.with(|map| {
        map.borrow()
            .iter()
            .filter_map(|(k, v)| Some((k.try_into().ok()?, v)))
            .collect()
    })
}

#[update]
pub async fn get_indexed_balance(id: String) -> Result<String, UpdateBalanceError> {
    call_indexer_balance(id, 1, 72_000_000).await
}

#[update]
async fn redeem_btc(args: GetBoxAddressArgs) -> Result<String, UpdateBalanceError> {
    // verify args.op = RedeemBitcoin or throw erorr
    if args.op != SyronOperation::RedeemBitcoin {
        return Err(UpdateBalanceError::GenericError{
            error_code: 400,
            error_message: "Invalid operation".to_string(),
        });
    }   

    let ssi = (&args.ssi).to_string();
    let sdb = get_btc_address::get_box_address(args.clone()).await;

    // @dev
    // 1. Check SU$D balance of the safety deposit box with the Tyron indexer
    let outcall = call_indexer_balance(sdb.clone(), 1, 72_000_000).await?;
    let syron_f64: f64 = outcall.parse().unwrap_or(0.0);
    let syron_u64: u64 = (syron_f64 * 100_000_000 as f64) as u64;
    
    // 2. Get the SU$D balance of the user's $Box (subaccount with nonce 1) = SU$D[1]
    // 3. Deposit must be at lest >= SU$D[1] - 0.02 SU$D OR throw UpdateBalanceError
    let balance = balance_of(SyronLedger::SUSD, &ssi, 1).await.unwrap();
    let limit = balance - 2_000_000;

    if syron_u64 < limit {
        return Err(UpdateBalanceError::GenericError{
            error_code: 401,
            error_message: "Insufficient SUSD deposited balance to redeem bitcoin".to_string(),
        });
    }

    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    
    // 4. Transfer bitcoin from SDB to wallet
    let amount = balance_of(SyronLedger::BTC, &ssi, 1).await.unwrap();

    let tx_id = bitcoin_wallet::burn_p2wpkh(
        amount,
        &ssi,
        network,
        key_name,
        sdb,
        &ssi,
    )
    .await;

    // 4. Update Syron ledgers of debtor
    let _ = check_postcondition(updates::update_balance::update_ssi_balance(args).await);

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    Ok(hex::encode(txid_bytes))
}


#[update]
async fn liquidate(args: GetBoxAddressArgs, id: String) -> Result<String, UpdateBalanceError> {
    let ssi: &str = &args.ssi;
    
    // @dev Verify collateral ratio is below 12,000 basis points or throw error
    let btc_1 = balance_of(SyronLedger::BTC, ssi, 1).await.unwrap();
    let susd_1 = balance_of(SyronLedger::SUSD, ssi, 1).await.unwrap();
    // let collateral_ratio = btc_1 * exchange_rate / susd_1; @review move upstream
    
    let sdb_debtor = get_btc_address::get_box_address(args.clone()).await;

    let liquidator = GetBoxAddressArgs {
        ssi: id.clone(),
        op: get_btc_address::SyronOperation::Liquidation,
    };

    let sdb_liquidator = get_btc_address::get_box_address(liquidator).await;

    // @dev
    // 2. Check the liquidator's SUSD balance in their safety deposit box with the Tyron indexer
    let outcall = call_indexer_balance(sdb_liquidator.clone(), 1, 72_000_000).await?;
    let syron_f64: f64 = outcall.parse().unwrap_or(0.0);
    let syron_u64: u64 = (syron_f64 * 100_000_000 as f64) as u64;
    
    // 3. Liquidator's balance must be at lest >= SU$D[1] - 0.02 SU$D OR throw UpdateBalanceError
    let limit = btc_1 - 2_000_000;

    if syron_u64 < limit {
        return Err(UpdateBalanceError::GenericError{
            error_code: 501,
            error_message: "Insufficient liquidator SUSD balance to liquidate debtor".to_string(),
        });
    }

    let network = NETWORK.with(|n| n.get());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    
    // 4. Transfer bitcoin from debtor's SDB to the user's wallet (SSI)
    let amount = balance_of(SyronLedger::BTC, &ssi, 1).await.unwrap();

    let tx_id = bitcoin_wallet::burn_p2wpkh(
        amount,
        ssi,
        network,
        key_name,
        sdb_debtor,
        &id
    )
    .await;

    // 4. Update Syron ledgers
    let _ = check_postcondition(updates::update_balance::update_ssi_balance(args).await);

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    Ok(hex::encode(txid_bytes))
}
