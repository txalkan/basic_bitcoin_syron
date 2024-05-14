// @review (dep)
// The foundation of this file comes from the EVM RPC canister: https://github.com/internet-computer-protocol/evm-rpc-canister], 
// but a Candid dependency issue prevents direct import into Tyron.
// I'm also making it more blockchain agnostic.

use ic_cdk::api::management_canister::http_request::HttpHeader;

use crate::{constants::{BIS_CREDENTIAL_PATH, BIS_SIGNET_HOSTNAME, BIS_TESTNET_HOSTNAME, BTC_SIGNET_CHAIN_ID, BTC_TESTNET_CHAIN_ID}, types::{Provider, RegisterProviderArgs, ServiceProvider, StorableServiceProvider}, BIS_MAINNET_HOSTNAME, BTC_MAINNET_CHAIN_ID, TYRON_CREDENTIAL_PATH, TYRON_MAINNET_HOSTNAME, TYRON_TESTNET_HOSTNAME, UNISAT_MAINNET_HOSTNAME, UNISAT_TESTNET_HOSTNAME };

use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
#[cfg(target_arch = "wasm32")]
use ic_stable_structures::DefaultMemoryImpl;
#[cfg(not(target_arch = "wasm32"))]
use ic_stable_structures::VectorMemory;
use ic_stable_structures::{Cell, StableBTreeMap};
use std::cell::RefCell;

use crate::types::*;

#[cfg(not(target_arch = "wasm32"))]
type Memory = VirtualMemory<VectorMemory>;
#[cfg(target_arch = "wasm32")]
type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    // @review (mainnet)
    // Unstable static data: this is reset when the canister is upgraded.
    // pub static UNSTABLE_METRICS: RefCell<Metrics> = RefCell::new(Metrics::default());
    // pub static UNSTABLE_SUBNET_SIZE: RefCell<u32> = RefCell::new(NODES_IN_FIDUCIARY_SUBNET);

    // Stable static data: this is preserved when the canister is upgraded.
    #[cfg(not(target_arch = "wasm32"))]
    pub static MEMORY_MANAGER: RefCell<MemoryManager<VectorMemory>> =
        RefCell::new(MemoryManager::init(VectorMemory::new(RefCell::new(vec![]))));
    #[cfg(target_arch = "wasm32")]
    pub static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
    pub static METADATA: RefCell<Cell<Metadata, Memory>> = RefCell::new(Cell::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))),
            Metadata::default()).unwrap());
    pub static AUTH: RefCell<StableBTreeMap<PrincipalStorable, AuthSet, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))));
    pub static PROVIDERS: RefCell<StableBTreeMap<u64, Provider, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2)))));
    
    pub static SERVICE_PROVIDER_MAP: RefCell<StableBTreeMap<StorableServiceProvider, u64, Memory>> = RefCell::new(
        StableBTreeMap::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(3)))));
}

pub fn init_service_provider() {
    for provider in get_default_providers() {
        register_provider(provider);
    }
    for (service, hostname) in get_default_service_provider_hostnames() {
        let provider = find_provider(|p| {
            Some(p.chain_id) == get_known_chain_id(&service) && p.hostname == hostname
        })
        .unwrap_or_else(|| {
            panic!(
                "Missing default provider for service {:?} with hostname {:?}",
                service, hostname
            )
        });
        set_service_provider(&service, &provider);
    }
}

pub fn get_default_providers() -> Vec<RegisterProviderArgs> {
    vec![
        //@provider
        RegisterProviderArgs {
            chain_id: BTC_MAINNET_CHAIN_ID,
            hostname: TYRON_MAINNET_HOSTNAME.to_string(),
            credential_path: TYRON_CREDENTIAL_PATH.to_string(),
            credential_headers: None,
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_TESTNET_CHAIN_ID,
            hostname: TYRON_TESTNET_HOSTNAME.to_string(),
            credential_path: TYRON_CREDENTIAL_PATH.to_string(),
            credential_headers: None,
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_MAINNET_CHAIN_ID,
            hostname: UNISAT_MAINNET_HOSTNAME.to_string(),
            credential_path: "".to_string(),
            credential_headers: Some(vec![HttpHeader{
                name: "Authorization".to_string(),
                value: "Bearer @api".to_string()
            }]),
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_TESTNET_CHAIN_ID,
            hostname: UNISAT_TESTNET_HOSTNAME.to_string(),
            credential_path: "".to_string(),
            credential_headers: Some(vec![HttpHeader{
                name: "Authorization".to_string(),
                value: "Bearer @api".to_string()
            }]),
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_MAINNET_CHAIN_ID,
            hostname: BIS_MAINNET_HOSTNAME.to_string(),
            credential_path: BIS_CREDENTIAL_PATH.to_string(),
            credential_headers: Some(vec![HttpHeader{
                name: "x-api-key".to_string(),
                value: "@api better to include provider as update operation".to_string()
            }]),
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_TESTNET_CHAIN_ID,
            hostname: BIS_TESTNET_HOSTNAME.to_string(),
            credential_path: BIS_CREDENTIAL_PATH.to_string(),
            credential_headers: Some(vec![HttpHeader{
                name: "x-api-key".to_string(),
                value: "@api".to_string()
            }]),
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        },
        RegisterProviderArgs {
            chain_id: BTC_SIGNET_CHAIN_ID,
            hostname: BIS_SIGNET_HOSTNAME.to_string(),
            credential_path: BIS_CREDENTIAL_PATH.to_string(),
            credential_headers: None,
            cycles_per_call: 0,
            cycles_per_message_byte: 0,
        }
    ]
}

pub fn register_provider(args: RegisterProviderArgs) -> u64 {
    // @review (mainnet)
    // validate_hostname(&args.hostname).unwrap();
    // validate_credential_path(&args.credential_path).unwrap();
    
    let provider_id = METADATA.with(|m| {
        let mut metadata = m.borrow().get().clone();
        let id = metadata.next_provider_id;
        metadata.next_provider_id += 1;
        m.borrow_mut().set(metadata).unwrap();
        id
    });

    // @review (auth) 
    // do_deauthorize(caller, Auth::RegisterProvider);
    // log!(INFO, "[{}] Registering provider: {:?}", caller, provider_id);
    
    PROVIDERS.with(|providers| {
        providers.borrow_mut().insert(
            provider_id,
            Provider {
                provider_id,
                owner: ic_cdk::caller(),
                chain_id: args.chain_id,
                hostname: args.hostname,
                credential_path: args.credential_path,
                credential_headers: args.credential_headers.unwrap_or_default(),
                cycles_per_call: args.cycles_per_call,
                cycles_per_message_byte: args.cycles_per_message_byte,
                cycles_owed: 0,
                primary: false,
            },
        )
    });
    provider_id
}

// @provider
pub fn get_default_service_provider_hostnames() -> Vec<(ServiceProvider, &'static str)> {
    vec![
        (
            ServiceProvider::Chain(0),
            TYRON_MAINNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(11),
            TYRON_TESTNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(0),
            UNISAT_MAINNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(11),
            UNISAT_TESTNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(0),
            BIS_MAINNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(11),
            BIS_TESTNET_HOSTNAME,
        ),
        (
            ServiceProvider::Chain(111),
            BIS_SIGNET_HOSTNAME,
        ),
    ]
}

pub fn find_provider(f: impl Fn(&Provider) -> bool) -> Option<Provider> {
    PROVIDERS.with(|providers| {
        let providers = providers.borrow();
        Some(
            providers
                .iter()
                .find(|(_, p)| p.primary && f(p))
                .or_else(|| providers.iter().find(|(_, p)| f(p)))?
                .1,
        )
    })
}
pub fn get_known_chain_id(service: &ServiceProvider) -> Option<u64> {
    match service {
        // RpcService::EthMainnet(_) => Some(ETH_MAINNET_CHAIN_ID),
        // RpcService::EthSepolia(_) => Some(ETH_SEPOLIA_CHAIN_ID),
        ServiceProvider::Chain(chain_id) => Some(*chain_id),
        ServiceProvider::Provider(_) => None,
        // RpcService::Custom(_) => None,
    }
}

pub fn set_service_provider(service: &ServiceProvider, provider: &Provider) {
    // log!(
    //     INFO,
    //     "Changing service {:?} to use provider: {}",
    //     service,
    //     provider.provider_id
    // );
    if let Some(chain_id) = get_known_chain_id(service) {
        if chain_id != provider.chain_id {
            ic_cdk::trap(&format!(
                "Mismatch between service and provider chain ids ({} != {})",
                chain_id, provider.chain_id
            ))
        }
    }
    SERVICE_PROVIDER_MAP.with(|mappings| {
        mappings
            .borrow_mut()
            .insert(StorableServiceProvider::new(service), provider.provider_id);
    });
}

pub fn resolve_service_provider(service: ServiceProvider) -> Result<ResolvedServiceProvider, ProviderError> {
    Ok(match service {
        ServiceProvider::Chain(id) => ResolvedServiceProvider::Provider(PROVIDERS.with(|providers| {
            let providers = providers.borrow();
            Ok(providers
                .iter()
                .find(|(_, p)| p.primary && p.chain_id == id)
                .or_else(|| providers.iter().find(|(_, p)| p.chain_id == id))
                .ok_or(ProviderError::ProviderNotFound)?
                .1)
        })?),
        ServiceProvider::Provider(id) => ResolvedServiceProvider::Provider({
            PROVIDERS.with(|providers| {
                providers
                    .borrow()
                    .get(&id)
                    .ok_or(ProviderError::ProviderNotFound)
            })?
        }),
    })
}
