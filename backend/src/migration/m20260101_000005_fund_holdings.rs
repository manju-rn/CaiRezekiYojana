//! The portfolio behind each mutual fund, for the look-through view.
//!
//! 1. `fund_holding`: one row per security in a fund's latest disclosed
//!    portfolio, with the share of the fund it makes up. Filled by a
//!    holdings feed on refresh and replaced wholesale each time - a
//!    disclosure is a snapshot, not a series.
//!
//! 2. `instrument.holdings_ref`: the feed's identifier for the fund, kept
//!    once a match has been confirmed by scheme code or ISIN so later
//!    refreshes go straight to the portfolio instead of searching again.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(Instrument::Table)
                .add_column(ColumnDef::new(Instrument::HoldingsRef).string_len(160).null())
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(FundHolding::Table)
                .if_not_exists()
                .col(ColumnDef::new(FundHolding::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(FundHolding::InstrumentId).integer().not_null())
                .col(ColumnDef::new(FundHolding::AsOf).date().not_null())
                .col(ColumnDef::new(FundHolding::Name).string_len(240).not_null())
                .col(ColumnDef::new(FundHolding::Nature).string_len(16).not_null())
                .col(ColumnDef::new(FundHolding::InstrumentType).string_len(80).null())
                .col(ColumnDef::new(FundHolding::Sector).string_len(80).null())
                .col(ColumnDef::new(FundHolding::WeightPct).double().not_null())
                .col(ColumnDef::new(FundHolding::MarketValue).double().null())
                .col(ColumnDef::new(FundHolding::SecurityRef).string_len(120).null())
                .col(ColumnDef::new(FundHolding::Source).string_len(24).not_null())
                .col(ColumnDef::new(FundHolding::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_fund_holding_instrument")
                        .from(FundHolding::Table, FundHolding::InstrumentId)
                        .to(Instrument::Table, Instrument::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("ix_fund_holding_instrument")
                .table(FundHolding::Table)
                .col(FundHolding::InstrumentId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(FundHolding::Table).if_exists().to_owned())
            .await?;
        m.alter_table(
            Table::alter()
                .table(Instrument::Table)
                .drop_column(Instrument::HoldingsRef)
                .to_owned(),
        )
        .await
    }
}
