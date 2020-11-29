// Copyright (c) 2020 MobileCoin Inc.

//! Decorated types for the service to return, with constructors from the database types.

use crate::models::{AccountTxoStatus, Txo};
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Default)]
pub struct JsonCreateAccountResponse {
    pub entropy: String,
    pub public_address_b58: String,
    pub account_id: String,
}

#[derive(Deserialize, Serialize, Default)]
pub struct JsonImportAccountResponse {
    pub public_address_b58: String,
    pub account_id: String,
}

#[derive(Deserialize, Serialize, Default)]
pub struct JsonListTxosResponse {
    pub txo_id: String,
    pub value: String,
    pub txo_type: String,
    pub txo_status: String,
}

impl JsonListTxosResponse {
    pub fn new(txo: &Txo, account_txo_status: &AccountTxoStatus) -> Self {
        Self {
            txo_id: txo.txo_id_hex.clone(),
            value: txo.value.to_string(),
            txo_type: account_txo_status.txo_type.clone(),
            txo_status: account_txo_status.txo_status.clone(),
        }
    }
}