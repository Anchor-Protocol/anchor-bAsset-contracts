use cosmwasm_std::{
    coin, coins, from_binary, log, to_binary, Api, BankMsg, Binary, CosmosMsg, Decimal, Env,
    Extern, HandleResponse, HumanAddr, InitResponse, Querier, QueryRequest, StakingMsg, StdError,
    StdResult, Storage, Uint128, WasmMsg, WasmQuery,
};

use crate::math::decimal_division;
use crate::msg::{InitMsg, QueryMsg};
use crate::state::{
    config, config_read, epoch_read, get_all_delegations, get_bonded, get_burn_epochs,
    get_finished_amount, is_valid_validator, pool_info, pool_info_read, read_total_amount,
    read_valid_validators, read_validators, remove_undelegated_wait_list, remove_white_validators,
    save_epoch, set_all_delegations, set_bonded, store_total_amount, store_undelegated_wait_list,
    store_white_validators, EpochId, GovConfig, EPOCH,
};
use anchor_basset_reward::hook::InitHook;
use anchor_basset_reward::init::RewardInitMsg;
use anchor_basset_reward::msg::HandleMsg::{Swap, UpdateGlobalIndex};
use anchor_basset_token::msg::HandleMsg::{Burn, Mint};
use anchor_basset_token::msg::{TokenInitHook, TokenInitMsg};
use anchor_basset_token::state::TokenInfo;
use cosmwasm_storage::to_length_prefixed;
use cw20::{Cw20CoinHuman, Cw20ReceiveMsg, MinterResponse};
use gov_courier::PoolInfo;
use gov_courier::Registration;
use gov_courier::{Cw20HookMsg, HandleMsg};
use rand::{Rng, SeedableRng, XorShiftRng};

const LUNA: &str = "uluna";
const EPOCH_PER_UNDELEGATION_PERIOD: u64 = 2;
const DECIMALS: u8 = 6;

pub fn init<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: InitMsg,
) -> StdResult<InitResponse> {
    // validate token info
    msg.validate()?;

    // store token info
    let sender = env.message.sender;
    let sndr_raw = deps.api.canonical_address(&sender)?;
    let data = GovConfig { creator: sndr_raw };
    config(&mut deps.storage).save(&data)?;

    let pool = PoolInfo {
        exchange_rate: Decimal::one(),
        last_index_modification: env.block.time,
        ..Default::default()
    };
    pool_info(&mut deps.storage).save(&pool)?;

    //store the first epoch.
    let first_epoch = EpochId {
        epoch_id: 0,
        current_block_time: env.block.time,
    };
    save_epoch(&mut deps.storage).save(&first_epoch)?;

    //store total amount zero for the first epoc
    store_total_amount(&mut deps.storage, first_epoch.epoch_id, Uint128::zero())?;

    let mut messages: Vec<CosmosMsg> = vec![];

    let gov_address = env.contract.address;
    let token_message = to_binary(&HandleMsg::RegisterSubContracts {
        contract: Registration::Token,
    })?;

    //set minted and all_delegations to keep the record of slashing.
    set_bonded(&mut deps.storage).save(&Uint128::zero())?;
    set_all_delegations(&mut deps.storage).save(&Uint128::zero())?;

    //instantiate token contract
    messages.push(CosmosMsg::Wasm(WasmMsg::Instantiate {
        code_id: msg.token_code_id,
        msg: to_binary(&TokenInitMsg {
            name: msg.name,
            symbol: msg.symbol,
            decimals: DECIMALS,
            initial_balances: vec![Cw20CoinHuman {
                address: gov_address.clone(),
                amount: Uint128(1),
            }],
            owner: deps.api.canonical_address(&gov_address)?,
            init_hook: Some(TokenInitHook {
                msg: token_message,
                contract_addr: gov_address.clone(),
            }),
            mint: Some(MinterResponse {
                minter: gov_address.clone(),
                cap: None,
            }),
        })?,
        send: vec![],
        label: None,
    }));

    //instantiate reward contract
    let reward_message = to_binary(&HandleMsg::RegisterSubContracts {
        contract: Registration::Reward,
    })?;
    messages.push(CosmosMsg::Wasm(WasmMsg::Instantiate {
        code_id: msg.reward_code_id,
        msg: to_binary(&RewardInitMsg {
            owner: deps.api.canonical_address(&gov_address)?,
            init_hook: Some(InitHook {
                msg: reward_message,
                contract_addr: gov_address,
            }),
        })?,
        send: vec![],
        label: None,
    }));

    let res = InitResponse {
        messages,
        log: vec![],
    };
    Ok(res)
}

pub fn handle<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    msg: HandleMsg,
) -> StdResult<HandleResponse> {
    match msg {
        HandleMsg::Receive(msg) => receive_cw20(deps, env, msg),
        HandleMsg::Mint { validator } => handle_mint(deps, env, validator),
        HandleMsg::UpdateGlobalIndex {} => handle_update_global(deps, env),
        HandleMsg::FinishBurn {} => handle_finish(deps, env),
        HandleMsg::RegisterSubContracts { contract } => {
            handle_register_contracts(deps, env, contract)
        }
        HandleMsg::RegisterValidator { validator } => handle_reg_validator(deps, env, validator),
        HandleMsg::DeRegisterValidator { validator } => {
            handle_dereg_validator(deps, env, validator)
        }
        HandleMsg::ReportSlashing {} => handle_slashing(deps, env),
    }
}

pub fn receive_cw20<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    cw20_msg: Cw20ReceiveMsg,
) -> StdResult<HandleResponse> {
    let contract_addr = env.message.sender.clone();

    if let Some(msg) = cw20_msg.msg {
        match from_binary(&msg)? {
            Cw20HookMsg::InitBurn {} => {
                // only asset contract can execute this message
                let pool = pool_info_read(&deps.storage).load()?;
                if deps.api.canonical_address(&contract_addr)? != pool.token_account {
                    return Err(StdError::unauthorized());
                }
                handle_burn(deps, env, cw20_msg.amount, cw20_msg.sender)
            }
        }
    } else {
        Err(StdError::generic_err("Invalid request"))
    }
}

pub fn handle_mint<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    validator: HumanAddr,
) -> StdResult<HandleResponse> {
    let is_valid = is_valid_validator(&deps.storage, validator.clone())?;
    if !is_valid {
        return Err(StdError::generic_err("Unsupported validator"));
    }

    //Check whether the account has sent the native coin in advance.
    let payment = env
        .message
        .sent_funds
        .iter()
        .find(|x| x.denom == LUNA && x.amount > Uint128::zero())
        .ok_or_else(|| StdError::generic_err(format!("No {} tokens sent", LUNA)))?;

    let mut pool = pool_info_read(&deps.storage).load()?;
    let sender = env.message.sender.clone();

    //update the exchange rate
    if slashing(deps, env.clone()).is_ok() {
        pool.update_exchange_rate();
    }
    let amount_with_exchange_rate = decimal_division(payment.amount, pool.exchange_rate);

    //update pool_info
    pool.total_bond_amount += payment.amount;
    pool.total_issued += amount_with_exchange_rate;

    pool_info(&mut deps.storage).save(&pool)?;

    let mut messages: Vec<CosmosMsg> = vec![];

    // Issue the bluna token for sender
    let mint_msg = Mint {
        recipient: sender.clone(),
        amount: amount_with_exchange_rate,
    };
    let token_address = deps.api.human_address(&pool.token_account)?;
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: token_address,
        msg: to_binary(&mint_msg)?,
        send: vec![],
    }));

    //delegate the amount
    messages.push(CosmosMsg::Staking(StakingMsg::Delegate {
        validator,
        amount: payment.clone(),
    }));

    //add minted for slashing
    set_bonded(&mut deps.storage).update(|mut bonded| {
        bonded += payment.amount;
        Ok(bonded)
    })?;

    let res = HandleResponse {
        messages,
        log: vec![
            log("action", "mint"),
            log("from", sender),
            log("bonded", payment.amount),
            log("minted", amount_with_exchange_rate),
        ],
        data: None,
    };
    Ok(res)
}

pub fn handle_update_global<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let mut messages: Vec<CosmosMsg> = vec![];

    let pool = pool_info_read(&deps.storage).load()?;
    let reward_addr = deps.api.human_address(&pool.reward_account)?;

    //retrieve all validators
    let validators: Vec<HumanAddr> = read_validators(&deps.storage)?;

    //send withdraw message
    let mut withdraw_msgs = withdraw_all_rewards(validators);
    messages.append(&mut withdraw_msgs);

    //send Swap message to reward contract
    let swap_msg = Swap {};
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: reward_addr.clone(),
        msg: to_binary(&swap_msg).unwrap(),
        send: vec![],
    }));

    //send update GlobalIndex message to reward contract
    let global_msg = UpdateGlobalIndex {};
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: reward_addr,
        msg: to_binary(&global_msg).unwrap(),
        send: vec![],
    }));

    //update pool_info last modified
    pool_info(&mut deps.storage).update(|mut pool| {
        pool.last_index_modification = env.block.time;
        Ok(pool)
    })?;

    let res = HandleResponse {
        messages,
        log: vec![log("action", "claim_reward")],
        data: None,
    };
    Ok(res)
}

//create withdraw requests for all validators
fn withdraw_all_rewards(validators: Vec<HumanAddr>) -> Vec<CosmosMsg> {
    let mut messages: Vec<CosmosMsg> = vec![];
    for val in validators {
        let msg: CosmosMsg = CosmosMsg::Staking(StakingMsg::Withdraw {
            validator: val,
            recipient: None,
        });
        messages.push(msg)
    }
    messages
}

pub fn handle_burn<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    amount: Uint128,
    sender: HumanAddr,
) -> StdResult<HandleResponse> {
    if amount == Uint128::zero() {
        return Err(StdError::generic_err("Invalid zero amount"));
    }

    let mut epoch = epoch_read(&deps.storage).load()?;
    // get all amount that is gathered in a epoch.
    let mut undelegated_so_far = read_total_amount(&deps.storage, epoch.epoch_id)?;

    let mut messages: Vec<CosmosMsg> = vec![];

    //update pool info and calculate the new exchange rate.
    let mut exchange_rate = Decimal::zero();
    pool_info(&mut deps.storage).update(|mut pool_inf| {
        pool_inf.total_bond_amount = Uint128(pool_inf.total_bond_amount.0 - amount.0);
        pool_inf.total_issued = (pool_inf.total_issued - amount)?;
        pool_inf.update_exchange_rate();
        exchange_rate = pool_inf.exchange_rate;
        Ok(pool_inf)
    })?;

    let pool = pool_info_read(&deps.storage).load()?;

    //send Burn message to token contract
    let token_address = deps.api.human_address(&pool.token_account)?;
    let burn_msg = Burn { amount };
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: token_address,
        msg: to_binary(&burn_msg)?,
        send: vec![],
    }));

    //compute Epoch time
    let block_time = env.block.time;
    if epoch.is_epoch_passed(block_time) {
        let last_epoch = epoch.epoch_id;
        epoch.compute_current_epoch(block_time);

        //this will store the user request for the past epoch.
        store_total_amount(&mut deps.storage, epoch.epoch_id, Uint128::zero())?;

        let delegator = env.contract.address;

        // send undelegated requests
        undelegated_so_far += amount;
        let undelegated_amount = exchange_rate * undelegated_so_far;
        let all_validators = read_validators(&deps.storage).unwrap();
        let block_height = env.block.height;
        let mut undelegated_msgs = pick_validator(
            deps,
            all_validators,
            undelegated_amount,
            delegator.clone(),
            block_height,
        );
        //messages.append(&mut undelegated_msgs);
        messages.append(&mut undelegated_msgs);
        save_epoch(&mut deps.storage).save(&epoch)?;

        //update all_delegation
        let mut delegated_before = Uint128::zero();
        let all_delegations = deps.querier.query_all_delegations(delegator)?;
        for delegation in all_delegations {
            delegated_before += delegation.amount.amount
        }

        let delegated_after_burn = (delegated_before - undelegated_so_far)?;

        set_all_delegations(&mut deps.storage).save(&delegated_after_burn)?;

        //store the sender for the previous epoch
        store_undelegated_wait_list(&mut deps.storage, last_epoch, sender.clone(), amount)?;
    } else {
        undelegated_so_far += amount;

        store_undelegated_wait_list(&mut deps.storage, epoch.epoch_id, sender.clone(), amount)?;
        //store the claimed_so_far for the current epoch;
        store_total_amount(&mut deps.storage, epoch.epoch_id, undelegated_so_far)?;
    }

    let res = HandleResponse {
        messages,
        log: vec![
            log("action", "burn"),
            log("from", sender),
            log("undelegated_amount", undelegated_so_far),
        ],
        data: None,
    };
    Ok(res)
}

pub fn handle_finish<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    let sender_human = env.message.sender.clone();
    let contract_address = env.contract.address.clone();

    //check the liquidation period.
    let epoch = epoch_read(&deps.storage).load()?;
    let block_time = env.block.time;

    // get current epoch id.
    let current_epoch_id =
        compute_current_epoch(epoch.epoch_id, epoch.current_block_time, block_time);

    // Compute all of burn requests with epoch Id corresponding to 21 (can be changed to arbitrary value) days ago
    let epoch_id = get_past_epoch(current_epoch_id);

    let payable_amount = get_finished_amount(&deps.storage, epoch_id, sender_human.clone())?;

    if payable_amount.is_zero() {
        return Err(StdError::generic_err(
            "Previously requested amount is not ready yet",
        ));
    }

    //remove the previous epochs for the user
    let deprecated_epochs = get_burn_epochs(&deps.storage, sender_human.clone(), epoch_id)?;
    remove_undelegated_wait_list(&mut deps.storage, deprecated_epochs, sender_human.clone())?;

    slashing(deps, env)?;

    let exchange_rate = pool_info_read(&deps.storage).load()?.exchange_rate;
    let final_amount = payable_amount * exchange_rate;
    send_undelegated_luna(final_amount, sender_human, contract_address)
}

//return the epoch-id of the 21 days ago.
fn get_past_epoch(current_epoch: u64) -> u64 {
    if current_epoch < EPOCH_PER_UNDELEGATION_PERIOD {
        return 0;
    }
    current_epoch - EPOCH_PER_UNDELEGATION_PERIOD
}

fn send_undelegated_luna(
    amount: Uint128,
    to_address: HumanAddr,
    contract_address: HumanAddr,
) -> StdResult<HandleResponse> {
    let msgs = vec![BankMsg::Send {
        from_address: contract_address.clone(),
        to_address,
        amount: coins(Uint128::u128(&amount), LUNA),
    }
    .into()];

    let res = HandleResponse {
        messages: msgs,
        log: vec![
            log("action", "finish_burn"),
            log("from", contract_address),
            log("amount", amount),
        ],
        data: None,
    };
    Ok(res)
}

pub fn handle_register_contracts<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    contract: Registration,
) -> StdResult<HandleResponse> {
    let raw_sender = deps.api.canonical_address(&env.message.sender)?;
    let mut messages: Vec<CosmosMsg> = vec![];
    match contract {
        Registration::Reward => {
            let mut pool = pool_info_read(&deps.storage).load()?;
            if pool.is_reward_exist {
                return Err(StdError::generic_err("The request is not valid"));
            }
            pool.reward_account = raw_sender.clone();
            pool.is_reward_exist = true;
            pool_info(&mut deps.storage).save(&pool)?;

            let msg: CosmosMsg = CosmosMsg::Staking(StakingMsg::Withdraw {
                validator: HumanAddr::default(),
                recipient: Some(deps.api.human_address(&raw_sender)?),
            });
            messages.push(msg);
        }
        Registration::Token => {
            pool_info(&mut deps.storage).update(|mut pool| {
                if pool.is_token_exist {
                    return Err(StdError::generic_err("The request is not valid"));
                }
                pool.token_account = raw_sender.clone();
                pool.is_token_exist = true;
                Ok(pool)
            })?;
        }
    }
    let res = HandleResponse {
        messages,
        log: vec![log("action", "register"), log("sub_contract", raw_sender)],
        data: None,
    };
    Ok(res)
}

pub fn handle_reg_validator<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    validator: HumanAddr,
) -> StdResult<HandleResponse> {
    let gov_conf = config_read(&deps.storage).load()?;

    let sender_raw = deps.api.canonical_address(&env.message.sender)?;
    if gov_conf.creator != sender_raw {
        return Err(StdError::generic_err(
            "Only the creator can send this message",
        ));
    }

    let is_exist = deps
        .querier
        .query_validators()?
        .iter()
        .any(|val| val.address == validator);
    if !is_exist {
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

pub fn handle_dereg_validator<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
    validator: HumanAddr,
) -> StdResult<HandleResponse> {
    let token = config_read(&deps.storage).load()?;

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

    //redelegate the amount to a random validator.
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

pub fn slashing<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<()> {
    let mut amount = Uint128::zero();
    let all_delegations = get_all_delegations(&deps.storage).load()?;
    let bonded = get_bonded(&deps.storage).load()?;
    let all_delegated_amount = deps.querier.query_all_delegations(env.contract.address)?;
    for delegate in all_delegated_amount {
        if delegate.amount.denom == LUNA {
            amount += delegate.amount.amount
        }
    }
    let all_changes = (amount - all_delegations)?;
    if bonded.0 > all_changes.0 {
        pool_info(&mut deps.storage).update(|mut pool| {
            pool.total_bond_amount = amount;
            pool.update_exchange_rate();
            Ok(pool)
        })?;
    }
    set_all_delegations(&mut deps.storage).save(&amount)?;
    set_bonded(&mut deps.storage).save(&Uint128::zero())?;
    Ok(())
}

pub fn handle_slashing<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    env: Env,
) -> StdResult<HandleResponse> {
    slashing(deps, env)?;
    Ok(HandleResponse::default())
}

fn pick_validator<S: Storage, A: Api, Q: Querier>(
    deps: &mut Extern<S, A, Q>,
    validators: Vec<HumanAddr>,
    claim: Uint128,
    delegator: HumanAddr,
    block_height: u64,
) -> Vec<CosmosMsg> {
    let mut messages: Vec<CosmosMsg> = vec![];
    let mut claimed = claim;
    let mut rng = XorShiftRng::seed_from_u64(block_height);

    while claimed.0 > 0 {
        let random_index = rng.gen_range(0, validators.len());
        let validator: HumanAddr = HumanAddr::from(validators.get(random_index).unwrap());
        let val = deps
            .querier
            .query_delegation(delegator.clone(), validator.clone())
            .unwrap()
            .unwrap()
            .amount
            .amount;
        let undelegated_amount: Uint128;
        if val.0 > claimed.0 {
            undelegated_amount = claimed;
            claimed = Uint128::zero();
        } else {
            undelegated_amount = val;
            claimed = Uint128(claimed.0 - val.0);
        }
        let msgs: CosmosMsg = CosmosMsg::Staking(StakingMsg::Undelegate {
            validator,
            amount: coin(undelegated_amount.0, LUNA),
        });
        messages.push(msgs);
    }
    messages
}

fn compute_current_epoch(mut epoch_id: u64, prev_time: u64, current_time: u64) -> u64 {
    epoch_id += (current_time - prev_time) / EPOCH;
    epoch_id
}

pub fn query<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
    msg: QueryMsg,
) -> StdResult<Binary> {
    match msg {
        QueryMsg::ExchangeRate {} => to_binary(&query_exg_rate(&deps)?),
        QueryMsg::WhiteListedValidators {} => to_binary(&query_white_validators(&deps)?),
        QueryMsg::WithdrawableUnbonded { address } => {
            to_binary(&query_withdrawable_unbonded(&deps, address)?)
        }
        QueryMsg::GetToken {} => to_binary(&query_token(&deps)?),
        QueryMsg::GetReward {} => to_binary(&query_reward(&deps)?),
    }
}

fn query_exg_rate<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<Decimal> {
    let pool = pool_info_read(&deps.storage).load()?;
    Ok(pool.exchange_rate)
}

fn query_white_validators<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Vec<HumanAddr>> {
    let validators = read_valid_validators(&deps.storage)?;
    Ok(validators)
}

fn query_withdrawable_unbonded<S: Storage, A: Api, Q: Querier>(
    _deps: &Extern<S, A, Q>,
    _address: HumanAddr,
) -> StdResult<Uint128> {
    unimplemented!()
}

fn query_token<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<HumanAddr> {
    let pool = pool_info_read(&deps.storage).load()?;
    deps.api.human_address(&pool.token_account)
}

fn query_reward<S: Storage, A: Api, Q: Querier>(deps: &Extern<S, A, Q>) -> StdResult<HumanAddr> {
    let pool = pool_info_read(&deps.storage).load()?;
    deps.api.human_address(&pool.reward_account)
}

fn _query_total_issued<S: Storage, A: Api, Q: Querier>(
    deps: &Extern<S, A, Q>,
) -> StdResult<Uint128> {
    let token_address = deps
        .api
        .human_address(&pool_info_read(&deps.storage).load()?.token_account)?;
    let res = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Raw {
        contract_addr: token_address,
        key: Binary::from(to_length_prefixed(b"token_info")),
    }))?;
    let token_info: TokenInfo = from_binary(&res)?;
    Ok(token_info.total_supply)
}
