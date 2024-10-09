mod bitcoin_api;
mod bitcoin_wallet;
mod ecdsa_api;
mod constants;
mod types;
mod provider;
mod http;
mod tests;

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
use ic_ckbtc_minter_tyron::address::get_ssi_derivation_path;
use ic_ckbtc_minter_tyron::address::public_key_to_p2wpkh;
use ic_ckbtc_minter_tyron::lifecycle::init::BtcNetwork;
use ic_ckbtc_minter_tyron::updates::retrieve_btc::balance_of;
use ic_ckbtc_minter_tyron::updates::retrieve_btc::SyronLedger;
use ic_ckbtc_minter_tyron::updates::update_balance::syron_update;
use ic_ckbtc_minter_tyron::updates::update_balance::CollateralizedAccount;
use icrc_ledger_types::icrc1::account::Account;
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
        self, get_btc_address::{self, GetBoxAddressArgs, SyronOperation}, get_withdrawal_account::compute_subaccount, update_balance::{UpdateBalanceError, UtxoStatus, get_collateralized_account}
    },
    MinterInfo
};

use icrc_ledger_types::icrc1::account::Subaccount;

use candid::candid_method;

struct TransferResult {
    txid: String,
    syron: u64,
    limit: u64,
}

async fn syron_transfer(
    txid: String,
    provider: u64,
    cycles_cost: u128,
    key_name: String,
    origin_derivation_path: Vec<Vec<u8>>,
    origin_address: String,
    dst_address: &str,
    amount: u64,
) -> Result<TransferResult, UpdateBalanceError> {
    // @dev Check BRC-20 transfer inscription.
    let outcall = call_indexer_inscription(provider, txid.clone(), cycles_cost).await?;

    let outcall_json: Value = serde_json::from_str(&outcall).unwrap();

    // Access the "amt" field in the "brc20" object
    let receiver_address: String = outcall_json.pointer("/utxo/address")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if receiver_address != origin_address {
        return Err(UpdateBalanceError::GenericError{
            error_code: 302,
            error_message: format!("The inscription receiver address ({}) must be equal to the origin of the transfer ({})", receiver_address, origin_address),
        });
    }

    // Access the "amt" field in the "brc20" object
    let syron_inscription: String = outcall_json.pointer("/brc20/amt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // The Syron inscribed amount must be less than the limit or throw UpdateBalanceError::GenericError
    let syron_f64: f64 = syron_inscription.parse().unwrap_or(0.0);
    let syron_u64: u64 = (syron_f64 * 100_000_000 as f64) as u64;

    // Set a limit so that users cannot withdraw more than 2 cents above the amount, being 1 cent = 1_000_000
    let limit = amount + 2_000_000;

    if syron_u64 > limit {
        return Err(UpdateBalanceError::GenericError{
            error_code: 303,
            error_message: "Insufficient SUSD balance to withdraw the inscribed amount of stablecoin".to_string(),

        });
    }

    // @dev Send SUSD to the destination address

    let btc_network = NETWORK.with(|n| n.get());

    let tx_id = bitcoin_wallet::syron_p2wpkh(
        btc_network,
        key_name,
        origin_derivation_path,
        origin_address,
        &dst_address,
        txid,
    )
    .await?;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    let txid_hex = hex::encode(txid_bytes);

    Ok(TransferResult{
        txid: txid_hex,
        syron: syron_u64,
        limit
    })
}

/// Mint SUSD using P2WPKH - the transaction id must correspond to the required transfer inscription
pub async fn mint(ssi: String, txid: String, cycles_cost: u128, provider: u64) -> Result<String, UpdateBalanceError> {
    // @dev Read SUSD available balance (nonce #2)
    let balance = balance_of(SyronLedger::SUSD, &ssi, 2).await.unwrap();

    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());

    // if empty, throw error
    if key_name.is_empty() {
        return Err(UpdateBalanceError::GenericError{
            error_code: 301,
            error_message: "Key name is empty".to_string(),
        });
    }
    
    // @dev Get Syron Bitcoin address (The receiver of this transfer inscription must be equal to the Syron address)
    // @review (cost)
    let syron_derivation_path = DERIVATION_PATH.with(|d| d.clone());
    
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), syron_derivation_path.clone()).await;
    
    let syron_address = public_key_to_p2wpkh(&own_public_key);

    // @dev Send SUSD to the user's wallet (SSI)
    let transfer = syron_transfer(
        txid,
        provider,
        cycles_cost,
        key_name,
        syron_derivation_path,
        syron_address,
        &ssi,
        balance
    ).await?;

    // Update Syron SUSD Ledger
    // @dev Compute the new balance amount as the limit less the syron inscription
    let new_balance = transfer.limit - transfer.syron;

    // do not consider new balance below 0.003 SUSD
    if new_balance < 3_000_000 {
        // withdraw full balance
        syron_update(&ssi, 2, 3, balance).await.unwrap(); // @doc 2 is the nonce of the balance subaccount, and 3 the BRC-20 subaccount.
    } else {
        syron_update(&ssi, 2, 3, transfer.syron).await.unwrap();
    }

    Ok(transfer.txid)
}

fn check_postcondition<T>(t: T) -> T {
    #[cfg(feature = "self_check")]
    ok_or_die(check_invariants());
    t
}

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
            BitcoinNetwork::Mainnet => "key_1",
            // BitcoinNetwork::Signet => "sig_key_1",
            // On the IC we're using a test ECDSA key.
            _ => "test_key_1"
            
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

/// Returns the 100 fee percentiles measured in millisatoshi/byte.
/// Percentiles are computed from the last 10,000 transactions (if available).
#[update]
pub async fn get_current_fee_percentiles() -> Vec<MillisatoshiPerByte> {
    let network = NETWORK.with(|n| n.get());
    bitcoin_api::get_current_fee_percentiles(network).await
}
#[update]
pub async fn get_fee_percentile(percentile: u64) -> u64 {
    let network = NETWORK.with(|n| n.get());
    let fee_percentiles = bitcoin_api::get_current_fee_percentiles(network).await;
    fee_percentiles[percentile as usize]
}

/// Returns the P2WPKH address of this canister at a specific derivation path.
#[update]
pub async fn get_p2wpkh_address() -> String {
    let derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    bitcoin_wallet::get_p2wpkh_address(key_name, derivation_path).await
}

#[update]
async fn susd_balance_of(ssi: String, nonce: u64) -> u64 {
    let res = match balance_of(SyronLedger::SUSD, &ssi, nonce).await {
        Ok(bal) => bal,
        Err(_err) => 0
    };
    res
}

#[update]
async fn sbtc_balance_of(ssi: String, nonce: u64) -> u64 {
    let res = match balance_of(SyronLedger::BTC, &ssi, nonce).await {
        Ok(bal) => bal,
        Err(_err) => 0
    };
    res
}

#[query]
async fn get_subaccount(nonce: u64, ssi: String) -> Subaccount {
    compute_subaccount(nonce, &ssi)
}

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
pub async fn get_inscription(txid: String, cycles_cost: u64, provider: u64) -> Result<String, UpdateBalanceError> {
    call_indexer_inscription(provider, txid.clone(), cycles_cost as u128).await
}

#[update]
pub async fn get_indexed_balance(id: String) -> Result<String, UpdateBalanceError> {
    call_indexer_balance(id, 0, 72_000_000).await //@mainnet
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
async fn get_box_address(args: GetBoxAddressArgs) -> String {
    // check_anonymous_caller();
    get_btc_address::get_box_address(args).await
}

#[update]
pub async fn withdraw_susd(args: GetBoxAddressArgs, txid: String, cycles_cost: u64, provider: u64) -> Result<String, UpdateBalanceError> {
    // @review (mainnet) automate provider config per network
    
    // @dev Verify args.op = GetSyron or throw erorr
    if args.op != SyronOperation::GetSyron {
        return Err(UpdateBalanceError::GenericError{
            error_code: 300,
            error_message: "Invalid operation".to_string(),
        });
    }

    // @dev 1. Update Balance (the user's SDB MUST have BTC deposit confirmed)
    updates::update_balance::update_ssi_balance(args.clone()).await?;

    mint(args.ssi, txid, cycles_cost as u128, provider).await
}

#[update]
async fn redeem_btc(args: GetBoxAddressArgs, txid: String) -> Result<String, UpdateBalanceError> {
    // @dev

    // 1. Verify args.op = RedeemBitcoin or throw erorr
    if args.op != SyronOperation::RedeemBitcoin {
        return Err(UpdateBalanceError::GenericError{
            error_code: 400,
            error_message: "Invalid operation".to_string(),
        });
    }

    // 2. Get the Syron ledger's SUSD record of the user's SDB (subaccount with nonce 1) = SUSD[1]
    let ssi = (&args.ssi).to_string();

    let loan = balance_of(SyronLedger::SUSD, &ssi, 1).await.map_err(|_| UpdateBalanceError::GenericError {
        error_code: 401,
        error_message: "Failed to get loan balance".to_string(),
    })?;
    // if the loan balance is 0, throw an error
    if loan == 0 {
        return Err(UpdateBalanceError::GenericError {
            error_code: 402,
            error_message: "Your loan balance is zero, which is not allowed for redemptions.".to_string(),
        });
    }

    // 3. Check SUSD balance of the safety deposit box with the Tyron indexer
    let sdb = get_btc_address::get_box_address(args.clone()).await;
    let syron_u64: u64 = match get_syron_balance(sdb.clone()).await {
        Some(balance) if balance > 0 => balance,
        _ => {
            return Err(UpdateBalanceError::GenericError {
                error_code: 403,
                error_message: "Invalid balance from indexer".to_string(),
            });
        }
    };

    // 4. Deposit must be at least the loan amount SUSD[1] or throw UpdateBalanceError
    let limit = 2_000_000; // @governance
    if syron_u64 < loan - limit {
        return Err(UpdateBalanceError::GenericError{
            error_code: 404,
            error_message: "Insufficient SUSD deposited balance to redeem bitcoin".to_string(),
        });
    }
    // if syron_u64 > loan {
    //     return Err(UpdateBalanceError::GenericError{
    //         error_code: 404,
    //         error_message: "The SUSD balance in your SDB exceeds the loan amount. Please withdraw SUSD and try again.".to_string(),
    //     });
    // }

    // 5. Check BRC-20 transfer inscription @mainnet
    let outcall = call_indexer_inscription(0, txid.clone(), 72_000_000).await?;

    let outcall_json: Value = serde_json::from_str(&outcall).unwrap();

    // Access the "amt" field in the "brc20" object
    let receiver_address: String = outcall_json.pointer("/utxo/address")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if receiver_address != sdb {
        return Err(UpdateBalanceError::GenericError{
            error_code: 405,
            error_message: format!("The inscription receiver address ({}) must be equal to your SDB ({})", receiver_address, sdb),
        });
    }

    // Access the "amt" field in the "brc20" object
    let syron_inscription: String = outcall_json.pointer("/brc20/amt")
    .and_then(Value::as_str)
    .unwrap_or("")
    .to_string();

    // The Syron inscribed amount must be equal to the loan or throw UpdateBalanceError::GenericError
    let syron_f64: f64 = syron_inscription.parse().unwrap_or(0.0);
    let syron_u64_i: u64 = (syron_f64 * 100_000_000 as f64) as u64;

    if syron_u64_i < loan - limit || syron_u64_i > loan + limit || syron_u64_i > syron_u64 {
        return Err(UpdateBalanceError::GenericError{
            error_code: 406,
            error_message: "Incorrect inscribed amount of stablecoin to repay the loan.".to_string(),
        });
    }

    // if syron_u64_i != syron_u64 {
    //     return Err(UpdateBalanceError::GenericError{
    //         error_code: 406,
    //         error_message: "Insufficient inscribed amount of stablecoin".to_string(),
    //     });
    // }
    
    // 6. Get Syron Bitcoin address
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
    // if empty, throw error
    if key_name.is_empty() {
        return Err(UpdateBalanceError::GenericError{
            error_code: 407, // @review (key) error code
            error_message: "Key name is empty".to_string(),
        });
    }
    
    let syron_derivation_path = DERIVATION_PATH.with(|d| d.clone());
    
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), syron_derivation_path.clone()).await;
    
    let syron_address = public_key_to_p2wpkh(&own_public_key);

    // 7. Transfer bitcoin from SDB to wallet
    let btc_network = NETWORK.with(|n| n.get());
    let amount = balance_of(SyronLedger::BTC, &ssi, 1).await.unwrap();

    let tx_id = bitcoin_wallet::burn_p2wpkh(
        amount,
        &ssi,
        btc_network,
        key_name,
        sdb,
        &ssi,
        &syron_address,
        txid
    ).await?;

    // 8. Update Syron ledgers of debtor
    updates::update_balance::update_ssi_balance(args).await?;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    Ok(hex::encode(txid_bytes))
}

#[update]
async fn redemption_gas(args: GetBoxAddressArgs) -> Result<u64, UpdateBalanceError> {
    // @dev Verify args.op = RedeemBitcoin or throw erorr
    if args.op != SyronOperation::RedeemBitcoin {
        return Err(UpdateBalanceError::GenericError{
            error_code: 400,
            error_message: "Invalid operation".to_string(),
        });
    }

    let ssi = (&args.ssi).to_string();
    let sdb = get_btc_address::get_box_address(args.clone()).await;

    let syron_derivation_path = DERIVATION_PATH.with(|d| d.clone());
    
    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());

    // if empty, throw error
    if key_name.is_empty() {
        return Err(UpdateBalanceError::GenericError{
            error_code: 301,
            error_message: "Key name is empty".to_string(),
        });
    }

    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), syron_derivation_path.clone()).await;
    
    let syron_address = public_key_to_p2wpkh(&own_public_key);

    let amount = balance_of(SyronLedger::BTC, &ssi, 1).await.unwrap();
    let btc_network = NETWORK.with(|n| n.get());
    
    let gas = bitcoin_wallet::gas_p2wpkh(
        amount,
        &ssi,
        btc_network,
        sdb,
        &ssi,
        &syron_address
    )
    .await;

    Ok(gas)
}

#[update]
async fn get_account(ssi: String, dummy: bool) -> Result<CollateralizedAccount, UpdateBalanceError> {
    check_postcondition(get_collateralized_account(&ssi, dummy).await)
}

#[update]
// @review the order of UTXOs is important to transfer the proper inscription
async fn liquidate(args: GetBoxAddressArgs, id: String, txid: String) -> Result<Vec<String>, UpdateBalanceError> {
    let ssi: &str = &args.ssi;
    
    // @dev 1. Verify collateral ratio is below 12,000 basis points or throw error
    let collateralized_account = get_collateralized_account(ssi, true).await?;

    if collateralized_account.collateral_ratio > 12_000 {
        return Err(UpdateBalanceError::GenericError{
            error_code: 500,
            error_message: "Collateral ratio is above 1,2".to_string(),
        });
    }

    let btc_1 = collateralized_account.btc_1;
    let susd_1 = collateralized_account.susd_1;

    let sdb_debtor = get_btc_address::get_box_address(args.clone()).await;

    let liquidator = GetBoxAddressArgs {
        ssi: id.clone(),
        op: get_btc_address::SyronOperation::Liquidation,
    };

    let sdb_liquidator = get_btc_address::get_box_address(liquidator).await;

    // 2. Check the liquidator's SUSD balance in their safety deposit box with the Tyron indexer
    let syron_u64: u64 = match get_syron_balance(sdb_liquidator.clone()).await {
        Some(balance) => balance,
        None => {
            return Err(UpdateBalanceError::GenericError{
                error_code: 501,
                error_message: "Invalid balance".to_string(),
            });
        }
    };

    // 3. Liquidator's balance must be at least >= debtor's SUSD[1] OR throw UpdateBalanceError
    if syron_u64 <= susd_1 {
        return Err(UpdateBalanceError::GenericError{
            error_code: 502,
            error_message: "Insufficient SUSD balance in the liquidator's account to liquidate the debtor.".to_string(),
        });
    }

    // 4. Transfer syron from liquidator's SDB to minter and bitcoin from debtor's SDB to the user's wallet (liquidator)
    let mut res: Vec<String> = Vec::new();

    let provider = 0; // @review (mainnet) provider ID
    let cycles_cost = 72_000_000;

    let key_name = KEY_NAME.with(|kn| kn.borrow().to_string());
     
    let minter_derivation_path = DERIVATION_PATH.with(|d| d.clone());
    let dst_address = bitcoin_wallet::get_p2wpkh_address(key_name.clone(), minter_derivation_path).await;

    let sdb_subaccount = compute_subaccount(1, &id);
    let account = Account {
        owner: ic_cdk::id(),
        subaccount: Some(sdb_subaccount)
    };
    let origin_derivation_path: Vec<Vec<u8>> = get_ssi_derivation_path(&account, &id).into_iter().map(|index| index.0).collect();

    let payment = syron_transfer(
        txid,
        provider,
        cycles_cost,
        key_name.clone(),
        origin_derivation_path,
        sdb_liquidator,
        &dst_address,
        susd_1).await?;
    res.push(payment.txid);
    
    let network = NETWORK.with(|n| n.get());
    
    let tx_id = bitcoin_wallet::liquidate_p2wpkh(
        btc_1,
        ssi,
        network,
        key_name,
        sdb_debtor,
        &id
    )
    .await;

    let txid_bytes = tx_id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
    res.push(hex::encode(txid_bytes));

    // 5. Update Syron ledgers (debtor)
    updates::update_balance::update_ssi_balance(args).await?;

    Ok(res)
}
