use cosmwasm_std::{
    from_binary, Api, Binary, CanonicalAddr, Extern, HumanAddr, Querier, QueryRequest, StdResult,
    Storage, WasmQuery,
};
use cosmwasm_storage::to_length_prefixed;
use gov_courier::PoolInfo;

pub fn query_token_contract<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    contract_addr: HumanAddr,
) -> StdResult<CanonicalAddr> {
    let res: Binary = deps
        .querier
        .query(&QueryRequest::Wasm(WasmQuery::Raw {
            contract_addr,
            key: Binary::from(to_length_prefixed(b"pool_info")),
        }))
        .unwrap();

    let pool_info: PoolInfo = from_binary(&res)?;
    Ok(pool_info.token_account)
}
