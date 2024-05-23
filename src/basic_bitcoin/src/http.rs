use ic_cdk::api::management_canister::http_request::{CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs, TransformContext};
use ic_ckbtc_minter_tyron::updates::update_balance::UpdateBalanceError;
use serde_json::Value;
use crate::{resolve_service_provider, HttpOutcallError, ResolvedServiceProvider, ServiceError, ServiceProvider, ServiceResult, CONTENT_TYPE_HEADER, CONTENT_TYPE_VALUE };
use num_traits::ToPrimitive;

pub async fn call_indexer_inscription(
    provider: u64,
    txid: String,
    cycles_cost: u128
) -> Result<String, UpdateBalanceError> {
    let endpoint = match provider {
        0 | 1 =>   format!("get-unisat-inscription-info?id={}i0", txid), // @tyron
        2 | 3 =>   format!("v1/indexer/inscription/info/{}i0", txid), // @unisat
        _ => format!("inscription/single_info_id?inscription_id={}i0", txid) // @bis
    };

    let outcall = match web3_request(ServiceProvider::Provider(provider), &endpoint, "", 2048, cycles_cost).await {
        Ok(result) => result,
        Err(err) => {
            return Err(UpdateBalanceError::GenericError{
                error_code: 333,
                error_message: format!("Failed to finalize HTTPS Outcall with error: {:?}", err),
            });
        }
    };
    Ok(outcall)
}

pub async fn call_indexer_balance(
    address: String,
    provider: u64,
    cycles_cost: u128
) -> Result<String, UpdateBalanceError> {
    let endpoint = match provider {
        0 | 1 =>   format!("get-unisat-brc20-info?id={}", address), // @tyron
        2 | 3 =>   format!("v1/indexer/address/{}/brc20/summary", address), // @unisat
        _ => format!("inscription/single_info_id?inscription_id={}i0", address) // @bis
    };

    let outcall = match web3_request(ServiceProvider::Provider(provider), &endpoint, "", 2048, cycles_cost).await {
        Ok(result) => result,
        Err(err) => {
            return Err(UpdateBalanceError::GenericError{
                error_code: 444,
                error_message: format!("Failed to finalize HTTPS Outcall with error: {:?}", err),
            });
        }
    };
    Ok(outcall)
}

pub async fn web3_request(
    service: ServiceProvider,
    endpoint: &str,
    payload: &str,
    max_response_bytes: u64,
    cycles_cost: u128
) -> Result<String, ServiceError> {
    let response = do_request(
        resolve_service_provider(service)?,
        endpoint,
        payload,
        max_response_bytes,
        cycles_cost
    )
    .await?;
    get_http_response_body(response)
}

async fn do_request(
    service: ResolvedServiceProvider,
    endpoint: &str,
    payload: &str,
    max_response_bytes: u64,
    cycles_cost: u128
) -> ServiceResult<HttpResponse> {
    let api = service.api();
    let mut request_headers = vec![HttpHeader {
        name: CONTENT_TYPE_HEADER.to_string(),
        value: CONTENT_TYPE_VALUE.to_string(),
    }];
    if let Some(headers) = api.headers {
        request_headers.extend(headers);
    }

    let mut method = HttpMethod::GET;
    if !payload.is_empty(){
        method = HttpMethod::POST;
    }

    // Match service provider to the appropriate transform function
    let transform_fn: Option<TransformContext> = match service {
        ResolvedServiceProvider::Provider(provider) => {
            match provider.provider_id {
                0 | 1 => Some(TransformContext::from_name(
                    "transform_request".to_string(),
                    vec![],
                )),
                2 | 3 => Some(TransformContext::from_name(
                    "transform_unisat_request".to_string(),
                    vec![],
                )),
                _ => None,
            }
        }
    };

    let request = CanisterHttpRequestArgument {
        url: api.url + endpoint,
        max_response_bytes: Some(max_response_bytes),
        method,
        headers: request_headers,
        body: Some(payload.as_bytes().to_vec()),
        transform: transform_fn,
    };

    match ic_cdk::api::management_canister::http_request::http_request(request, cycles_cost).await {
        Ok((response,)) => {
            Ok(response)
        }
        Err((code, message)) => {
            Err(HttpOutcallError::IcError{code, message}.into())
        }
    }
}

fn get_http_response_body(response: HttpResponse) -> Result<String, ServiceError> {
    String::from_utf8(response.body).map_err(|e| {
        HttpOutcallError::InvalidHttpJsonRpcResponse {
            status: get_http_response_status(response.status),
            body: "".to_string(),
            parsing_error: Some(format!("{e}")),
        }
        .into()
    })
}

pub fn get_http_response_status(status: candid::Nat) -> u16 {
    status.0.to_u16().unwrap_or(u16::MAX)
}

pub fn do_transform_request(mut args: TransformArgs) -> HttpResponse {
    let body = canonicalize_json(&args.response.body).unwrap_or(args.response.body.clone());
    args.response.body = body;
    
    // Remove potentially conflicting fields to reach a consensus across replicas
    args.response.headers.clear();

    args.response
}

pub fn do_transform_unisat_request(mut args: TransformArgs) -> HttpResponse {
    // if args.response.status >= 300u64 {
    //     // The error response might contain non-deterministic fields that make it impossible to reach consensus,
    //     // such as timestamps:
    //     // {"timestamp":"2023-03-01T20:35:49.416+00:00","status":403,"error":"Forbidden","message":"AccessDenied","path":"/api/kyt/v2/users/cktestbtc/transfers"}
    //     args.response.body.clear()
    // } else {
        // The response body is expected to be JSON, so let's canonicalize it to remove non-deterministic fields
        let body = canonicalize_json(&args.response.body).unwrap_or(args.response.body.clone());
        let body_json: Value = serde_json::from_slice(&body).unwrap();

        // Access the "amt" field in the "brc20" object
        let pointer = "/data";
        let syron_inscription: String = body_json.pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        args.response.body = body;//syron_inscription.into();
    // }

    // Remove potentially conflicting fields to reach a consensus across replicas
    args.response.headers.clear();

    args.response
}

pub fn do_transform_bis_request(mut args: TransformArgs) -> HttpResponse {
    // if args.response.status >= 300u64 {
    //     // The error response might contain non-deterministic fields that make it impossible to reach consensus,
    //     // such as timestamps:
    //     // {"timestamp":"2023-03-01T20:35:49.416+00:00","status":403,"error":"Forbidden","message":"AccessDenied","path":"/api/kyt/v2/users/cktestbtc/transfers"}
    //     args.response.body.clear()
    // } else {
        // The response body is expected to be JSON, so let's canonicalize it to remove non-deterministic fields
        let body = canonicalize_json(&args.response.body).unwrap_or(args.response.body.clone());
        let body_json: Value = serde_json::from_slice(&body).unwrap();

        // Access the "amt" field in the "brc20" object
        let pointer = "/";
        let syron_inscription: String = body_json.pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        args.response.body = syron_inscription.into();
    // }

    // Remove potentially conflicting fields to reach a consensus across replicas
    args.response.headers.clear();

    args.response
}

pub fn canonicalize_json(text: &[u8]) -> Option<Vec<u8>> {
    let json = serde_json::from_slice::<Value>(text).ok()?;
    serde_json::to_vec(&json).ok()
}
