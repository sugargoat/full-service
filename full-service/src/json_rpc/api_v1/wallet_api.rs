// Copyright (c) 2020-2021 MobileCoin Inc.

//! The Wallet API for Full Service. Version 1.

use crate::{
    json_rpc::api_v1::decorated_types::{
        JsonAddress, JsonBlock, JsonBlockContents, JsonProof, JsonSubmitResponse,
        JsonTransactionLog, JsonTxo, StringifiedJsonTxProposal,
    },
    service::WalletService,
};
use mc_common::logger::global_log;
use mc_connection::{BlockchainConnection, UserTxConnection};
use mc_fog_report_validation::FogPubkeyResolver;
use mc_mobilecoind_json::data_types::{JsonTx, JsonTxOut, JsonTxProposal};
use rocket_contrib::json::Json;
use serde::{Deserialize, Serialize};
use serde_json::Map;
use std::{convert::TryFrom, iter::FromIterator};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

// Helper method to format displaydoc errors in json.
fn format_error<T: std::fmt::Display + std::fmt::Debug>(e: T) -> String {
    json!({"error": format!("{:?}", e), "details": e.to_string()}).to_string()
}

pub fn help_str_v1() -> String {
    let mut help_str = "Please use json data to choose wallet commands. For example, \n\ncurl -s localhost:9090/wallet -d '{\"method\": \"create_account\", \"params\": {\"name\": \"Alice\"}}' -X POST -H 'Content-type: application/json'\n\nAvailable commands are:\n\n".to_owned();
    for e in JsonCommandRequestV1::iter() {
        help_str.push_str(&format!("{:?}\n\n", e));
    }
    help_str
}

#[derive(Deserialize, Serialize, EnumIter, Debug)]
#[serde(tag = "method", content = "params")]
#[allow(non_camel_case_types)]
pub enum JsonCommandRequestV1 {
    get_all_txos_by_account {
        account_id: String,
    },
    get_txo {
        txo_id: String,
    },
    create_address {
        account_id: String,
        comment: Option<String>,
    },
    get_all_addresses_by_account {
        account_id: String,
    },
    send_transaction {
        account_id: String,
        recipient_public_address: String,
        value: String,
        input_txo_ids: Option<Vec<String>>,
        fee: Option<String>,
        tombstone_block: Option<String>,
        max_spendable_value: Option<String>,
        comment: Option<String>,
    },
    build_transaction {
        account_id: String,
        recipient_public_address: String,
        value: String,
        input_txo_ids: Option<Vec<String>>,
        fee: Option<String>,
        tombstone_block: Option<String>,
        max_spendable_value: Option<String>,
    },
    submit_transaction {
        tx_proposal: StringifiedJsonTxProposal,
        comment: Option<String>,
        account_id: Option<String>,
    },
    get_all_transactions_by_account {
        account_id: String,
    },
    get_transaction {
        transaction_log_id: String,
    },
    get_transaction_object {
        transaction_log_id: String,
    },
    get_txo_object {
        txo_id: String,
    },
    get_block_object {
        block_index: String,
    },
    get_proofs {
        transaction_log_id: String,
    },
    verify_proof {
        account_id: String,
        txo_id: String,
        proof: String,
    },
}
#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "method", content = "result")]
#[allow(non_camel_case_types)]
pub enum JsonCommandResponseV1 {
    get_all_txos_by_account {
        txo_ids: Vec<String>,
        txo_map: Map<String, serde_json::Value>,
    },
    get_txo {
        txo: JsonTxo,
    },
    create_address {
        address: JsonAddress,
    },
    get_all_addresses_by_account {
        address_ids: Vec<String>,
        address_map: Map<String, serde_json::Value>,
    },
    send_transaction {
        transaction: JsonSubmitResponse,
    },
    build_transaction {
        tx_proposal: StringifiedJsonTxProposal,
    },
    submit_transaction {
        transaction: JsonSubmitResponse,
    },
    get_all_transactions_by_account {
        transaction_log_ids: Vec<String>,
        transaction_log_map: Map<String, serde_json::Value>,
    },
    get_transaction {
        transaction: JsonTransactionLog,
    },
    get_transaction_object {
        transaction: JsonTx,
    },
    get_txo_object {
        txo: JsonTxOut,
    },
    get_block_object {
        block: JsonBlock,
        block_contents: JsonBlockContents,
    },
    get_proofs {
        proofs: Vec<JsonProof>,
    },
    verify_proof {
        verified: bool,
    },
}

// The Wallet API inner method, which handles switching on the method enum.
//
// Note that this is structured this way so that the routes can be defined to
// take explicit Rocket state, and then pass the service to the inner method.
// This allows us to properly construct state with Mock Connection Objects in
// tests. This also allows us to version the overall API easily.
#[allow(clippy::bind_instead_of_map)]
pub fn wallet_api_inner_v1<T, FPR>(
    service: &WalletService<T, FPR>,
    command: Json<JsonCommandRequestV1>,
) -> Result<Json<JsonCommandResponseV1>, String>
where
    T: BlockchainConnection + UserTxConnection + 'static,
    FPR: FogPubkeyResolver + Send + Sync + 'static,
{
    global_log::trace!("Running command {:?}", command);
    let result = match command.0 {
        JsonCommandRequestV1::get_all_txos_by_account { account_id } => {
            let txos = service.list_txos(&account_id).map_err(format_error)?;
            let txo_map: Map<String, serde_json::Value> = Map::from_iter(
                txos.iter()
                    .map(|t| {
                        (
                            t.txo_id.clone(),
                            serde_json::to_value(t.clone()).expect("Could not get json value"),
                        )
                    })
                    .collect::<Vec<(String, serde_json::Value)>>(),
            );

            JsonCommandResponseV1::get_all_txos_by_account {
                txo_ids: txos.iter().map(|t| t.txo_id.clone()).collect(),
                txo_map,
            }
        }
        JsonCommandRequestV1::get_txo { txo_id } => {
            let result = service.get_txo(&txo_id).map_err(format_error)?;
            JsonCommandResponseV1::get_txo { txo: result }
        }
        JsonCommandRequestV1::create_address {
            account_id,
            comment,
        } => JsonCommandResponseV1::create_address {
            address: service
                .create_assigned_subaddress(&account_id, comment.as_deref())
                .map_err(format_error)?,
        },
        JsonCommandRequestV1::get_all_addresses_by_account { account_id } => {
            let addresses = service
                .list_assigned_subaddresses(&account_id)
                .map_err(format_error)?;
            let address_map: Map<String, serde_json::Value> = Map::from_iter(
                addresses
                    .iter()
                    .map(|a| {
                        (
                            a.address_id.clone(),
                            serde_json::to_value(&(*a).clone()).expect("Could not get json value"),
                        )
                    })
                    .collect::<Vec<(String, serde_json::Value)>>(),
            );

            JsonCommandResponseV1::get_all_addresses_by_account {
                address_ids: addresses.iter().map(|a| a.address_id.clone()).collect(),
                address_map,
            }
        }
        JsonCommandRequestV1::send_transaction {
            account_id,
            recipient_public_address,
            value,
            input_txo_ids,
            fee,
            tombstone_block,
            max_spendable_value,
            comment,
        } => {
            let transaction_details = service
                .send_transaction(
                    &account_id,
                    &recipient_public_address,
                    value,
                    input_txo_ids.as_ref(),
                    fee,
                    tombstone_block,
                    max_spendable_value,
                    comment,
                )
                .map_err(format_error)?;
            JsonCommandResponseV1::send_transaction {
                transaction: transaction_details,
            }
        }
        JsonCommandRequestV1::build_transaction {
            account_id,
            recipient_public_address,
            value,
            input_txo_ids,
            fee,
            tombstone_block,
            max_spendable_value,
        } => {
            let tx_proposal = service
                .build_transaction(
                    &account_id,
                    &recipient_public_address,
                    value,
                    input_txo_ids.as_ref(),
                    fee,
                    tombstone_block,
                    max_spendable_value,
                )
                .map_err(format_error)?;
            JsonCommandResponseV1::build_transaction {
                tx_proposal: StringifiedJsonTxProposal::from(&tx_proposal),
            }
        }
        JsonCommandRequestV1::submit_transaction {
            tx_proposal,
            comment,
            account_id,
        } => JsonCommandResponseV1::submit_transaction {
            transaction: service
                .submit_transaction(JsonTxProposal::try_from(&tx_proposal)?, comment, account_id)
                .map_err(format_error)?,
        },
        JsonCommandRequestV1::get_all_transactions_by_account { account_id } => {
            let transactions = service
                .list_transactions(&account_id)
                .map_err(format_error)?;
            let transaction_log_map: Map<String, serde_json::Value> = Map::from_iter(
                transactions
                    .iter()
                    .map(|t| {
                        (
                            t.transaction_log_id.clone(),
                            serde_json::to_value(&(*t).clone()).expect("Could not get json value"),
                        )
                    })
                    .collect::<Vec<(String, serde_json::Value)>>(),
            );

            JsonCommandResponseV1::get_all_transactions_by_account {
                transaction_log_ids: transactions
                    .iter()
                    .map(|t| t.transaction_log_id.clone())
                    .collect(),
                transaction_log_map,
            }
        }
        JsonCommandRequestV1::get_transaction { transaction_log_id } => {
            JsonCommandResponseV1::get_transaction {
                transaction: service
                    .get_transaction(&transaction_log_id)
                    .map_err(format_error)?,
            }
        }
        JsonCommandRequestV1::get_transaction_object { transaction_log_id } => {
            JsonCommandResponseV1::get_transaction_object {
                transaction: service
                    .get_transaction_object(&transaction_log_id)
                    .map_err(format_error)?,
            }
        }
        JsonCommandRequestV1::get_txo_object { txo_id } => JsonCommandResponseV1::get_txo_object {
            txo: service.get_txo_object(&txo_id).map_err(format_error)?,
        },
        JsonCommandRequestV1::get_block_object { block_index } => {
            let (block, block_contents) = service
                .get_block_object(block_index.parse::<u64>().map_err(format_error)?)
                .map_err(format_error)?;
            JsonCommandResponseV1::get_block_object {
                block,
                block_contents,
            }
        }
        JsonCommandRequestV1::get_proofs { transaction_log_id } => {
            JsonCommandResponseV1::get_proofs {
                proofs: service
                    .get_proofs(&transaction_log_id)
                    .map_err(format_error)?,
            }
        }
        JsonCommandRequestV1::verify_proof {
            account_id,
            txo_id,
            proof,
        } => {
            let result = service
                .verify_proof(&account_id, &txo_id, &proof)
                .map_err(format_error)?;
            JsonCommandResponseV1::verify_proof { verified: result }
        }
    };
    Ok(Json(result))
}

#[cfg(test)]
mod e2e {
    use crate::{
        db::b58_decode,
        json_rpc::api_test_utils::{dispatch, dispatch_expect_error, setup, wait_for_sync},
        test_utils::add_block_to_ledger_db,
    };
    use mc_common::logger::{test_with_logger, Logger};
    use mc_crypto_rand::rand_core::RngCore;
    use mc_transaction_core::ring_signature::KeyImage;
    use rand::{rngs::StdRng, SeedableRng};

    #[test_with_logger]
    fn test_get_all_txos(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);
        let (client, mut ledger_db, _db_ctx, network_state) = setup(&mut rng, logger.clone());

        // Add an account
        let body = json!({
            "api_version": "2",
            "method": "create_account",
            "params": {
                "name": "Alice Main Account",
                "first_block": "0",
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let account_obj = result.get("account").unwrap();
        let account_id = account_obj.get("account_id").unwrap().as_str().unwrap();
        let b58_public_address = account_obj.get("main_address").unwrap().as_str().unwrap();
        let public_address = b58_decode(b58_public_address).unwrap();

        // Add a block with a txo for this address
        add_block_to_ledger_db(
            &mut ledger_db,
            &vec![public_address],
            100,
            &vec![KeyImage::from(rng.next_u64())],
            &mut rng,
        );

        wait_for_sync(&client, &ledger_db, &network_state, &logger);

        let body = json!({
            "method": "get_all_txos_by_account",
            "params": {
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let txos = result.get("txo_ids").unwrap().as_array().unwrap();
        assert_eq!(txos.len(), 1);
        let txo_map = result.get("txo_map").unwrap().as_object().unwrap();
        let txo = txo_map.get(txos[0].as_str().unwrap()).unwrap();
        let account_status_map = txo
            .get("account_status_map")
            .unwrap()
            .as_object()
            .unwrap()
            .get(account_id)
            .unwrap();
        let txo_status = account_status_map
            .get("txo_status")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(txo_status, "txo_status_unspent");
        let txo_type = account_status_map
            .get("txo_type")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(txo_type, "txo_type_received");
        let value = txo.get("value_pmob").unwrap().as_str().unwrap();
        assert_eq!(value, "100");

        // Check the overall balance for the account
        let body = json!({
            "api_version": "2",
            "method": "get_balance_for_account",
            "params": {
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let balance_status = result.get("balance").unwrap();
        let unspent = balance_status
            .get("unspent_pmob")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(unspent, "100");
    }

    #[test_with_logger]
    fn test_build_and_submit_transaction(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);
        let (client, mut ledger_db, _db_ctx, network_state) = setup(&mut rng, logger.clone());

        // Add an account
        let body = json!({
            "api_version": "2",
            "method": "create_account",
            "params": {
                "name": "Alice Main Account",
                "first_block": "0",
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let account_obj = result.get("account").unwrap();
        let account_id = account_obj.get("account_id").unwrap().as_str().unwrap();
        let b58_public_address = account_obj.get("main_address").unwrap().as_str().unwrap();
        let public_address = b58_decode(b58_public_address).unwrap();

        // Add a block with a txo for this address (note that value is smaller than
        // MINIMUM_FEE)
        add_block_to_ledger_db(
            &mut ledger_db,
            &vec![public_address.clone()],
            100,
            &vec![KeyImage::from(rng.next_u64())],
            &mut rng,
        );

        wait_for_sync(&client, &ledger_db, &network_state, &logger);

        // Create a tx proposal to ourselves
        let body = json!({
            "method": "build_transaction",
            "params": {
                "account_id": account_id,
                "recipient_public_address": b58_public_address,
                "value": "42",
            }
        });
        // We will fail because we cannot afford the fee, which is 100000000000 pMOB
        // (.01 MOB)
        dispatch_expect_error(&client, body, &logger, "{\"details\":\"Error building transaction: Wallet DB Error: Insufficient funds from Txos under max_spendable_value: Max spendable value in wallet: 100, but target value: 10000000042\",\"error\":\"TransactionBuilder(WalletDb(InsufficientFundsUnderMaxSpendable(\\\"Max spendable value in wallet: 100, but target value: 10000000042\\\")))\"}".to_string());

        // Add a block with significantly more MOB
        add_block_to_ledger_db(
            &mut ledger_db,
            &vec![public_address],
            100000000000000, // 100.0 MOB
            &vec![KeyImage::from(rng.next_u64())],
            &mut rng,
        );

        wait_for_sync(&client, &ledger_db, &network_state, &logger);

        // Create a tx proposal to ourselves
        let body = json!({
            "method": "build_transaction",
            "params": {
                "account_id": account_id,
                "recipient_public_address": b58_public_address,
                "value": "42000000000000", // 42.0 MOB
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let tx_proposal = result.get("tx_proposal").unwrap();
        let tx = tx_proposal.get("tx").unwrap();
        let tx_prefix = tx.get("prefix").unwrap();

        // Assert the fee is correct in both places
        let prefix_fee = tx_prefix.get("fee").unwrap().as_str().unwrap();
        let fee = tx_proposal.get("fee").unwrap();
        // FIXME: WS-9 - Note, minimum fee does not fit into i32 - need to make sure we
        // are not losing precision with the JsonTxProposal treating Fee as number
        assert_eq!(fee, "10000000000");
        assert_eq!(fee, prefix_fee);

        // Transaction builder attempts to use as many inputs as we have txos
        let inputs = tx_proposal.get("input_list").unwrap().as_array().unwrap();
        assert_eq!(inputs.len(), 2);
        let prefix_inputs = tx_prefix.get("inputs").unwrap().as_array().unwrap();
        assert_eq!(prefix_inputs.len(), inputs.len());

        // One destination
        let outlays = tx_proposal.get("outlay_list").unwrap().as_array().unwrap();
        assert_eq!(outlays.len(), 1);

        // Map outlay -> tx_out, should have one entry for one outlay
        let outlay_index_to_tx_out_index = tx_proposal
            .get("outlay_index_to_tx_out_index")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(outlay_index_to_tx_out_index.len(), 1);

        // Two outputs in the prefix, one for change
        let prefix_outputs = tx_prefix.get("outputs").unwrap().as_array().unwrap();
        assert_eq!(prefix_outputs.len(), 2);

        // One outlay confirmation number for our one outlay (no receipt for change)
        let outlay_confirmation_numbers = tx_proposal
            .get("outlay_confirmation_numbers")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(outlay_confirmation_numbers.len(), 1);

        // Tombstone block = ledger height (12 to start + 2 new blocks + 50 default
        // tombstone)
        let prefix_tombstone = tx_prefix.get("tombstone_block").unwrap();
        assert_eq!(prefix_tombstone, "64");

        // Get current balance
        let body = json!({
            "api_version": "2",
            "method": "get_balance_for_account",
            "params": {
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let balance_status = result.get("balance").unwrap();
        let unspent = balance_status
            .get("unspent_pmob")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(unspent, "100000000000100");

        // Submit the tx_proposal
        let body = json!({
            "method": "submit_transaction",
            "params": {
                "tx_proposal": tx_proposal,
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let transaction_id = result
            .get("transaction")
            .unwrap()
            .get("transaction_id")
            .unwrap()
            .as_str()
            .unwrap();
        // Note - we cannot test here that the transaction ID is consistent, because
        // there is randomness in the transaction creation.

        wait_for_sync(&client, &ledger_db, &network_state, &logger);

        // Get balance after submission
        let body = json!({
            "api_version": "2",
            "method": "get_balance_for_account",
            "params": {
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let balance_status = result.get("balance").unwrap();
        let unspent = balance_status
            .get("unspent_pmob")
            .unwrap()
            .as_str()
            .unwrap();
        let pending = balance_status
            .get("pending_pmob")
            .unwrap()
            .as_str()
            .unwrap();
        let spent = balance_status.get("spent_pmob").unwrap().as_str().unwrap();
        let secreted = balance_status
            .get("secreted_pmob")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(unspent, "0");
        assert_eq!(pending, "0");
        assert_eq!(spent, "99990000000100");
        assert_eq!(secreted, "0");

        // FIXME: FS-93 Increment ledger manually so tx lands.

        // Get the transaction_id and verify it contains what we expect
        let body = json!({
            "method": "get_transaction",
            "params": {
                "transaction_log_id": transaction_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let transaction_log = result.get("transaction").unwrap();
        assert_eq!(
            transaction_log.get("direction").unwrap().as_str().unwrap(),
            "tx_direction_sent"
        );
        assert_eq!(
            transaction_log.get("value_pmob").unwrap().as_str().unwrap(),
            "42000000000000"
        );
        assert_eq!(
            transaction_log
                .get("recipient_address_id")
                .unwrap()
                .as_str()
                .unwrap(),
            b58_public_address
        );
        transaction_log.get("account_id").unwrap().as_str().unwrap();
        assert_eq!(
            transaction_log.get("fee_pmob").unwrap().as_str().unwrap(),
            "10000000000"
        );
        assert_eq!(
            transaction_log.get("status").unwrap().as_str().unwrap(),
            "tx_status_pending"
        );
        assert_eq!(
            transaction_log
                .get("submitted_block_height")
                .unwrap()
                .as_str()
                .unwrap(),
            "14"
        );
        assert_eq!(
            transaction_log
                .get("transaction_log_id")
                .unwrap()
                .as_str()
                .unwrap(),
            transaction_id
        );
    }

    #[test_with_logger]
    fn test_create_assigned_subaddress(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);
        let (client, mut ledger_db, _db_ctx, network_state) = setup(&mut rng, logger.clone());

        // Add an account
        let body = json!({
            "api_version": "2",
            "method": "create_account",
            "params": {
                "name": "Alice Main Account",
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let account_id = result
            .get("account")
            .unwrap()
            .get("account_id")
            .unwrap()
            .as_str()
            .unwrap();

        // Create a subaddress
        let body = json!({
            "method": "create_address",
            "params": {
                "account_id": account_id,
                "comment": "For Bob",
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let b58_public_address = result
            .get("address")
            .unwrap()
            .get("public_address")
            .unwrap()
            .as_str()
            .unwrap();
        let from_bob_public_address = b58_decode(b58_public_address).unwrap();

        // Add a block to the ledger with a transaction "From Bob"
        add_block_to_ledger_db(
            &mut ledger_db,
            &vec![from_bob_public_address],
            42000000000000,
            &vec![KeyImage::from(rng.next_u64())],
            &mut rng,
        );

        wait_for_sync(&client, &ledger_db, &network_state, &logger);

        let body = json!({
            "method": "get_all_txos_by_account",
            "params": {
                "account_id": account_id,
            }
        });
        let res = dispatch(&client, body, &logger);
        let result = res.get("result").unwrap();
        let txos = result.get("txo_ids").unwrap().as_array().unwrap();
        assert_eq!(txos.len(), 1);
        let txo_map = result.get("txo_map").unwrap().as_object().unwrap();
        let txo = &txo_map.get(txos[0].as_str().unwrap()).unwrap();
        let status_map = txo
            .get("account_status_map")
            .unwrap()
            .as_object()
            .unwrap()
            .get(account_id)
            .unwrap();
        let txo_status = status_map.get("txo_status").unwrap().as_str().unwrap();
        assert_eq!(txo_status, "txo_status_unspent");
        let txo_type = status_map.get("txo_type").unwrap().as_str().unwrap();
        assert_eq!(txo_type, "txo_type_received");
        let value = txo.get("value_pmob").unwrap().as_str().unwrap();
        assert_eq!(value, "42000000000000");
    }
}