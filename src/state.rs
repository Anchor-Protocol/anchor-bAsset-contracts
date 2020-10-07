use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Decimal, HumanAddr, ReadonlyStorage, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};
use rand::Rng;
use std::collections::HashMap;

// EPOC = 21600s is equal to 6 hours
const EPOC: u64 = 21600;

pub static TOKEN_STATE_KEY: &[u8] = b"token_state";
pub static TOKEN_INFO_KEY: &[u8] = b"token_info";
pub static POOL_INFO: &[u8] = b"pool_info";
const BALANCE: &[u8] = b"balance";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenInfo {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: Uint128,
    //TODO: Add Undelegation Period as a TokenInfo which should be changed.
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Clone, JsonSchema, Debug)]
pub struct EpocId {
    pub epoc_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenState {
    pub current_epoc: u64,
    pub current_block_time: u64,
    pub delegation_map: HashMap<HumanAddr, Uint128>,
    pub holder_map: HashMap<HumanAddr, Decimal>,
    pub undelegated_wait_list: HashMap<EpocId, Undelegation>,
    pub redeem_wait_list_map: HashMap<HumanAddr, Uint128>,
}

impl TokenState {
    pub fn compute_current_epoc(&mut self, block_time: u64) {
        let epoc = self.current_epoc;
        let time = self.current_block_time;

        self.current_block_time = block_time;
        self.current_epoc = epoc + (block_time - time) / EPOC;
    }

    pub fn is_epoc_passed(&mut self, block_time: u64) -> bool {
        let time = self.current_block_time;

        self.current_block_time = block_time;
        if (block_time - time) / EPOC < 1 {
            return false;
        }
        true
    }

    pub fn choose_validator(&self, claim: Uint128) -> HumanAddr {
        let validator_array: Vec<HumanAddr> = self.delegation_map.clone().into_keys().collect();
        let mut rng = rand::thread_rng();
        loop {
            let random = rng.gen_range(0, validator_array.capacity() - 1);
            let address = validator_array.get(random).unwrap();
            let address_clone = address.clone();
            let val = self
                .delegation_map
                .get(address)
                .expect("The address existence is checked previously");
            if val > &claim {
                return address_clone;
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema, Default)]
pub struct Undelegation {
    pub claim: Uint128,
    pub undelegated_wait_list_map: HashMap<HumanAddr, Uint128>,
}

impl Undelegation {
    pub fn compute_claim(&mut self) {
        let mut claim = self.claim;
        for (_, value) in &self.undelegated_wait_list_map {
            claim += *value;
        }

        self.claim = claim;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PoolInfo {
    pub exchange_rate: Decimal,
    pub total_bond_amount: Uint128,
    pub total_issued: Uint128,
    pub claimed: Uint128,
    pub reward_index: Decimal,
}

impl Default for PoolInfo {
    fn default() -> Self {
        Self {
            exchange_rate: Decimal::one(),
            total_bond_amount: Default::default(),
            total_issued: Default::default(),
            claimed: Default::default(),
            reward_index: Default::default(),
        }
    }
}

impl PoolInfo {
    pub fn update_exchange_rate(&mut self) {
        //FIXME: Is total supply equal to total issued?
        self.exchange_rate = Decimal::from_ratio(self.total_bond_amount, self.total_issued);
    }
}
pub fn token_info<S: Storage>(storage: &mut S) -> Singleton<S, TokenInfo> {
    singleton(storage, TOKEN_INFO_KEY)
}

pub fn token_info_read<S: ReadonlyStorage>(storage: &S) -> ReadonlySingleton<S, TokenInfo> {
    singleton_read(storage, TOKEN_INFO_KEY)
}

pub fn balances<S: Storage>(storage: &mut S) -> Bucket<S, Uint128> {
    bucket(BALANCE, storage)
}

pub fn balances_read<S: ReadonlyStorage>(storage: &S) -> ReadonlyBucket<S, Uint128> {
    bucket_read(BALANCE, storage)
}

pub fn token_state<S: Storage>(storage: &mut S) -> Singleton<S, TokenState> {
    singleton(storage, TOKEN_STATE_KEY)
}

pub fn token_state_read<S: ReadonlyStorage>(storage: &S) -> ReadonlySingleton<S, TokenState> {
    singleton_read(storage, TOKEN_STATE_KEY)
}

pub fn pool_info<S: Storage>(storage: &mut S) -> Singleton<S, PoolInfo> {
    singleton(storage, POOL_INFO)
}

pub fn pool_info_read<S: ReadonlyStorage>(storage: &S) -> ReadonlySingleton<S, PoolInfo> {
    singleton_read(storage, POOL_INFO)
}
