use rusqlite::TransactionBehavior;

use crate::{DeviceRecord, Store, StoreError, StoreResult, database};

impl Store {
    /// Lists device IDs, labels, and revocation state for the local administration CLI.
    pub fn list_devices(&self) -> StoreResult<Vec<DeviceRecord>> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT pc_id, label, revoked_at IS NULL FROM devices ORDER BY created_at, pc_id",
            )
            .map_err(database::sql_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(DeviceRecord {
                    pc_id: row.get(0)?,
                    label: row.get(1)?,
                    active: row.get(2)?,
                })
            })
            .map_err(database::sql_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(database::sql_error)
    }

    /// Revokes a device while holding the same SQLite writer lock as visible device mutations.
    pub fn revoke_device(&self, pc_id: &str) -> StoreResult<()> {
        if !crate::auth::is_lower_hex(pc_id, 32) {
            return Err(StoreError::NotFound);
        }
        let now = self.now()?;
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database::sql_error)?;
        let exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM devices WHERE pc_id = ?1)",
                [pc_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database::sql_error)?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        transaction
            .execute(
                "UPDATE devices SET revoked_at = COALESCE(revoked_at, ?1) WHERE pc_id = ?2",
                (&now, pc_id),
            )
            .map_err(database::sql_error)?;
        transaction.commit().map_err(database::sql_error)
    }
}
