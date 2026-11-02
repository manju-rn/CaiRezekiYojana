//! Transfer fees by amount, for pairs that convert at what Wise charges.
//!
//! 1. `fx_fee`: the fee Wise quotes for a pair at a handful of amounts,
//!    refreshed with the rates. Conversions interpolate between them, since
//!    the fee depends on how much is converted.
//!
//! 2. Every pair still on the untouched default - no margin, no manual rate -
//!    moves to `WISE`, so figures reflect what converting would really cost
//!    once the fees have been fetched. A pair set to anything else keeps it.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(FxFee::Table)
                .if_not_exists()
                .col(ColumnDef::new(FxFee::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(FxFee::BaseCode).string_len(3).not_null())
                .col(ColumnDef::new(FxFee::QuoteCode).string_len(3).not_null())
                .col(ColumnDef::new(FxFee::Provider).string_len(24).not_null())
                .col(ColumnDef::new(FxFee::Route).string_len(48).not_null())
                .col(ColumnDef::new(FxFee::Points).text().not_null())
                .col(ColumnDef::new(FxFee::MidRate).double().null())
                .col(ColumnDef::new(FxFee::FetchedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("ux_fx_fee_pair")
                .table(FxFee::Table)
                .col(FxFee::BaseCode)
                .col(FxFee::QuoteCode)
                .unique()
                .to_owned(),
        )
        .await?;

        m.exec_stmt(
            Query::update()
                .table(FxSetting::Table)
                .value(FxSetting::MarginMode, "WISE")
                .and_where(Expr::col(FxSetting::MarginMode).eq("NONE"))
                .and_where(Expr::col(FxSetting::UseManual).eq(false))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.exec_stmt(
            Query::update()
                .table(FxSetting::Table)
                .value(FxSetting::MarginMode, "NONE")
                .and_where(Expr::col(FxSetting::MarginMode).eq("WISE"))
                .to_owned(),
        )
        .await?;
        m.drop_table(Table::drop().table(FxFee::Table).if_exists().to_owned()).await
    }
}
