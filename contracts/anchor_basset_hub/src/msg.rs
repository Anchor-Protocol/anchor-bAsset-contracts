use crate::state::UnbondHistory;
use cosmwasm_std::{Decimal, HumanAddr, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type UnbondRequest = Vec<(u64, Uint128)>;
pub type History = Vec<(u64, UnbondHistory)>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub epoch_period: u64,
    pub underlying_coin_denom: String,
    pub unbonding_period: u64,
    pub peg_recovery_fee: Decimal,
    pub er_threshold: Decimal,
    pub reward_denom: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    State {},
    ExchangeRate {},
    WhitelistedValidators {},
    CurrentBatch {},
    WithdrawableUnbonded {
        address: HumanAddr,
        block_time: u64,
    },
    Parameters {},
    UnbondRequests {
        address: HumanAddr,
    },
    UnbondBatches {
        address: HumanAddr,
    },
    AllHistory {
        start_from: Option<u64>,
        limit: Option<u32>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateResponse {
    pub exchange_rate: Decimal,
    pub total_bond_amount: Uint128,
    pub last_index_modification: u64,
    pub prev_hub_balance: Uint128,
    pub actual_unbonded_amount: Uint128,
    pub last_unbonded_time: u64,
    pub last_processed_batch: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
    pub owner: HumanAddr,
    pub reward_contract: Option<HumanAddr>,
    pub token_contract: Option<HumanAddr>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ExchangeRateResponse {
    pub rate: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WhitelistedValidatorsResponse {
    pub validators: Vec<HumanAddr>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CurrentBatchResponse {
    pub id: u64,
    pub requested_with_fee: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct WithdrawableUnbondedResponse {
    pub withdrawable: Uint128,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnbondRequestsResponse {
    pub address: HumanAddr,
    pub requests: UnbondRequest,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UnbondBatchesResponse {
    pub unbond_batches: Vec<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct AllHistoryResponse {
    pub history: History,
}
