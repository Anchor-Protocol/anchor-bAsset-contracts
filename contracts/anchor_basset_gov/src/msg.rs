use cosmwasm_std::{HumanAddr, StdError, StdResult, Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InitMsg {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub code_id: u64,
}

impl InitMsg {
    pub fn validate(&self) -> StdResult<()> {
        // Check name, symbol, decimals
        if !is_valid_name(&self.name) {
            return Err(StdError::generic_err(
                "Name is not in the expected format (3-30 UTF-8 bytes)",
            ));
        }
        if !is_valid_symbol(&self.symbol) {
            return Err(StdError::generic_err(
                "Ticker symbol is not in expected format [A-Z]{3,6}",
            ));
        }
        //terra supports 6 decimals
        if self.decimals > 6 {
            return Err(StdError::generic_err("Decimals must not exceed 6"));
        }
        Ok(())
    }
}

fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < 3 || bytes.len() > 30 {
        return false;
    }
    true
}

fn is_valid_symbol(symbol: &str) -> bool {
    let bytes = symbol.as_bytes();
    if bytes.len() < 3 || bytes.len() > 6 {
        return false;
    }
    for byte in bytes.iter() {
        if *byte < 65 || *byte > 90 {
            return false;
        }
    }
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandleMsg {
    /// Mint is a message to work as follows:
    /// Receives `amount` Luna from sender.
    /// Delegate `amount` to a specific `validator`.
    /// Issue the same `amount` of bLuna to sender.
    Mint {
        validator: HumanAddr,
    },
    /// Update general index
    UpdateGlobalIndex {},
    /// InitBurn is send an undelegate message after receiving all
    /// requests for an specific period of time.
    InitBurn {
        amount: Uint128,
    },
    /// FinishBurn is suppose to ask for liquidated luna
    FinishBurn {
        amount: Uint128,
    },
    /// Send is like a base message in CW20 to move bluna to another account
    Send {
        recipient: HumanAddr,
        amount: Uint128,
    },
    // Register receives the reward contract address
    Register {},
    // Register valid validators to validators whitelist
    RegisterValidator {
        validator: HumanAddr,
    },
    // Remove the validator from validators whitelist
    DeRegisterValidator {
        validator: HumanAddr,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct TokenInfoResponse {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub total_supply: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Balance { address: HumanAddr },
    TokenInfo {},
    ExchangeRate {},
    WhiteListedValidators {},
    AccruedRewards { address: HumanAddr },
    WithdrawableUnbonded { address: HumanAddr },
}
