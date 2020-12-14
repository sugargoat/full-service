// Copyright (c) 2020 MobileCoin Inc.

//! DB impl for the Txo model.

use crate::{
    db::{
        account::{AccountID, AccountModel},
        account_txo_status::AccountTxoStatusModel,
        assigned_subaddress::AssignedSubaddressModel,
        b58_encode,
        models::{
            Account, AccountTxoStatus, AssignedSubaddress, NewAccountTxoStatus, NewTxo, Txo,
            CHANGE, OUTPUT, TXO_MINTED, TXO_ORPHANED, TXO_PENDING, TXO_RECEIVED, TXO_SECRETED,
            TXO_SPENT, TXO_UNSPENT,
        },
    },
    error::WalletDbError,
};

use mc_account_keys::{AccountKey, PublicAddress};
use mc_common::HashMap;
use mc_crypto_digestible::{Digestible, MerlinTranscript};
use mc_crypto_keys::RistrettoPublic;
use mc_mobilecoind::payments::TxProposal;
use mc_transaction_core::{
    constants::MAX_INPUTS,
    ring_signature::KeyImage,
    tx::{TxOut, TxOutConfirmationNumber},
};

use diesel::{
    prelude::*,
    r2d2::{ConnectionManager, PooledConnection},
    RunQueryDsl,
};
use std::{fmt, iter::FromIterator};

#[derive(Debug)]
pub struct TxoID(String);

impl From<&TxOut> for TxoID {
    fn from(src: &TxOut) -> TxoID {
        let temp: [u8; 32] = src.digest32::<MerlinTranscript>(b"txo_data");
        Self(hex::encode(temp))
    }
}

impl fmt::Display for TxoID {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub trait TxoModel {
    /// Create a received Txo.
    ///
    /// Note that a received Txo may have a null subaddress_index if the Txo is "orphaned."
    /// This means that in syncing, the Txo was determined to belong to an account, but the
    /// subaddress is not yet being tracked, so we were unable to match the subaddress spend
    /// public key. An orphaned Txo is not spendable until the subaddress to which it belongs
    /// is added to the assigned_subaddresses table.
    ///
    /// Returns:
    /// * txo_id_hex
    fn create_received(
        txo: TxOut,
        subaddress_index: Option<i64>,
        key_image: Option<KeyImage>,
        value: u64,
        received_block_height: i64,
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<String, WalletDbError>;

    /// Create a new minted Txo.
    ///
    /// Returns:
    /// * (public address of the recipient, txo_id_hex, value of the txo, txo type)
    fn create_minted(
        account_id_hex: Option<&str>,
        txo: &TxOut,
        tx_proposal: &TxProposal,
        outlay_index: usize,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(Option<PublicAddress>, String, i64, String), WalletDbError>;

    /// Update an existing Txo when it is received in the ledger.
    /// A Txo can be created before being received if it is minted, for example.
    fn update_received(
        &self,
        account_id_hex: &str,
        subaddress_index: Option<i64>,
        key_image: Option<KeyImage>,
        received_block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError>;

    /// Update an existing Txo to spendable by including its subaddress_index and key_image.
    fn update_to_spendable(
        &self,
        received_subaddress_index: Option<i64>,
        received_key_image: Option<KeyImage>,
        block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError>;

    /// Update a Txo's status to pending
    fn update_to_pending(
        txo_id_hex: &TxoID,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError>;

    /// Get all Txos associated with a given account.
    fn list_for_account(
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<(Txo, AccountTxoStatus)>, WalletDbError>;

    /// Get a map of txo_status -> Vec<Txo> for all txos in a given account.
    fn list_by_status(
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<HashMap<String, Vec<Txo>>, WalletDbError>;

    /// Get the details for a specific Txo for a given account.
    ///
    /// Returns:
    /// * (Txo, Txo Status, Assigned Subaddress)
    fn get(
        account_id_hex: &AccountID,
        txo_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(Txo, AccountTxoStatus, Option<AssignedSubaddress>), WalletDbError>;

    /// Select several Txos by their TxoIds
    ///
    /// Returns:
    /// * Vec<(Txo, TxoStatus)>
    fn select_by_id(
        txo_ids: &[String],
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<(Txo, AccountTxoStatus)>, WalletDbError>;

    /// Check whether all of the given Txos are spent.
    fn are_all_spent(
        txo_ids: &[String],
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError>;

    /// Check whether any of the given Txos failed.
    fn any_failed(
        txo_ids: &[String],
        block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError>;

    /// Select a set of unspent Txos to reach a given value.
    ///
    /// Returns:
    /// * Vec<Txo>
    fn select_unspent_txos_for_value(
        account_id_hex: &str,
        target_value: u64,
        max_spendable_value: Option<i64>,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<Txo>, WalletDbError>;

    /// Verify a proof for a Txo
    ///
    /// Returns:
    /// * Bool - true if verified
    fn verify_proof(
        account_id: &AccountID,
        txo_id: &str,
        proof: &TxOutConfirmationNumber,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError>;
}

impl TxoModel for Txo {
    fn create_received(
        txo: TxOut,
        subaddress_index: Option<i64>,
        key_image: Option<KeyImage>,
        value: u64,
        received_block_height: i64,
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<String, WalletDbError> {
        let txo_id = TxoID::from(&txo);
        conn.transaction::<(), WalletDbError, _>(|| {
            // If we already have this TXO for this account (e.g. from minting in a previous transaction), we need to update it
            match Txo::get(
                &AccountID(account_id_hex.to_string()),
                &txo_id.to_string(),
                conn,
            ) {
                Ok((received_txo, _txo_status, _opt_assigned_subaddress)) => {
                    received_txo.update_received(
                        account_id_hex,
                        subaddress_index,
                        key_image,
                        received_block_height,
                        conn,
                    )?;
                }
                Err(WalletDbError::TxoExistsForAnotherAccount(_)) => {
                    // Txo already exists for another account. Update the status with respect to this account
                    let status = if subaddress_index.is_some() {
                        TXO_UNSPENT
                    } else {
                        // Note: An orphaned Txo cannot be spent until the subaddress is recovered.
                        TXO_ORPHANED
                    };
                    AccountTxoStatus::create(
                        account_id_hex,
                        &txo_id.to_string(),
                        status,
                        TXO_RECEIVED,
                        conn,
                    )?;
                }
                // If we don't already have this TXO, create a new entry
                Err(WalletDbError::TxoNotFound(_)) => {
                    let key_image_bytes = key_image.map(|k| mc_util_serial::encode(&k));
                    let new_txo = NewTxo {
                        txo_id_hex: &txo_id.to_string(),
                        value: value as i64,
                        target_key: &mc_util_serial::encode(&txo.target_key),
                        public_key: &mc_util_serial::encode(&txo.public_key),
                        e_fog_hint: &mc_util_serial::encode(&txo.e_fog_hint),
                        txo: &mc_util_serial::encode(&txo),
                        subaddress_index,
                        key_image: key_image_bytes.as_ref(),
                        received_block_height: Some(received_block_height as i64),
                        pending_tombstone_block_height: None,
                        spent_block_height: None,
                        proof: None,
                    };

                    diesel::insert_into(crate::db::schema::txos::table)
                        .values(&new_txo)
                        .execute(conn)?;

                    let status = if subaddress_index.is_some() {
                        TXO_UNSPENT
                    } else {
                        // Note: An orphaned Txo cannot be spent until the subaddress is recovered.
                        TXO_ORPHANED
                    };
                    AccountTxoStatus::create(
                        account_id_hex,
                        &txo_id.to_string(),
                        status,
                        TXO_RECEIVED,
                        conn,
                    )?;
                }
                Err(e) => {
                    return Err(e);
                }
            };
            Ok(())
        })?;
        Ok(txo_id.to_string())
    }

    fn create_minted(
        account_id_hex: Option<&str>,
        output: &TxOut,
        tx_proposal: &TxProposal,
        output_index: usize,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(Option<PublicAddress>, String, i64, String), WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let txo_id = TxoID::from(output);

        let total_input_value: u64 = tx_proposal.utxos.iter().map(|u| u.value).sum();
        let total_output_value: u64 = tx_proposal.outlays.iter().map(|o| o.value).sum();
        let change_value: u64 = total_input_value - total_output_value - tx_proposal.fee();
        // Determine whether this output is an outlay destination, or change.
        let (value, proof, outlay_receiver) = if let Some(outlay_index) = tx_proposal
            .outlay_index_to_tx_out_index
            .iter()
            .find_map(|(k, &v)| if v == output_index { Some(k) } else { None })
        {
            let outlay = &tx_proposal.outlays[*outlay_index];
            (
                outlay.value,
                Some(*outlay_index),
                Some(outlay.receiver.clone()),
            )
        } else {
            // This is the change output. Note: there should only be one change output
            // per transaction, based on how we construct transactions. If we change
            // how we construct transactions, these assumptions will change, and should be
            // reflected in the TxProposal.
            (change_value, None, None)
        };

        // Update receiver, transaction_value, and transaction_txo_type, if outlay was found.
        let transaction_txo_type = if outlay_receiver.is_some() {
            OUTPUT
        } else {
            // If not in an outlay, this output is change, according to how we build transactions.
            CHANGE
        };

        let encoded_proof =
            proof.map(|p| mc_util_serial::encode(&tx_proposal.outlay_confirmation_numbers[p]));

        conn.transaction::<(), WalletDbError, _>(|| {
            let new_txo = NewTxo {
                txo_id_hex: &txo_id.to_string(),
                value: value as i64,
                target_key: &mc_util_serial::encode(&output.target_key),
                public_key: &mc_util_serial::encode(&output.public_key),
                e_fog_hint: &mc_util_serial::encode(&output.e_fog_hint),
                txo: &mc_util_serial::encode(output),
                subaddress_index: None, // Minted set subaddress_index to None. If later received, updates.
                key_image: None,        // Only the recipient can calculate the KeyImage
                received_block_height: None,
                pending_tombstone_block_height: Some(tx_proposal.tx.prefix.tombstone_block as i64),
                spent_block_height: None,
                proof: encoded_proof.as_ref(),
            };

            diesel::insert_into(txos::table)
                .values(&new_txo)
                .execute(conn)?;

            // If account_id is provided, then log a relationship. Also possible to create minted
            // from a TxProposal not belonging to any existing account.
            if let Some(account_id_hex) = account_id_hex.as_deref() {
                let new_account_txo_status = NewAccountTxoStatus {
                    account_id_hex: &account_id_hex,
                    txo_id_hex: &txo_id.to_string(),
                    txo_status: TXO_SECRETED, // We cannot track spent status for minted TXOs unless change
                    txo_type: TXO_MINTED,
                };
                diesel::insert_into(account_txo_statuses::table)
                    .values(&new_account_txo_status)
                    .execute(conn)?;
            }
            Ok(())
        })?;

        Ok((
            outlay_receiver,
            txo_id.to_string(),
            total_output_value as i64,
            transaction_txo_type.to_string(),
        ))
    }

    fn update_received(
        &self,
        account_id_hex: &str,
        received_subaddress_index: Option<i64>,
        received_key_image: Option<KeyImage>,
        received_block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError> {
        conn.transaction::<(), WalletDbError, _>(|| {
            // get the type of this TXO
            let account_txo_status =
                AccountTxoStatus::get(account_id_hex, &self.txo_id_hex, &conn)?;

            // For TXOs that we sent previously, they are either change, or we sent to ourselves
            // for some other reason. Their status will be "secreted" in either case.
            match account_txo_status.txo_type.as_str() {
                TXO_MINTED => {
                    // Update received block height and subaddress index
                    self.update_to_spendable(
                        received_subaddress_index,
                        received_key_image,
                        received_block_height,
                        &conn,
                    )?;

                    // Update the status to unspent - all TXOs set lifecycle to unspent when first received
                    account_txo_status.set_unspent(&conn)?;
                }
                TXO_RECEIVED => {
                    // If the existing Txo subaddress is null and we have the received subaddress
                    // now, then we want to update to received subaddress. Otherwise, it will remain orphaned.
                    // Do not update to unspent, because this Txo may have already been processed and is
                    // annotated correctly if spent.
                    if received_subaddress_index.is_some() {
                        self.update_to_spendable(
                            received_subaddress_index,
                            received_key_image,
                            received_block_height,
                            &conn,
                        )?;
                    }
                }
                _ => {
                    panic!("New txo_type must be handled");
                }
            }

            // If this Txo was previously orphaned, we can now update it, and make it spendable
            if account_txo_status.txo_status == TXO_ORPHANED {
                self.update_to_spendable(
                    received_subaddress_index,
                    received_key_image,
                    received_block_height,
                    &conn,
                )?;
                account_txo_status.set_unspent(conn)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn update_to_spendable(
        &self,
        received_subaddress_index: Option<i64>,
        received_key_image: Option<KeyImage>,
        block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError> {
        use crate::db::schema::txos::{key_image, received_block_height, subaddress_index};

        // Verify that we have a subaddress, otherwise this transaction will be
        // unspendable.
        if received_subaddress_index.is_none() || received_key_image.is_none() {
            return Err(WalletDbError::NullSubaddressOnReceived);
        }

        let encoded_key_image = received_key_image.map(|k| mc_util_serial::encode(&k));

        diesel::update(self)
            .set((
                received_block_height.eq(Some(block_height)),
                subaddress_index.eq(received_subaddress_index),
                key_image.eq(encoded_key_image),
            ))
            .execute(conn)?;
        Ok(())
    }

    fn update_to_pending(
        txo_id: &TxoID,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(), WalletDbError> {
        use crate::db::schema::account_txo_statuses::dsl::account_txo_statuses;

        conn.transaction::<(), WalletDbError, _>(|| {
            // Find the account associated with this Txo.
            // Note: We should only be calling update_to_pending on inputs, which we had to own to spend.
            let account = Account::get_by_txo_id(&txo_id.to_string(), conn)?;

            // Update the status to pending.
            diesel::update(
                account_txo_statuses.find((&account.account_id_hex, &txo_id.to_string())),
            )
            .set(crate::db::schema::account_txo_statuses::txo_status.eq(TXO_PENDING.to_string()))
            .execute(conn)?;
            Ok(())
        })?;
        Ok(())
    }

    fn list_for_account(
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<(Txo, AccountTxoStatus)>, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let results: Vec<(Txo, AccountTxoStatus)> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))),
            )
            .select((txos::all_columns, account_txo_statuses::all_columns))
            .load(conn)?;
        Ok(results)
    }

    fn list_by_status(
        account_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<HashMap<String, Vec<Txo>>, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let unspent: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_UNSPENT))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        let pending: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_PENDING))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        let spent: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_SPENT))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        let secreted: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_SECRETED))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        let orphaned: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_ORPHANED))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        let results = HashMap::from_iter(vec![
            (TXO_UNSPENT.to_string(), unspent),
            (TXO_PENDING.to_string(), pending),
            (TXO_SPENT.to_string(), spent),
            (TXO_SECRETED.to_string(), secreted),
            (TXO_ORPHANED.to_string(), orphaned),
        ]);
        Ok(results)
    }

    fn get(
        account_id_hex: &AccountID,
        txo_id_hex: &str,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<(Txo, AccountTxoStatus, Option<AssignedSubaddress>), WalletDbError> {
        use crate::db::schema::txos::dsl::{txo_id_hex as dsl_txo_id_hex, txos};

        let txo: Txo = match txos
            .filter(dsl_txo_id_hex.eq(txo_id_hex))
            .get_result::<Txo>(conn)
        {
            Ok(t) => t,
            // Match on NotFound to get a more informative NotFound Error
            Err(diesel::result::Error::NotFound) => {
                return Err(WalletDbError::TxoNotFound(txo_id_hex.to_string()));
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        let account_txo_status: AccountTxoStatus =
            match AccountTxoStatus::get(&account_id_hex.to_string(), txo_id_hex, conn) {
                Ok(txo_status) => txo_status,
                Err(WalletDbError::AccountTxoStatusNotFound(_)) => {
                    // In this case, the Txo exists, but for some other account.
                    return Err(WalletDbError::TxoExistsForAnotherAccount(
                        txo_id_hex.to_string(),
                    ));
                }
                Err(e) => {
                    return Err(e);
                }
            };

        // Get the subaddress details if assigned
        let assigned_subaddress = if let Some(subaddress_index) = txo.subaddress_index {
            let account: Account = Account::get(account_id_hex, conn)?;
            let account_key: AccountKey = mc_util_serial::decode(&account.encrypted_account_key)?;
            let subaddress = account_key.subaddress(subaddress_index as u64);
            let subaddress_b58 = b58_encode(&subaddress)?;

            Some(AssignedSubaddress::get(&subaddress_b58, conn)?)
        } else {
            None
        };

        Ok((txo, account_txo_status, assigned_subaddress))
    }

    fn select_by_id(
        txo_ids: &[String],
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<(Txo, AccountTxoStatus)>, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let txos: Vec<(Txo, AccountTxoStatus)> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(txos::txo_id_hex.eq_any(txo_ids))),
            )
            .select((txos::all_columns, account_txo_statuses::all_columns))
            .load(conn)?;
        Ok(txos)
    }

    fn are_all_spent(
        txo_ids: &[String],
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let txos: i64 = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(txos::txo_id_hex.eq_any(txo_ids))
                    .and(account_txo_statuses::txo_status.eq(TXO_SPENT))),
            )
            .select(diesel::dsl::count(txos::txo_id_hex))
            .first(conn)?;

        Ok(txos == txo_ids.len() as i64)
    }

    fn any_failed(
        txo_ids: &[String],
        block_height: i64,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let txos: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(txos::txo_id_hex.eq_any(txo_ids))
                    .and(account_txo_statuses::txo_status.eq_any(vec![TXO_UNSPENT, TXO_PENDING]))
                    .and(txos::pending_tombstone_block_height.lt(Some(block_height)))),
            )
            .select(txos::all_columns)
            .load(conn)?;

        // Report true if any txos have expired
        Ok(!txos.is_empty())
    }

    fn select_unspent_txos_for_value(
        account_id_hex: &str,
        target_value: u64,
        max_spendable_value: Option<i64>,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<Vec<Txo>, WalletDbError> {
        use crate::db::schema::account_txo_statuses;
        use crate::db::schema::txos;

        let mut spendable_txos: Vec<Txo> = txos::table
            .inner_join(
                account_txo_statuses::table.on(txos::txo_id_hex
                    .eq(account_txo_statuses::txo_id_hex)
                    .and(account_txo_statuses::account_id_hex.eq(account_id_hex))
                    .and(account_txo_statuses::txo_status.eq(TXO_UNSPENT))
                    .and(txos::subaddress_index.is_not_null())
                    .and(txos::key_image.is_not_null()) // Could technically recreate with subaddress
                    .and(txos::value.le(max_spendable_value.unwrap_or(i64::MAX)))),
            )
            .select(txos::all_columns)
            .order_by(txos::value.desc())
            .load(conn)?;

        if spendable_txos.is_empty() {
            return Err(WalletDbError::NoSpendableTxos);
        }

        // The maximum spendable is limited by the maximal number of inputs we can use. Since
        // the txos are sorted by decreasing value, this is the maximum value we can possibly spend
        // in one transaction.
        let max_spendable_in_wallet = spendable_txos
            .iter()
            .take(MAX_INPUTS as usize)
            .map(|utxo| utxo.value as u64)
            .sum();
        if target_value > max_spendable_in_wallet {
            // See if we merged the UTXOs we would be able to spend this amount.
            let total_unspent_value_in_wallet: u64 =
                spendable_txos.iter().map(|utxo| utxo.value as u64).sum();
            if total_unspent_value_in_wallet >= target_value {
                return Err(WalletDbError::InsufficientFundsFragmentedTxos);
            } else {
                return Err(WalletDbError::InsufficientFundsUnderMaxSpendable(format!(
                    "Max spendable value in wallet: {:?}, but target value: {:?}",
                    max_spendable_in_wallet, target_value
                )));
            }
        }

        // Select the actual Txos to spend. We want to opportunistically fill up the input slots
        // with dust, from any subaddress, so we take from the back of the Txo vec. This is
        // a knapsack problem, and the selection could be improved. For now, we simply move the
        // window of MAX_INPUTS up from the back of the sorted vector until we have a window with
        // a large enough sum.
        let mut selected_utxos: Vec<Txo> = Vec::new();
        let mut total: u64 = 0;
        loop {
            if total >= target_value {
                break;
            }

            // Grab the next (smallest) utxo, in order to opportunistically sweep up dust
            let next_utxo = spendable_txos.pop().ok_or_else(|| {
                WalletDbError::InsufficientFunds(format!(
                    "Not enough Txos to sum to target value: {:?}",
                    target_value
                ))
            })?;
            selected_utxos.push(next_utxo.clone());
            total += next_utxo.value as u64;

            // Cap at maximum allowed inputs.
            if selected_utxos.len() > MAX_INPUTS as usize {
                // Remove the lowest utxo.
                selected_utxos.remove(0);
            }
        }

        if selected_utxos.is_empty() || selected_utxos.len() > MAX_INPUTS as usize {
            return Err(WalletDbError::InsufficientFunds(
                "Logic error. Could not select Txos despite having sufficient funds".to_string(),
            ));
        }

        Ok(selected_utxos)
    }

    fn verify_proof(
        account_id: &AccountID,
        txo_id: &str,
        proof: &TxOutConfirmationNumber,
        conn: &PooledConnection<ConnectionManager<SqliteConnection>>,
    ) -> Result<bool, WalletDbError> {
        let (txo, _txo_status, _opt_assigned_subaddress) = Txo::get(account_id, txo_id, conn)?;
        let public_key: RistrettoPublic = mc_util_serial::decode(&txo.public_key)?;
        let account = Account::get(account_id, conn)?;
        let account_key: AccountKey = mc_util_serial::decode(&account.encrypted_account_key)?;
        Ok(proof.validate(&public_key, account_key.view_private_key()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::{
            account::{AccountID, AccountModel},
            models::{Account, TransactionLog},
            transaction_log::TransactionLogModel,
        },
        service::{
            sync::{sync_account, SyncThread},
            transaction_builder::WalletTransactionBuilder,
        },
        test_utils::{
            add_block_with_tx_proposal, create_test_minted_and_change_txos,
            create_test_received_txo, get_test_ledger, random_account_with_seed_values,
            WalletDbTestContext, MOB,
        },
    };
    use mc_account_keys::{AccountKey, RootIdentity};
    use mc_common::{
        logger::{test_with_logger, Logger},
        HashSet,
    };
    use mc_crypto_rand::RngCore;
    use mc_fog_report_validation::MockFogPubkeyResolver;
    use mc_ledger_db::Ledger;
    use mc_transaction_core::constants::MINIMUM_FEE;
    use mc_util_from_random::FromRandom;
    use rand::{rngs::StdRng, SeedableRng};
    use std::{iter::FromIterator, sync::Arc};

    #[test_with_logger]
    fn test_received_tx_lifecycle(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);

        let db_test_context = WalletDbTestContext::default();
        let wallet_db = db_test_context.get_db_instance(logger);

        let account_key = AccountKey::random(&mut rng);
        let (account_id_hex, _public_address_b58) = Account::create(
            &account_key,
            0,
            1,
            2,
            0,
            1,
            None,
            "Alice's Main Account",
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Create TXO for the account
        let (txo_hex, txo, key_image) =
            create_test_received_txo(&account_key, 0, 10, 144, &mut rng, &wallet_db);

        let txos =
            Txo::list_for_account(&account_id_hex.to_string(), &wallet_db.get_conn().unwrap())
                .unwrap();
        assert_eq!(txos.len(), 1);

        let expected_txo = Txo {
            id: 1,
            txo_id_hex: txo_hex.clone(),
            value: 10,
            target_key: mc_util_serial::encode(&txo.target_key),
            public_key: mc_util_serial::encode(&txo.public_key),
            e_fog_hint: mc_util_serial::encode(&txo.e_fog_hint),
            txo: mc_util_serial::encode(&txo),
            subaddress_index: Some(0),
            key_image: Some(mc_util_serial::encode(&key_image)),
            received_block_height: Some(144),
            pending_tombstone_block_height: None,
            spent_block_height: None,
            proof: None,
        };
        // Verify that the statuses table was updated correctly
        let expected_txo_status = AccountTxoStatus {
            account_id_hex: account_id_hex.to_string(),
            txo_id_hex: txo_hex,
            txo_status: TXO_UNSPENT.to_string(),
            txo_type: TXO_RECEIVED.to_string(),
        };
        assert_eq!(txos[0].0, expected_txo);
        assert_eq!(txos[0].1, expected_txo_status);

        // Verify that the status filter works as well
        let balances =
            Txo::list_by_status(&account_id_hex.to_string(), &wallet_db.get_conn().unwrap())
                .unwrap();
        assert_eq!(balances[TXO_UNSPENT].len(), 1);

        // Now we'll "spend" the TXO
        // FIXME: TODO: construct transaction proposal to spend it, maybe needs a helper in test_utils
        // self.update_submitted_transaction(tx_proposal)?;

        // Now we'll process the ledger and verify that the TXO was spent
        let spent_block_height = 365;

        let account = Account::get(&account_id_hex, &wallet_db.get_conn().unwrap()).unwrap();
        account
            .update_spent_and_increment_next_block(
                spent_block_height,
                vec![key_image],
                &wallet_db.get_conn().unwrap(),
            )
            .unwrap();

        let txos =
            Txo::list_for_account(&account_id_hex.to_string(), &wallet_db.get_conn().unwrap())
                .unwrap();
        assert_eq!(txos.len(), 1);
        assert_eq!(
            txos[0].0.spent_block_height.unwrap(),
            spent_block_height as i64
        );
        assert_eq!(txos[0].1.txo_status, TXO_SPENT.to_string());

        // Verify that the next block height is + 1
        let account = Account::get(&account_id_hex, &wallet_db.get_conn().unwrap()).unwrap();
        assert_eq!(account.next_block, spent_block_height + 1);

        // Verify that there are no unspent txos
        let balances =
            Txo::list_by_status(&account_id_hex.to_string(), &wallet_db.get_conn().unwrap())
                .unwrap();
        assert!(balances[TXO_UNSPENT].is_empty());
    }

    #[test_with_logger]
    fn test_select_txos_for_value(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);

        let db_test_context = WalletDbTestContext::default();
        let wallet_db = db_test_context.get_db_instance(logger);

        let account_key = AccountKey::random(&mut rng);
        let (account_id_hex, _public_address_b58) = Account::create(
            &account_key,
            0,
            1,
            2,
            0,
            1,
            None,
            "Alice's Main Account",
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Create some TXOs for the account
        // [100, 200, 300, ... 2000]
        for i in 1..20 {
            let (_txo_hex, _txo, _key_image) = create_test_received_txo(
                &account_key,
                0,
                (100 * MOB * i) as u64, // 100.0 MOB * i
                (144 + i) as u64,
                &mut rng,
                &wallet_db,
            );
        }

        // Greedily take smallest to exact value
        let txos_for_value = Txo::select_unspent_txos_for_value(
            &account_id_hex.to_string(),
            300 * MOB as u64,
            None,
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        let result_set = HashSet::from_iter(txos_for_value.iter().map(|t| t.value));
        assert_eq!(
            result_set,
            HashSet::<i64>::from_iter(vec![100 * MOB, 200 * MOB])
        );

        // Once we include the fee, we need another txo
        let txos_for_value = Txo::select_unspent_txos_for_value(
            &account_id_hex.to_string(),
            300 * MOB as u64 + MINIMUM_FEE,
            None,
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        let result_set = HashSet::from_iter(txos_for_value.iter().map(|t| t.value));
        assert_eq!(
            result_set,
            HashSet::<i64>::from_iter(vec![100 * MOB, 200 * MOB, 300 * MOB])
        );

        // Setting max spendable value gives us insufficient funds - only allows 100
        let res = Txo::select_unspent_txos_for_value(
            &account_id_hex.to_string(),
            300 * MOB as u64 + MINIMUM_FEE,
            Some(200 * MOB),
            &wallet_db.get_conn().unwrap(),
        );
        match res {
            Err(WalletDbError::InsufficientFundsUnderMaxSpendable(_)) => {}
            Ok(_) => panic!("Should error with InsufficientFundsUnderMaxSpendable"),
            Err(_) => panic!("Should error with InsufficientFundsUnderMaxSpendable"),
        }

        // sum(300..1800) to get a window where we had to increase past the smallest txos,
        // and also fill up all 16 input slots.
        let txos_for_value = Txo::select_unspent_txos_for_value(
            &account_id_hex.to_string(),
            16800 * MOB as u64,
            None,
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        let result_set = HashSet::from_iter(txos_for_value.iter().map(|t| t.value));
        assert_eq!(
            result_set,
            HashSet::<i64>::from_iter(vec![
                300 * MOB,
                400 * MOB,
                500 * MOB,
                600 * MOB,
                700 * MOB,
                800 * MOB,
                900 * MOB,
                1000 * MOB,
                1100 * MOB,
                1200 * MOB,
                1300 * MOB,
                1400 * MOB,
                1500 * MOB,
                1600 * MOB,
                1700 * MOB,
                1800 * MOB
            ])
        );
    }

    #[test_with_logger]
    fn test_select_txos_fragmented(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);

        let db_test_context = WalletDbTestContext::default();
        let wallet_db = db_test_context.get_db_instance(logger);

        let account_key = AccountKey::random(&mut rng);
        let (account_id_hex, _public_address_b58) = Account::create(
            &account_key,
            0,
            1,
            2,
            0,
            1,
            None,
            "Alice's Main Account",
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Create some TXOs for the account. Total value is 2000, but max can spend is 1600
        // [100, 100, ... 100]
        for i in 1..20 {
            let (_txo_hex, _txo, _key_image) = create_test_received_txo(
                &account_key,
                0,
                (100 * MOB) as u64,
                (144 + i) as u64,
                &mut rng,
                &wallet_db,
            );
        }

        let res = Txo::select_unspent_txos_for_value(
            &account_id_hex.to_string(), // FIXME: WS-11 - take AccountID
            1800 * MOB as u64,
            None,
            &wallet_db.get_conn().unwrap(),
        );
        match res {
            Err(WalletDbError::InsufficientFundsFragmentedTxos) => {}
            Ok(_) => panic!("Should error with InsufficientFundsFragmentedTxos"),
            Err(e) => panic!(
                "Should error with InsufficientFundsFragmentedTxos but got {:?}",
                e
            ),
        }
    }

    #[test_with_logger]
    fn test_create_minted(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);

        let src_account = AccountKey::from(&RootIdentity::from_random(&mut rng));

        // Seed our ledger with some utxos for the src_account
        let known_recipients = vec![src_account.subaddress(0)];
        let ledger_db = get_test_ledger(5, &known_recipients, 12, &mut rng);

        let db_test_context = WalletDbTestContext::default();
        let wallet_db = db_test_context.get_db_instance(logger.clone());

        Account::create(
            &src_account,
            0,
            1,
            2,
            0,
            0,
            None,
            "",
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Process the txos in the ledger into the DB
        sync_account(
            &ledger_db,
            &wallet_db,
            &AccountID::from(&src_account).to_string(),
            &logger,
        )
        .unwrap();

        let recipient =
            AccountKey::from(&RootIdentity::from_random(&mut rng)).subaddress(rng.next_u64());

        let (recipient_opt, txo_id, value, transaction_txo_type) =
            create_test_minted_and_change_txos(
                src_account.clone(),
                recipient,
                1 * MOB as u64,
                wallet_db.clone(),
                ledger_db,
                logger,
            );

        assert!(recipient_opt.is_some());
        assert_eq!(value, 1 * MOB as i64);
        assert_eq!(transaction_txo_type, OUTPUT);
        let (minted_txo, minted_account_txo_status, minted_assigned_subaddress) = Txo::get(
            &AccountID::from(&src_account),
            &txo_id,
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        assert_eq!(minted_txo.value, value);
        assert_eq!(minted_account_txo_status.txo_status, TXO_SECRETED);
        assert!(minted_assigned_subaddress.is_none());
    }

    // Test that proof verifies
    #[test_with_logger]
    fn test_verify_proof(logger: Logger) {
        let mut rng: StdRng = SeedableRng::from_seed([20u8; 32]);

        let db_test_context = WalletDbTestContext::default();
        let wallet_db = db_test_context.get_db_instance(logger.clone());
        let known_recipients: Vec<PublicAddress> = Vec::new();
        let mut ledger_db = get_test_ledger(5, &known_recipients, 12, &mut rng);

        // The account which will receive the Txo
        let recipient_account_key = AccountKey::random(&mut rng);
        Account::create(
            &recipient_account_key,
            0,
            1,
            2,
            0,
            0,
            None,
            "",
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Start sync thread
        let _sync_thread =
            SyncThread::start(ledger_db.clone(), wallet_db.clone(), None, logger.clone());

        let sender_account_key = random_account_with_seed_values(
            &wallet_db,
            &mut ledger_db,
            &vec![70 * MOB as u64, 80 * MOB as u64, 90 * MOB as u64],
            &mut rng,
        );

        // Create TxProposal from the sender account, which contains the Confirmation Number
        let mut builder: WalletTransactionBuilder<MockFogPubkeyResolver> =
            WalletTransactionBuilder::new(
                AccountID::from(&sender_account_key).to_string(),
                wallet_db.clone(),
                ledger_db.clone(),
                Some(Arc::new(MockFogPubkeyResolver::new())),
                logger.clone(),
            );
        builder
            .add_recipient(recipient_account_key.default_subaddress(), 50 * MOB as u64)
            .unwrap();
        builder.select_txos(None).unwrap();
        builder.set_tombstone(0).unwrap();
        let proposal = builder.build().unwrap();

        // Let's log this submitted Tx for the sender, which will create_minted for the sent Txo
        let tx_id = TransactionLog::log_submitted(
            proposal.clone(),
            ledger_db.num_blocks().unwrap(),
            "".to_string(),
            Some(&AccountID::from(&sender_account_key).to_string()),
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // Now we need to let this txo hit the ledger, which will update sender and receiver
        add_block_with_tx_proposal(&mut ledger_db, proposal.clone());

        // Now let our sync thread catch up for both sender and receiver
        std::thread::sleep(std::time::Duration::from_secs(4));

        // Then let's make sure we received the Txo on the recipient account
        let txos = Txo::list_for_account(
            &AccountID::from(&recipient_account_key).to_string(),
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        assert_eq!(txos.len(), 1);

        let (received_txo, _txo_status) = txos[0].clone();

        // Note: Because this txo is both received and sent, between two different accounts,
        // its proof does get updated. Typically, received txos have None for the proof.
        assert!(received_txo.proof.is_some());

        // Get the txo from the sent perspective
        let sender_txos = Txo::list_for_account(
            &AccountID::from(&sender_account_key).to_string(),
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // We seeded with 3 received (70, 80, 90), we have a change txo, and a secreted Txo (50)
        assert_eq!(sender_txos.len(), 5);

        // Get the associated Txos with the transaction log
        let tx_log = TransactionLog::get(&tx_id, &wallet_db.get_conn().unwrap()).unwrap();
        let associated = tx_log
            .get_associated_txos(&wallet_db.get_conn().unwrap())
            .unwrap();
        let sent_outputs = associated.outputs;
        assert_eq!(sent_outputs.len(), 1);
        let (sent_txo, _sent_txo_status, _opt_assigned_subaddress) = Txo::get(
            &AccountID::from(&sender_account_key),
            &sent_outputs[0],
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();

        // These two txos should actually be the same txo, and the account_txo_status is what
        // differentiates them.
        assert_eq!(sent_txo, received_txo);

        assert!(sent_txo.proof.is_some());
        let proof: TxOutConfirmationNumber =
            mc_util_serial::decode(&sent_txo.proof.unwrap()).unwrap();
        let verified = Txo::verify_proof(
            &AccountID::from(&recipient_account_key),
            &received_txo.txo_id_hex,
            &proof,
            &wallet_db.get_conn().unwrap(),
        )
        .unwrap();
        assert!(verified);
    }

    // FIXME: once we have create_minted, then select_txos test with no spendable
    // FIXME: test update txo after tombstone block is exceeded
    // FIXME: test update txo after it has landed via key_image update
    // FIXME: test any_failed and are_all_spent
    // FIXME: test max_spendable
    // FIXME: test for selecting utxos from multiple subaddresses in one account
}
