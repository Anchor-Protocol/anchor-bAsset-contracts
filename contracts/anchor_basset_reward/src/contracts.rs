use crate::init::RewardInitMsg;
use crate::msg::{HandleMsg, QueryMsg, TokenInfoResponse};
use crate::state::{
    config, config_read, index_read, index_store, pending_reward_read, pending_reward_store,
    read_holder_map, store_holder_map, Config, Index,
};
use cosmwasm_std::{
    coins, from_binary, log, to_binary, Api, BankMsg, Binary, CosmosMsg, Decimal, Env, Extern,
    HandleResponse, HumanAddr, InitResponse, Querier, QueryRequest, StdError, StdResult, Storage,
    Uint128, WasmMsg, WasmQuery,
};

use cosmwasm_storage::to_length_prefixed;
use std::ops::Add;
use terra_cosmwasm::{create_swap_msg, TerraMsgWrapper};

const SWAP_DENOM: &str = "uusd";

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    _env: Env,
    msg: RewardInitMsg,
) -> StdResult<InitResponse> {
    let conf = Config { owner: msg.owner };
    config(&mut deps.storage).save(&conf)?;

    let index = Index {
        global_index: Decimal::zero(),
    };
    index_store(&mut deps.storage).save(&index)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    if let Some(init_hook) = msg.init_hook {
        messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: init_hook.contract_addr,
            msg: init_hook.msg,
            send: vec![],
        }));
    }

    Ok(InitResponse {
        messages,
        log: vec![],
    })
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    match msg {
        HandleMsg::SendReward {} => handle_send_reward(deps, env),
        HandleMsg::Swap {} => handle_swap(deps, env),
        HandleMsg::UpdateGlobalIndex { past_balance } => {
            handle_global_index(deps, env, past_balance)
        }
        HandleMsg::UpdateUserIndex { address } => handle_update_index(deps, env, address),
    }
}

pub fn handle_send_reward<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    //check whether the gov contract has sent this
    let conf = config_read(&deps.storage).load()?;

    let owner_human = deps.api.human_address(&conf.owner)?;
    let sender = env.message.sender;
    let sender_raw = deps.api.canonical_address(&sender)?;
    //calculate the reward
    let recv_index = read_holder_map(&deps.storage, sender.clone())?;
    let global_index = index_read(&deps.storage).load()?.global_index;

    let balance = query_balance(&deps, &sender, owner_human).unwrap();
    let reward = calculate_reward(global_index, recv_index, balance)?;

    let pending_reward = pending_reward_read(&deps.storage).load(sender_raw.as_slice())?;

    //set the pending reward to zero
    pending_reward_store(&mut deps.storage)
        .update(sender_raw.as_slice(), |_| Ok(Uint128::zero()))?;

    let final_reward = reward + pending_reward;

    let contr_addr = env.contract.address;
    let msgs = vec![BankMsg::Send {
        from_address: contr_addr.clone(),
        to_address: sender,
        amount: coins(Uint128::u128(&final_reward), SWAP_DENOM),
    }
    .into()];

    let res = HandleResponse {
        messages: msgs,
        log: vec![
            log("action", "send_reward"),
            log("from", contr_addr),
            log("amount", reward),
        ],
        data: None,
    };
    Ok(res)
}

// calculate the reward based on the sender's index and the global index.
pub fn calculate_reward(
    general_index: Decimal,
    user_index: Decimal,
    user_balance: Uint128,
) -> StdResult<Uint128> {
    (general_index * user_balance) - (user_index * user_balance)
}

pub fn handle_swap<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    let contr_addr = env.contract.address.clone();
    let balance = deps.querier.query_all_balances(env.contract.address)?;
    let mut msgs: Vec<CosmosMsg<TerraMsgWrapper>> = Vec::new();

    for coin in balance {
        msgs.push(create_swap_msg(
            contr_addr.clone(),
            coin,
            SWAP_DENOM.to_string(),
        ));
    }

    let res = HandleResponse {
        messages: msgs,
        log: vec![log("action", "swap"), log("from", contr_addr)],
        data: None,
    };
    Ok(res)
}

pub fn handle_global_index<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    past_balance: Uint128,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    //TODO: Do we need to consider tax here?
    //check who sent this
    let config = config_read(&deps.storage).load()?;
    let owner_raw = deps.api.human_address(&config.owner)?;
    if env.message.sender != owner_raw {
        return Err(StdError::generic_err("Unauthorized"));
    }

    //check the balance of the reward contract.
    let balance = deps
        .querier
        .query_balance(env.contract.address, SWAP_DENOM)
        .unwrap();

    let total_supply = {
        let res = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Raw {
            contract_addr: owner_raw,
            key: Binary::from(to_length_prefixed(b"token_info")),
        }))?;
        let token_info: TokenInfoResponse = from_binary(&res)?;
        token_info.total_supply
    };

    let claimed_reward = (balance.amount - past_balance)?;
    //update the global index
    index_store(&mut deps.storage).update(|mut index| {
        index.global_index = index
            .global_index
            .add(Decimal::from_ratio(claimed_reward, total_supply.u128()));
        Ok(index)
    })?;

    let res = HandleResponse {
        messages: vec![],
        log: vec![log("action", "update_index")],
        data: None,
    };

    Ok(res)
}

pub fn handle_update_index<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    address: HumanAddr,
) -> StdResult<HandleResponse<TerraMsgWrapper>> {
    let config = config_read(&deps.storage).load()?;
    let owner_human = deps.api.human_address(&config.owner)?;
    let address_raw = deps.api.canonical_address(&address)?;

    let sender = env.message.sender;
    if sender != owner_human {
        return Err(StdError::generic_err("Unauthorized"));
    }

    let global_index = index_read(&deps.storage).load()?.global_index;

    if read_holder_map(&deps.storage, address.clone()).is_err() {
        //save the holder map
        store_holder_map(&mut deps.storage, address, global_index)?;
    } else {
        //calculate the reward
        let recv_index = read_holder_map(&deps.storage, sender.clone())?;
        let global_index = index_read(&deps.storage).load()?.global_index;

        let balance = query_balance(&deps, &sender, owner_human).unwrap();
        let reward = calculate_reward(global_index, recv_index, balance)?;

        //store the reward
        pending_reward_store(&mut deps.storage)
            .update(address_raw.as_slice(), |balance: Option<Uint128>| {
                Ok(balance.unwrap_or_default() + reward)
            })?;

        store_holder_map(&mut deps.storage, address, global_index)?;
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![log("action", "register_holder")],
        data: None,
    };

    Ok(res)
}

#[inline]
fn concat(namespace: &[u8], key: &[u8]) -> Vec<u8> {
    let mut k = namespace.to_vec();
    k.extend_from_slice(key);
    k
}

pub fn query_balance<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    address: &HumanAddr,
    contract_addr: HumanAddr,
) -> StdResult<Uint128> {
    let res: Binary = deps
        .querier
        .query(&QueryRequest::Wasm(WasmQuery::Raw {
            contract_addr,
            key: Binary::from(concat(
                &to_length_prefixed(b"balance").to_vec(),
                (deps.api.canonical_address(&address)?).as_slice(),
            )),
        }))
        .unwrap_or_else(|_| to_binary(&Uint128::zero()).unwrap());

    let bal: Uint128 = from_binary(&res)?;
    Ok(bal)
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
    _msg: QueryMsg,
) -> StdResult<Binary> {
    unimplemented!()
}
