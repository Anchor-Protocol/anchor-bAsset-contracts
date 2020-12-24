use crate::state::{
    read_config, read_validators, remove_white_validators, store_config, store_msg_status,
    store_parameters, store_white_validators, Parameters,
};
use anchor_basset_reward::msg::HandleMsg::UpdateRewardDenom;
use cosmwasm_std::{
    log, to_binary, Api, CosmosMsg, Decimal, Env, Extern, HandleResponse, HumanAddr, Querier,
    StakingMsg, StdError, StdResult, Storage, WasmMsg,
};
use hub_querier::{Deactivated, HandleMsg, Registration};
use rand::{Rng, SeedableRng, XorShiftRng};

/// Update general parameters
/// Only creator/owner is allowed to execute
#[allow(clippy::too_many_arguments)]
pub fn handle_update_params<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    epoch_period: Option<u64>,
    underlying_coin_denom: Option<String>,
    unbonding_period: Option<u64>,
    peg_recovery_fee: Option<Decimal>,
    er_threshold: Option<Decimal>,
    reward_denom: Option<String>,
) -> StdResult<HandleResponse> {
    // only owner can send this message.
    let config = read_config(&deps.storage).load()?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if sender_raw != config.creator {
        return Err(StdError::unauthorized());
    }

    let params: Parameters = store_parameters(&mut deps.storage).load()?;
    let new_params = Parameters {
        epoch_period: epoch_period.unwrap_or(params.epoch_period),
        underlying_coin_denom: underlying_coin_denom.unwrap_or(params.underlying_coin_denom),
        unbonding_period: unbonding_period.unwrap_or(params.unbonding_period),
        peg_recovery_fee: peg_recovery_fee.unwrap_or(params.peg_recovery_fee),
        er_threshold: er_threshold.unwrap_or(params.er_threshold),
        reward_denom: reward_denom.clone().unwrap_or(params.reward_denom),
    };

    let mut msgs: Vec<CosmosMsg> = vec![];
    if let Some(denom) = reward_denom {
        let reward_addr = deps.api.human_address(
            &config
                .reward_contract
                .expect("the reward contract must have been registered"),
        )?;

        // send update denom to the reward contract
        let set_swap = UpdateRewardDenom {
            reward_denom: Some(denom),
        };

        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: reward_addr,
            msg: to_binary(&set_swap)?,
            send: vec![],
        }));
    }

    store_parameters(&mut deps.storage).save(&new_params)?;
    let res = HandleResponse {
        messages: msgs,
        log: vec![log("action", "update_params")],
        data: None,
    };
    Ok(res)
}

/// Deactivate messages. Only unbond and slashing is supported.
/// Only creator/owner is allowed to execute
pub fn handle_deactivate<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: Deactivated,
) -> StdResult<HandleResponse> {
    // only owner must be able to send this message.
    let config = read_config(&deps.storage).load()?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if sender_raw != config.creator {
        return Err(StdError::unauthorized());
    }

    // store the status of slashing and unbond
    match msg {
        Deactivated::Slashing => {
            store_msg_status(&mut deps.storage).update(|mut msg_status| {
                msg_status.slashing = Some(msg);
                Ok(msg_status)
            })?;
        }
        Deactivated::Unbond => {
            store_msg_status(&mut deps.storage).update(|mut msg_status| {
                msg_status.unbond = Some(msg);
                Ok(msg_status)
            })?;
        }
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![log("action", "deactivate_msg")],
        data: None,
    };
    Ok(res)
}

/// Update the config. Update the owner, reward and token contracts.
/// Only creator/owner is allowed to execute
pub fn handle_update_config<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    owner: Option<HumanAddr>,
    reward_contract: Option<HumanAddr>,
    token_contract: Option<HumanAddr>,
) -> StdResult<HandleResponse> {
    // only owner must be able to send this message.
    let conf = read_config(&deps.storage).load()?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if sender_raw != conf.creator {
        return Err(StdError::unauthorized());
    }

    if let Some(o) = owner {
        let owner_raw = deps.api.canonical_address(&o)?;

        store_config(&mut deps.storage).update(|mut last_config| {
            last_config.creator = owner_raw;
            Ok(last_config)
        })?;
    }
    if let Some(reward) = reward_contract {
        let reward_raw = deps.api.canonical_address(&reward)?;

        store_config(&mut deps.storage).update(|mut last_config| {
            last_config.reward_contract = Some(reward_raw);
            Ok(last_config)
        })?;
    }

    if let Some(token) = token_contract {
        let token_raw = deps.api.canonical_address(&token)?;

        store_config(&mut deps.storage).update(|mut last_config| {
            last_config.token_contract = Some(token_raw);
            Ok(last_config)
        })?;
    }

    let res = HandleResponse {
        messages: vec![],
        log: vec![log("action", "change_the_owner")],
        data: None,
    };
    Ok(res)
}

/// Register subcontracts, reward and token contracts.
/// Only creator/owner is allowed to execute
pub fn handle_register_contracts<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    contract: Registration,
    contract_address: HumanAddr,
) -> StdResult<HandleResponse> {
    // only owner must be able to send this message.
    let conf = read_config(&deps.storage).load()?;
    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if sender_raw != conf.creator {
        return Err(StdError::unauthorized());
    }

    let raw_contract_addr = deps.api.canonical_address(&contract_address)?;
    let mut messages: Vec<CosmosMsg> = vec![];

    // if contract is reward, store the contract address for reward in config.
    // if contract is token, store the contract address for token in config.
    match contract {
        Registration::Reward => {
            if conf.reward_contract.is_some() {
                return Err(StdError::generic_err(
                    "The reward contract is already registered",
                ));
            }
            store_config(&mut deps.storage).update(|mut last_config| {
                last_config.reward_contract = Some(raw_contract_addr.clone());
                Ok(last_config)
            })?;
            let msg: CosmosMsg = CosmosMsg::Staking(StakingMsg::Withdraw {
                validator: HumanAddr::default(),
                recipient: Some(deps.api.human_address(&raw_contract_addr)?),
            });
            messages.push(msg);
        }
        Registration::Token => {
            store_config(&mut deps.storage).update(|mut last_config| {
                if last_config.token_contract.is_some() {
                    return Err(StdError::generic_err(
                        "The token contract is already registered",
                    ));
                }
                last_config.token_contract = Some(raw_contract_addr.clone());
                Ok(last_config)
            })?;
        }
    }
    let res = HandleResponse {
        messages,
        log: vec![
            log("action", "register"),
            log("sub_contract", contract_address),
        ],
        data: None,
    };
    Ok(res)
}

/// Register a white listed validator.
/// Only creator/owner is allowed to execute
pub fn handle_register_validator<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    validator: HumanAddr,
) -> StdResult<HandleResponse> {
    let hub_conf = read_config(&deps.storage).load()?;

    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if hub_conf.creator != sender_raw {
        return Err(StdError::unauthorized());
    }

    // given validator must be first a validator in the system.
    let exists = deps
        .querier
        .query_validators()?
        .iter()
        .any(|val| val.address == validator);
    if !exists {
        return Err(StdError::generic_err("Invalid validator"));
    }

    store_white_validators(&mut deps.storage, validator.clone())?;
    let res = HandleResponse {
        messages: vec![],
        log: vec![
            log("action", "register_validator"),
            log("validator", validator),
        ],
        data: None,
    };
    Ok(res)
}

/// Deregister a previously-whitelisted validator.
/// Only creator/owner is allowed to execute
pub fn handle_deregister_validator<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    validator: HumanAddr,
) -> StdResult<HandleResponse> {
    let token = read_config(&deps.storage).load()?;

    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if token.creator != sender_raw {
        return Err(StdError::generic_err(
            "Only the creator can send this message",
        ));
    }
    remove_white_validators(&mut deps.storage, validator.clone())?;

    let query = deps
        .querier
        .query_delegation(env.contract.address.clone(), validator.clone())?
        .unwrap();
    let delegated_amount = query.amount;

    let mut messages: Vec<CosmosMsg> = vec![];
    let validators = read_validators(&deps.storage)?;

    // redelegate the amount to a random validator.
    // another validator must be randomly chose to redelgate to
    let block_height = env.block.height;
    let mut rng = XorShiftRng::seed_from_u64(block_height);
    let random_index = rng.gen_range(0, validators.len());
    let replaced_val = HumanAddr::from(validators.get(random_index).unwrap());
    messages.push(CosmosMsg::Staking(StakingMsg::Redelegate {
        src_validator: validator.clone(),
        dst_validator: replaced_val,
        amount: delegated_amount,
    }));

    let msg = HandleMsg::UpdateGlobalIndex {};
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address,
        msg: to_binary(&msg)?,
        send: vec![],
    }));

    let res = HandleResponse {
        messages,
        log: vec![
            log("action", "de_register_validator"),
            log("validator", validator),
        ],
        data: None,
    };
    Ok(res)
}
