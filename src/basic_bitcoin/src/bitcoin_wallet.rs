//! A demo of a very bare-bones bitcoin "wallet".
//!
//! The wallet here showcases how bitcoin addresses can be be computed
//! and how bitcoin transactions can be signed. It is missing several
//! pieces that any production-grade wallet would have, including:
//! * Caching spent UTXOs so that they are not reused in future transactions.
//! * Option to set the fee.

use crate::{bitcoin_api, ecdsa_api};
use bitcoin::util::psbt::serialize::Serialize;
use bitcoin::{
    blockdata::{script::Builder, witness::Witness},
    hashes::Hash,
    Address, AddressType, EcdsaSighashType, OutPoint, Script, Transaction, TxIn, TxOut, Txid,
};
use ic_cdk::api::management_canister::bitcoin::{MillisatoshiPerByte, BitcoinNetwork, Satoshi, Utxo,  Outpoint};
use ic_cdk::print;
use ic_ckbtc_minter_tyron::address::BitcoinAddress;
use ic_ckbtc_minter_tyron::logs::P1;
use ic_ckbtc_minter_tyron::management::get_utxos;
use ic_ckbtc_minter_tyron::{
    state::Network,
    tx::{self, SignedTransaction, UnsignedInput, UnsignedTransaction, SignedInput},
    management::{sign_with_ecdsa, CallError, CallSource},
    signature::EncodedSignature,
    address::public_key_to_p2wpkh
};
use sha2::Digest;
use std::str::FromStr;
use serde_bytes::ByteBuf;
use ic_management_canister_types::DerivationPath;
use ic_canister_log::log;
use std::fmt;

const SIG_HASH_TYPE: EcdsaSighashType = EcdsaSighashType::All;

struct DisplayOutpoint<'a>(pub &'a Outpoint);

impl fmt::Display for DisplayOutpoint<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}:{}", self.0.txid, self.0.vout)
    }
}

/// Returns the P2PKH address of this canister at the given derivation path.
pub async fn get_p2pkh_address(
    network: BitcoinNetwork,
    key_name: String,
    derivation_path: Vec<Vec<u8>>,
) -> String {
    // Fetch the public key of the given derivation path.
    let public_key = ecdsa_api::ecdsa_public_key(key_name, derivation_path).await;

    // Compute the address.
    public_key_to_p2pkh_address(network, &public_key)
}

pub async fn get_p2wpkh_address(
    key_name: String,
    derivation_path: Vec<Vec<u8>>,
) -> String {
    // Fetch the public key of the given derivation path.
    let public_key = ecdsa_api::ecdsa_public_key(key_name, derivation_path).await;

    public_key_to_p2wpkh(&public_key)
}

/// Sends a transaction to the network that transfers the given amount to the
/// given destination, where the source of the funds is the canister itself
/// at the given derivation path.
pub async fn send(
    network: BitcoinNetwork,
    derivation_path: Vec<Vec<u8>>,
    key_name: String,
    dst_address: String,
    amount: Satoshi,
) -> Txid {
    // Get fee percentiles from previous transactions to estimate our own fee.
    let fee_percentiles = bitcoin_api::get_current_fee_percentiles(network).await;

    let fee_per_byte = if fee_percentiles.is_empty() {
        // There are no fee percentiles. This case can only happen on a regtest
        // network where there are no non-coinbase transactions. In this case,
        // we use a default of 2000 millisatoshis/byte (i.e. 2 satoshi/byte)
        2000
    } else {
        // Choose the 50th percentile for sending fees.
        fee_percentiles[50]
    };

    // Fetch our public key, P2PKH address, and UTXOs.
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;
    let own_address = public_key_to_p2pkh_address(network, &own_public_key);

    print("Fetching UTXOs...");
    // Note that pagination may have to be used to get all UTXOs for the given address.
    // For the sake of simplicity, it is assumed here that the `utxo` field in the response
    // contains all UTXOs.
    let own_utxos = bitcoin_api::get_utxos(network, own_address.clone())
        .await
        .utxos;

    let own_address = Address::from_str(&own_address).unwrap();
    let dst_address = Address::from_str(&dst_address).unwrap();

    // Build the transaction that sends `amount` to the destination address.
    let transaction = build_transaction(
        &own_public_key,
        &own_address,
        &own_utxos,
        &dst_address,
        amount,
        fee_per_byte,
    )
    .await;

    let tx_bytes = transaction.serialize();
    print(&format!("Transaction to sign: {}", hex::encode(tx_bytes)));

    // Sign the transaction.
    let signed_transaction = sign_transaction_p2pkh(
        &own_public_key,
        &own_address,
        transaction,
        key_name,
        derivation_path,
        ecdsa_api::sign_with_ecdsa,
    )
    .await;

    let signed_transaction_bytes = signed_transaction.serialize();
    print(&format!(
        "Signed transaction: {}",
        hex::encode(&signed_transaction_bytes)
    ));

    print("Sending transaction...");
    bitcoin_api::send_transaction(network, signed_transaction_bytes).await;
    print("Done");

    signed_transaction.txid()
}

pub async fn mint_p2wpkh(
    btc_network: BitcoinNetwork,
    derivation_path: Vec<Vec<u8>>,
    key_name: String,
    dst_address: String,
    txid: String,
) -> [u8;32] {
    // Get fee percentiles from previous transactions to estimate our own fee.
    // let fee_percentiles = bitcoin_api::get_current_fee_percentiles(btc_network).await;

    // @dev Gas in satoshis per byte @review (mainnet)
    let fee_per_byte = 50000;
    // if fee_percentiles.is_empty() {
        // There are no fee percentiles. This case can only happen on a regtest
        // network where there are no non-coinbase transactions. In this case,
        // we use a default of 20000 millisatoshis/byte (i.e. 20 satoshi/byte)
    //     20000
    // } else {
        // Choose the 50th percentile for sending fees.
    //     fee_percentiles[50]
    // };

    // @dev Fetch main public key, address, and UTXOs.
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;
    
    //@review (mainnet)
    let own_address = public_key_to_p2wpkh(&own_public_key);

    let network: Network = match btc_network {
        BitcoinNetwork::Mainnet => Network::Mainnet,
        BitcoinNetwork::Testnet => Network::Testnet,
        BitcoinNetwork::Regtest => Network::Regtest,
    };

    print("Fetching UTXOs...");
    // Note that pagination may have to be used to get all UTXOs for the given address.
    // For the sake of simplicity, it is assumed here that the `utxo` field in the response
    // contains all UTXOs.
    let own_utxos: Vec<Utxo> =
        bitcoin_api::get_utxos(btc_network, own_address.clone())
        .await
        .utxos;

    // let own_utxos: Vec<ic_btc_interface::Utxo> =
        // get_utxos(network, &own_address, 1, CallSource::Client) // @review (mainnet) min confirmations
        // .await
        // .unwrap().utxos;

    // for utxo in &own_utxos {
    //     log!(
    //         P1,
    //         "Minter UTXO: {}",
    //         DisplayOutpoint(&utxo.outpoint)
    //     );
    // }

    let mut option_utxo: Option<Utxo> = None;
    let mut fee_utxos = own_utxos.clone();

    for (index, utxo) in own_utxos.iter().enumerate() {
        // let outpoint = Outpoint {
        //     txid: utxo.outpoint.txid.as_ref().to_vec(),
        //     vout: utxo.outpoint.vout
        // };

        log!(
            P1,
            "Minter UTXO: {}",
            DisplayOutpoint(&utxo.outpoint)
        );

        let txid_bytes = utxo.outpoint.txid.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();
        let txid_hex = hex::encode(txid_bytes);

        if txid_hex == txid {
            // let ssi_utxo = Utxo {
            //     outpoint,
            //     value: utxo.value,
            //     height: utxo.height
            // };
            option_utxo = Some(/*ssi_*/utxo.clone());
            fee_utxos.remove(index);
            break
        }
    }

    let select_utxo = option_utxo.expect("No matching UTXO found!");

    let own_address = BitcoinAddress::parse(&own_address, network).unwrap();
    let dst_address = BitcoinAddress::parse(&dst_address, network).unwrap();
    
    // @dev Builds the transaction that sends the selected UTXO (transfer inscription) to the destination address.
    let transaction = build_unsigned_mint(
        &own_public_key,
        own_address,
        select_utxo,
        &fee_utxos,
        dst_address,
        fee_per_byte,
    )
    .await;

    // Sign the transaction.
    let signed_transaction: SignedTransaction = sign_transaction_p2wpkh(
        &own_public_key,
        transaction,
        key_name,
        derivation_path,
    )
    .await.unwrap();

    print("Sending transaction...");
    let signed_transaction_bytes = signed_transaction.serialize();
    bitcoin_api::send_transaction(btc_network, signed_transaction_bytes).await;
    print("Done");

    signed_transaction.wtxid()
}

pub async fn send_p2wpkh(
    btc_network: BitcoinNetwork,
    derivation_path: Vec<Vec<u8>>,
    key_name: String,
    dst_address: String,
    amount: Satoshi,
) -> [u8;32] {
    // Get fee percentiles from previous transactions to estimate our own fee.
    let fee_percentiles = bitcoin_api::get_current_fee_percentiles(btc_network).await;

    let fee_per_byte =
    if fee_percentiles.is_empty() {
        // There are no fee percentiles. This case can only happen on a regtest
        // network where there are no non-coinbase transactions. In this case,
        // we use a default of 2000 millisatoshis/byte (i.e. 2 satoshi/byte)
        2000
    } else {
        // Choose the 50th percentile for sending fees.
        fee_percentiles[50]
    };

    // Fetch our public key, address, and UTXOs.
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;

    let own_address = public_key_to_p2wpkh(&own_public_key);

    print("Fetching UTXOs...");
    // Note that pagination may have to be used to get all UTXOs for the given address.
    // For the sake of simplicity, it is assumed here that the `utxo` field in the response
    // contains all UTXOs.
    let own_utxos: Vec<Utxo> = bitcoin_api::get_utxos(btc_network, own_address.clone())
        .await
        .utxos;

    let network: Network = match btc_network {
        BitcoinNetwork::Mainnet => Network::Mainnet,
        BitcoinNetwork::Testnet => Network::Testnet,
        BitcoinNetwork::Regtest => Network::Regtest,
    };
    let own_address = BitcoinAddress::parse(&own_address, network).unwrap();
    let dst_address = BitcoinAddress::parse(&dst_address, network).unwrap();
    
    // @dev Build the transaction that sends `amount` to the destination address.
    let transaction = build_unsigned_transaction(
        &own_public_key,
        own_address,
        &own_utxos,
        dst_address,
        amount,
        fee_per_byte,
    )
    .await;

    // Sign the transaction.
    let signed_transaction: SignedTransaction = sign_transaction_p2wpkh(
        &own_public_key,
        transaction,
        key_name,
        derivation_path,
    )
    .await.unwrap();

    print("Sending transaction...");
    let signed_transaction_bytes = signed_transaction.serialize();
    bitcoin_api::send_transaction(btc_network, signed_transaction_bytes).await;
    print("Done");

    signed_transaction.wtxid()
}

pub async fn test_utxos(
    btc_network: BitcoinNetwork,
    derivation_path: Vec<Vec<u8>>,
    key_name: String,
) -> Vec<String> {
    // Fetch our public key, address, and UTXOs.
    let own_public_key =
        ecdsa_api::ecdsa_public_key(key_name.clone(), derivation_path.clone()).await;
 
    let own_address = public_key_to_p2wpkh(&own_public_key);

    print("Fetching UTXOs...");
    // Note that pagination may have to be used to get all UTXOs for the given address.
    // For the sake of simplicity, it is assumed here that the `utxo` field in the response
    // contains all UTXOs.
    let own_utxos: Vec<Utxo> = bitcoin_api::get_utxos(btc_network, own_address.clone())
        .await
        .utxos;

    let mut res: Vec<String> = vec![];
    for utxo in own_utxos {
        let id = utxo.outpoint.txid;

        let txid_bytes = id.iter().rev().map(|n| *n as u8).collect::<Vec<u8>>();

        res.push(hex::encode(txid_bytes))
    }
    res
}

// Builds a transaction to send the given `amount` of satoshis to the
// destination address.
async fn build_transaction(
    own_public_key: &[u8],
    own_address: &Address,
    own_utxos: &[Utxo],
    dst_address: &Address,
    amount: Satoshi,
    fee_per_byte: MillisatoshiPerByte,
) -> Transaction {
    // We have a chicken-and-egg problem where we need to know the length
    // of the transaction in order to compute its proper fee, but we need
    // to know the proper fee in order to figure out the inputs needed for
    // the transaction.
    //
    // We solve this problem iteratively. We start with a fee of zero, build
    // and sign a transaction, see what its size is, and then update the fee,
    // rebuild the transaction, until the fee is set to the correct amount.
    print("Building transaction...");
    let mut total_fee = 0;
    loop {
        let transaction =
            build_transaction_with_fee(own_utxos, own_address, dst_address, amount, total_fee)
                .expect("Error building transaction.");

        // Sign the transaction. In this case, we only care about the size
        // of the signed transaction, so we use a mock signer here for efficiency.
        let signed_transaction = sign_transaction_p2pkh(
            own_public_key,
            own_address,
            transaction.clone(),
            String::from(""), // mock key name
            vec![],           // mock derivation path
            mock_signer,
        )
        .await;

        let signed_tx_bytes_len = signed_transaction.serialize().len() as u64;

        if (signed_tx_bytes_len * fee_per_byte) / 1000 == total_fee {
            print(&format!("Transaction built with fee {}.", total_fee));
            return transaction;
        } else {
            total_fee = (signed_tx_bytes_len * fee_per_byte) / 1000;
        }
    }
}

async fn build_unsigned_transaction(
    own_public_key: &[u8],
    own_address: BitcoinAddress,
    own_utxos: &[Utxo],
    dst_address: BitcoinAddress,
    amount: Satoshi,
    fee_per_byte: MillisatoshiPerByte,
) -> UnsignedTransaction {
    // We have a chicken-and-egg problem where we need to know the length
    // of the transaction in order to compute its proper fee, but we need
    // to know the proper fee in order to figure out the inputs needed for
    // the transaction.
    //
    // We solve this problem iteratively. We start with a fee of zero, build
    // and sign a transaction, see what its size is, and then update the fee,
    // rebuild the transaction, until the fee is set to the correct amount.
    print("Building transaction...");
    let mut total_fee = 0;
    loop {
        let transaction =
            build_unsigned_tx_with_fee(own_utxos, own_address.clone(), dst_address.clone(), amount, total_fee)
                .expect("Error building transaction.");

        // Sign the transaction. In this case, we only care about the size
        // of the signed transaction, so we use a mock signer here for efficiency.
        let signed_transaction = sign_transaction_p2wpkh(
            own_public_key,
            transaction.clone(),
            String::from(""), // mock key name
            vec![],           // mock derivation path
        )
        .await.unwrap();

        let signed_tx_bytes_len = signed_transaction.serialize().len() as u64;

        if (signed_tx_bytes_len * fee_per_byte) / 1000 == total_fee {
            print(&format!("Transaction built with fee {}.", total_fee));
            return transaction;
        } else {
            total_fee = (signed_tx_bytes_len * fee_per_byte) / 1000;
        }
    }
}

fn build_transaction_with_fee(
    own_utxos: &[Utxo],
    own_address: &Address,
    dst_address: &Address,
    amount: u64,
    fee: u64,
) -> Result<Transaction, String> {
    // Assume that any amount below this threshold is dust.
    //@review (mainnet)
    const DUST_THRESHOLD: u64 = 0;

    // Select which UTXOs to spend. We naively spend the oldest available UTXOs,
    // even if they were previously spent in a transaction. This isn't a
    // problem as long as at most one transaction is created per block and
    // we're using min_confirmations of 1.
    let mut utxos_to_spend = vec![];
    let mut total_spent = 0;
    for utxo in own_utxos.iter().rev() {
        total_spent += utxo.value;
        utxos_to_spend.push(utxo);
        if total_spent >= amount + fee {
            // We have enough inputs to cover the amount we want to spend.
            break;
        }
    }

    if total_spent < amount + fee {
        return Err(format!(
            "Insufficient balance: {}, trying to transfer {} satoshi with fee {}",
            total_spent, amount, fee
        ));
    }

    let inputs: Vec<TxIn> = utxos_to_spend
        .into_iter()
        .map(|utxo| TxIn {
            previous_output: OutPoint {
                txid: Txid::from_hash(Hash::from_slice(&utxo.outpoint.txid).unwrap()),
                vout: utxo.outpoint.vout,
            },
            sequence: 0xffffffff,
            witness: Witness::new(),
            script_sig: Script::new(),
        })
        .collect();

    let mut outputs = vec![TxOut {
        script_pubkey: dst_address.script_pubkey(),
        value: amount,
    }];

    let remaining_amount = total_spent - amount - fee;

    if remaining_amount >= DUST_THRESHOLD {
        outputs.push(TxOut {
            script_pubkey: own_address.script_pubkey(),
            value: remaining_amount,
        });
    }

    Ok(Transaction {
        input: inputs,
        output: outputs,
        lock_time: 0,
        version: 1,
    })
}

fn vec_to_txid(vec: Vec<u8>) -> ic_ckbtc_minter_tyron::tx::Txid {
    let bytes: [u8; 32] = std::convert::TryInto::try_into(vec).expect("Can't convert to [u8; 32]");
    bytes.into()
}

fn build_unsigned_tx_with_fee(
    own_utxos: &[Utxo],
    own_address: BitcoinAddress,
    dst_address: BitcoinAddress,
    amount: u64,
    fee: u64,
) -> Result<UnsignedTransaction, String> {
    // Assume that any amount below this threshold is dust.
    //@review (mainnet)
    const DUST_THRESHOLD: u64 = 0;

    // Select which UTXOs to spend. We naively spend the oldest available UTXOs,
    // even if they were previously spent in a transaction. This isn't a
    // problem as long as at most one transaction is created per block and
    // we're using min_confirmations of 1.
    let mut utxos_to_spend = vec![];
    let mut total_spent = 0;
    for utxo in own_utxos.iter().rev() {
        total_spent += utxo.value;
        utxos_to_spend.push(utxo);
        if total_spent >= amount + fee {
            // We have enough inputs to cover the amount we want to spend.
            break;
        }
    }

    if total_spent < amount + fee {
        return Err(format!(
            "Insufficient balance: {}, trying to transfer {} satoshi with fee {}",
            total_spent, amount, fee
        ));
    }

    let inputs: Vec<UnsignedInput> = utxos_to_spend
        .into_iter()
        .map(|utxo| UnsignedInput {
            previous_output: ic_ckbtc_minter_tyron::tx::OutPoint {
                txid: vec_to_txid(utxo.outpoint.txid.clone()),
                vout: utxo.outpoint.vout,
            },
            value: utxo.value,
            sequence: 0xffffffff,
        })
        .collect();

    let mut outputs: Vec<ic_ckbtc_minter_tyron::tx::TxOut> = vec![ic_ckbtc_minter_tyron::tx::TxOut {
        address: dst_address,
        value: amount,
    }];

    let remaining_amount = total_spent - amount - fee;

    if remaining_amount >= DUST_THRESHOLD {
        outputs.push(ic_ckbtc_minter_tyron::tx::TxOut {
            address: own_address,
            value: remaining_amount,
        });
    }
    Ok(UnsignedTransaction {
        inputs,
        outputs,
        lock_time: 0,
    })
}

async fn build_unsigned_mint(
    own_public_key: &[u8],
    own_address: BitcoinAddress,
    select_utxo: Utxo,
    // fee_utxos: &[ic_btc_interface::Utxo],
    fee_utxos: &[Utxo],
    dst_address: BitcoinAddress,
    fee_per_byte: MillisatoshiPerByte,
) -> UnsignedTransaction {
    // We have a chicken-and-egg problem where we need to know the length
    // of the transaction in order to compute its proper fee, but we need
    // to know the proper fee in order to figure out the UTXO inputs needed for
    // the transaction.
    //
    // We solve this problem iteratively. We start with a fee of zero, build
    // and sign a transaction, see what its size is, and then update the fee,
    // rebuild the transaction, until the fee is set to the correct amount.
    print("Building transaction...");
    let mut total_fee = 0;
    loop {
        let transaction =
            build_unsigned_mint_with_fee(select_utxo.clone(), fee_utxos, own_address.clone(), dst_address.clone(), total_fee)
                .expect("Error building transaction.");

        // Sign the transaction. In this case, we only care about the size
        // of the signed transaction, so we use a mock signer here for efficiency.
        let signed_transaction = sign_transaction_p2wpkh(
            own_public_key,
            transaction.clone(),
            String::from(""), // mock key name
            vec![],           // mock derivation path
        )
        .await.unwrap();

        let signed_tx_bytes_len = signed_transaction.serialize().len() as u64;

        if (signed_tx_bytes_len * fee_per_byte) / 1000 == total_fee {
            print(&format!("Transaction built with fee {}.", total_fee));
            return transaction;
        } else {
            total_fee = (signed_tx_bytes_len * fee_per_byte) / 1000;
        }
    }
}

fn build_unsigned_mint_with_fee(
    select_utxo: Utxo,
    // fee_utxos: &[ic_btc_interface::Utxo],
    fee_utxos: &[Utxo],
    own_address: BitcoinAddress,
    dst_address: BitcoinAddress,
    fee: u64,
) -> Result<UnsignedTransaction, String> {
    // Select which UTXOs to spend. We naively spend the oldest available UTXOs,
    // even if they were previously spent in a transaction. This isn't a
    // problem as long as at most one transaction is created per block and
    // we're using min_confirmations of 1.
    let mut utxos_to_spend = vec![];
    let mut to_spend_in_fees = 0;
    for utxo in fee_utxos.iter().rev() {
        to_spend_in_fees += utxo.value;
        utxos_to_spend.push(utxo);
        if to_spend_in_fees >= fee {
            // We have enough inputs to cover the amount we want to spend.
            break;
        }
    }

    if to_spend_in_fees < fee {
        return Err(format!(
            "Insufficient balance: {}, to cover fee of {}",
            to_spend_in_fees, fee
        ));
    }

    let mut inputs: Vec<UnsignedInput> = vec![];

    inputs.push(UnsignedInput {
        previous_output: ic_ckbtc_minter_tyron::tx::OutPoint {
            txid: vec_to_txid(select_utxo.outpoint.txid),
            vout: select_utxo.outpoint.vout,
        },
        value: select_utxo.value,
        sequence: 0xffffffff,
    });

    let mut utxos_for_fee: Vec<UnsignedInput> = utxos_to_spend
        .into_iter()
        .map(|utxo| UnsignedInput {
            previous_output: ic_ckbtc_minter_tyron::tx::OutPoint {
                // txid: utxo.outpoint.txid,
                txid: vec_to_txid(utxo.outpoint.txid.clone()),
                vout: utxo.outpoint.vout,
            },
            value: utxo.value,
            sequence: 0xffffffff,
        })
        .collect();

    inputs.append(&mut utxos_for_fee);

    let mut outputs: Vec<ic_ckbtc_minter_tyron::tx::TxOut> = vec![ic_ckbtc_minter_tyron::tx::TxOut {
        address: dst_address,
        value: select_utxo.value,
    }];
    
    let remaining_amount = to_spend_in_fees - fee;

    outputs.push(ic_ckbtc_minter_tyron::tx::TxOut {
        address: own_address,
        value: remaining_amount,
    });
    
    Ok(UnsignedTransaction {
        inputs,
        outputs,
        lock_time: 0,
    })
}

// Sign a bitcoin transaction.
//
// IMPORTANT: This method is for testnet purposes only and it only
// supports signing transactions if:
// 1. All the inputs are referencing outpoints that are owned by `own_address`.
// 2.A `own_address` is a P2PKH address.
async fn sign_transaction_p2pkh<SignFun, Fut>(
    own_public_key: &[u8],
    own_address: &Address,
    mut transaction: Transaction,
    key_name: String,
    derivation_path: Vec<Vec<u8>>,
    signer: SignFun,
) -> Transaction
where
    SignFun: Fn(String, Vec<Vec<u8>>, Vec<u8>) -> Fut,
    Fut: std::future::Future<Output = Vec<u8>>,
{
    // Verify that our own address is P2PKH.
    assert_eq!(
        own_address.address_type(),
        Some(AddressType::P2pkh),
        "This function supports signing p2pkh addresses only."
    );

    let txclone = transaction.clone();
    for (index, input) in transaction.input.iter_mut().enumerate() {
        let sighash =
            txclone.signature_hash(index, &own_address.script_pubkey(), SIG_HASH_TYPE.to_u32());

        let signature = signer(key_name.clone(), derivation_path.clone(), sighash.to_vec()).await;

        // Convert signature to DER.
        let der_signature = sec1_to_der(signature);

        let mut sig_with_hashtype = der_signature;
        sig_with_hashtype.push(SIG_HASH_TYPE.to_u32() as u8);
        input.script_sig = Builder::new()
            .push_slice(sig_with_hashtype.as_slice())
            .push_slice(own_public_key)
            .into_script();
        input.witness.clear();
    }

    transaction
}

fn convert_to_bytebufs(data: Vec<Vec<u8>>) -> Vec<ByteBuf> {
    data.into_iter()
        .map(|inner| ByteBuf::from(inner))
        .collect()
}

// 2.B `own_address` is a P2WPKH address.
async fn sign_transaction_p2wpkh(
    own_public_key: &[u8],
    unsigned_tx: UnsignedTransaction,
    key_name: String,
    derivation_path: Vec<Vec<u8>>
) -> Result<SignedTransaction, CallError> {
    // Verify that our own address is P2WPKH. @review (test)
    // assert_eq!(
    //     own_address.address_type(),
    //     Some(AddressType::P2wpkh),
    //     "This function supports signing p2wpkh addresses only."
    // );
    let mut signed_inputs = Vec::with_capacity(unsigned_tx.inputs.len());
    
    let sighasher = tx::TxSigHasher::new(&unsigned_tx);

    let path = convert_to_bytebufs(derivation_path);
 
    let key_name = "test_key_1".to_string(); //@review (sign) key_name should not be empty

    for input in &unsigned_tx.inputs {
        let outpoint = &input.previous_output;

        let pubkey = ByteBuf::from(own_public_key);
        let pkhash = tx::hash160(&pubkey);

        let sighash = sighasher.sighash(&input, &pkhash);

        let sec1_signature =
            sign_with_ecdsa(key_name.clone(), DerivationPath::new(path.clone()), sighash)
            .await;

        signed_inputs.push(SignedInput {
            signature: EncodedSignature::from_sec1(&sec1_signature.unwrap()),
            pubkey,
            previous_output: outpoint.clone(),
            sequence: input.sequence,
        });
    }

    Ok(SignedTransaction {
        inputs: signed_inputs,
        outputs: unsigned_tx.outputs,
        lock_time: unsigned_tx.lock_time,
    })
}

fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}
fn ripemd160(data: &[u8]) -> Vec<u8> {
    let mut hasher = ripemd::Ripemd160::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

// Converts a public key to a P2PKH address.
fn public_key_to_p2pkh_address(network: BitcoinNetwork, public_key: &[u8]) -> String {
    // SHA-256 & RIPEMD-160
    let result = ripemd160(&sha256(public_key));

    let prefix = match network {
        BitcoinNetwork::Testnet | BitcoinNetwork::Regtest => 0x6f,
        BitcoinNetwork::Mainnet => 0x00,
    };
    let mut data_with_prefix = vec![prefix];
    data_with_prefix.extend(result);

    let checksum = &sha256(&sha256(&data_with_prefix.clone()))[..4];

    let mut full_address = data_with_prefix;
    full_address.extend(checksum);

    bs58::encode(full_address).into_string()
}

// A mock for rubber-stamping ECDSA signatures.
async fn mock_signer(
    _key_name: String,
    _derivation_path: Vec<Vec<u8>>,
    _message_hash: Vec<u8>,
) -> Vec<u8> {
    vec![255; 64]
}

// Converts a SEC1 ECDSA signature to the DER format.
fn sec1_to_der(sec1_signature: Vec<u8>) -> Vec<u8> {
    let r: Vec<u8> = if sec1_signature[0] & 0x80 != 0 {
        // r is negative. Prepend a zero byte.
        let mut tmp = vec![0x00];
        tmp.extend(sec1_signature[..32].to_vec());
        tmp
    } else {
        // r is positive.
        sec1_signature[..32].to_vec()
    };

    let s: Vec<u8> = if sec1_signature[32] & 0x80 != 0 {
        // s is negative. Prepend a zero byte.
        let mut tmp = vec![0x00];
        tmp.extend(sec1_signature[32..].to_vec());
        tmp
    } else {
        // s is positive.
        sec1_signature[32..].to_vec()
    };

    // Convert signature to DER.
    vec![
        vec![0x30, 4 + r.len() as u8 + s.len() as u8, 0x02, r.len() as u8],
        r,
        vec![0x02, s.len() as u8],
        s,
    ]
    .into_iter()
    .flatten()
    .collect()
}
