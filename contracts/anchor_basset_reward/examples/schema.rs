use std::env::current_dir;
use std::fs::create_dir_all;

use anchor_basset_reward::msg::{
    AccruedRewardsResponse, ConfigResponse, HandleMsg, HolderResponse, HoldersResponse, InitMsg,
    QueryMsg, StateResponse,
};
use cosmwasm_schema::{export_schema, remove_schemas, schema_for};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InitMsg), &out_dir);
    export_schema(&schema_for!(HandleMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(ConfigResponse), &out_dir);
    export_schema(&schema_for!(StateResponse), &out_dir);
    export_schema(&schema_for!(AccruedRewardsResponse), &out_dir);
    export_schema(&schema_for!(HolderResponse), &out_dir);
    export_schema(&schema_for!(HoldersResponse), &out_dir);
}
