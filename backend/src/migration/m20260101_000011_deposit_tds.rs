//! Tax withheld at source on deposit interest.
//!
//! An Indian bank deducts TDS from a cumulative deposit's interest each
//! 31 March, out of the deposit itself - 31.2% on an NRO deposit - so what
//! is in it falls behind the gross figure the bank quotes for maturity.
//! `tds_pct` records the rate; `NULL` means nothing is withheld.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(Deposit::Table)
                .add_column(ColumnDef::new(Deposit::TdsPct).double().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(Table::alter().table(Deposit::Table).drop_column(Deposit::TdsPct).to_owned()).await
    }
}
