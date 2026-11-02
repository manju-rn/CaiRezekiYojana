use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // ---- reference data -------------------------------------------------
        m.create_table(
            Table::create()
                .table(AppSetting::Table)
                .if_not_exists()
                .col(ColumnDef::new(AppSetting::Key).string_len(64).not_null().primary_key())
                .col(ColumnDef::new(AppSetting::Value).text().not_null())
                .col(ColumnDef::new(AppSetting::UpdatedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(Currency::Table)
                .if_not_exists()
                .col(ColumnDef::new(Currency::Code).string_len(3).not_null().primary_key())
                .col(ColumnDef::new(Currency::Name).string_len(80).not_null())
                .col(ColumnDef::new(Currency::Symbol).string_len(8).not_null())
                .col(ColumnDef::new(Currency::MinorUnits).integer().not_null().default(2))
                .col(ColumnDef::new(Currency::IsActive).boolean().not_null().default(true))
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(Exchange::Table)
                .if_not_exists()
                .col(ColumnDef::new(Exchange::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Exchange::Code).string_len(24).not_null().unique_key())
                .col(ColumnDef::new(Exchange::Name).string_len(120).not_null())
                .col(ColumnDef::new(Exchange::Country).string_len(2).not_null())
                .col(ColumnDef::new(Exchange::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(Exchange::Mic).string_len(8).null())
                .col(ColumnDef::new(Exchange::QuoteSuffix).string_len(12).null())
                .col(ColumnDef::new(Exchange::Timezone).string_len(64).null())
                .to_owned(),
        )
        .await?;

        // ---- accounts -------------------------------------------------------
        m.create_table(
            Table::create()
                .table(Account::Table)
                .if_not_exists()
                .col(ColumnDef::new(Account::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Account::Name).string_len(120).not_null())
                .col(ColumnDef::new(Account::Kind).string_len(32).not_null())
                .col(ColumnDef::new(Account::Country).string_len(2).not_null())
                .col(ColumnDef::new(Account::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(Account::Institution).string_len(120).null())
                .col(ColumnDef::new(Account::AccountRef).string_len(64).null())
                .col(ColumnDef::new(Account::OpenedOn).date().null())
                .col(ColumnDef::new(Account::IsActive).boolean().not_null().default(true))
                .col(ColumnDef::new(Account::Notes).text().null())
                .col(ColumnDef::new(Account::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(Account::UpdatedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;

        // ---- instruments ----------------------------------------------------
        m.create_table(
            Table::create()
                .table(Instrument::Table)
                .if_not_exists()
                .col(ColumnDef::new(Instrument::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Instrument::Kind).string_len(32).not_null())
                .col(ColumnDef::new(Instrument::Symbol).string_len(64).not_null())
                .col(ColumnDef::new(Instrument::Name).string_len(240).not_null())
                .col(ColumnDef::new(Instrument::Isin).string_len(16).null())
                .col(ColumnDef::new(Instrument::ExchangeId).integer().null())
                .col(ColumnDef::new(Instrument::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(Instrument::Country).string_len(2).null())
                .col(ColumnDef::new(Instrument::QuoteSymbol).string_len(64).null())
                .col(ColumnDef::new(Instrument::AmfiSchemeCode).string_len(24).null())
                .col(ColumnDef::new(Instrument::Sector).string_len(80).null())
                .col(ColumnDef::new(Instrument::IsActive).boolean().not_null().default(true))
                .col(ColumnDef::new(Instrument::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(Instrument::UpdatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_instrument_exchange")
                        .from(Instrument::Table, Instrument::ExchangeId)
                        .to(Exchange::Table, Exchange::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

        // An instrument is identified by (symbol, exchange-or-none). Two rows
        // may share a symbol across exchanges, e.g. a dual-listed ETF.
        m.create_index(
            Index::create()
                .name("ux_instrument_symbol_exchange")
                .table(Instrument::Table)
                .col(Instrument::Symbol)
                .col(Instrument::ExchangeId)
                .unique()
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_instrument_isin")
                .table(Instrument::Table)
                .col(Instrument::Isin)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_instrument_amfi")
                .table(Instrument::Table)
                .col(Instrument::AmfiSchemeCode)
                .to_owned(),
        )
        .await?;

        // ---- import batches (before txn: txn references it) -----------------
        m.create_table(
            Table::create()
                .table(ImportBatch::Table)
                .if_not_exists()
                .col(ColumnDef::new(ImportBatch::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(ImportBatch::Kind).string_len(32).not_null())
                .col(ColumnDef::new(ImportBatch::Filename).string_len(255).not_null())
                .col(ColumnDef::new(ImportBatch::Status).string_len(16).not_null())
                .col(ColumnDef::new(ImportBatch::RowCount).integer().not_null().default(0))
                .col(ColumnDef::new(ImportBatch::OkCount).integer().not_null().default(0))
                .col(ColumnDef::new(ImportBatch::ErrorCount).integer().not_null().default(0))
                .col(ColumnDef::new(ImportBatch::SkippedCount).integer().not_null().default(0))
                .col(ColumnDef::new(ImportBatch::ErrorsJson).text().null())
                .col(ColumnDef::new(ImportBatch::Message).text().null())
                .col(ColumnDef::new(ImportBatch::CreatedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;

        // ---- transactions ---------------------------------------------------
        m.create_table(
            Table::create()
                .table(Txn::Table)
                .if_not_exists()
                .col(ColumnDef::new(Txn::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Txn::AccountId).integer().not_null())
                .col(ColumnDef::new(Txn::InstrumentId).integer().not_null())
                .col(ColumnDef::new(Txn::TradeDate).date().not_null())
                .col(ColumnDef::new(Txn::Kind).string_len(24).not_null())
                .col(ColumnDef::new(Txn::Quantity).double().not_null().default(0.0))
                .col(ColumnDef::new(Txn::Price).double().not_null().default(0.0))
                .col(ColumnDef::new(Txn::Fees).double().not_null().default(0.0))
                .col(ColumnDef::new(Txn::Taxes).double().not_null().default(0.0))
                .col(ColumnDef::new(Txn::NetAmount).double().not_null().default(0.0))
                .col(ColumnDef::new(Txn::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(Txn::FxRate).double().null())
                .col(ColumnDef::new(Txn::ExternalId).string_len(120).null())
                .col(ColumnDef::new(Txn::ImportBatchId).integer().null())
                .col(ColumnDef::new(Txn::Notes).text().null())
                .col(ColumnDef::new(Txn::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_txn_account")
                        .from(Txn::Table, Txn::AccountId)
                        .to(Account::Table, Account::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_txn_instrument")
                        .from(Txn::Table, Txn::InstrumentId)
                        .to(Instrument::Table, Instrument::Id)
                        .on_delete(ForeignKeyAction::Restrict),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_txn_import_batch")
                        .from(Txn::Table, Txn::ImportBatchId)
                        .to(ImportBatch::Table, ImportBatch::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

        m.create_index(
            Index::create()
                .name("ix_txn_account_date")
                .table(Txn::Table)
                .col(Txn::AccountId)
                .col(Txn::TradeDate)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_txn_instrument_date")
                .table(Txn::Table)
                .col(Txn::InstrumentId)
                .col(Txn::TradeDate)
                .to_owned(),
        )
        .await?;
        // Re-uploading the same broker statement must not double-count.
        m.create_index(
            Index::create()
                .name("ux_txn_account_external")
                .table(Txn::Table)
                .col(Txn::AccountId)
                .col(Txn::ExternalId)
                .unique()
                .to_owned(),
        )
        .await?;

        // ---- prices ---------------------------------------------------------
        m.create_table(
            Table::create()
                .table(PricePoint::Table)
                .if_not_exists()
                .col(ColumnDef::new(PricePoint::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(PricePoint::InstrumentId).integer().not_null())
                .col(ColumnDef::new(PricePoint::AsOf).date().not_null())
                .col(ColumnDef::new(PricePoint::Close).double().not_null())
                .col(ColumnDef::new(PricePoint::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(PricePoint::Source).string_len(24).not_null())
                .col(ColumnDef::new(PricePoint::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_price_instrument")
                        .from(PricePoint::Table, PricePoint::InstrumentId)
                        .to(Instrument::Table, Instrument::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_price_instrument_date")
                .table(PricePoint::Table)
                .col(PricePoint::InstrumentId)
                .col(PricePoint::AsOf)
                .unique()
                .to_owned(),
        )
        .await?;

        // ---- FX -------------------------------------------------------------
        m.create_table(
            Table::create()
                .table(FxRate::Table)
                .if_not_exists()
                .col(ColumnDef::new(FxRate::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(FxRate::BaseCode).string_len(3).not_null())
                .col(ColumnDef::new(FxRate::QuoteCode).string_len(3).not_null())
                .col(ColumnDef::new(FxRate::Rate).double().not_null())
                .col(ColumnDef::new(FxRate::AsOf).date().not_null())
                .col(ColumnDef::new(FxRate::Source).string_len(32).not_null())
                .col(ColumnDef::new(FxRate::CreatedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_fx_rate_pair_date_source")
                .table(FxRate::Table)
                .col(FxRate::BaseCode)
                .col(FxRate::QuoteCode)
                .col(FxRate::AsOf)
                .col(FxRate::Source)
                .unique()
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(FxSetting::Table)
                .if_not_exists()
                .col(ColumnDef::new(FxSetting::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(FxSetting::BaseCode).string_len(3).not_null())
                .col(ColumnDef::new(FxSetting::QuoteCode).string_len(3).not_null())
                .col(ColumnDef::new(FxSetting::Provider).string_len(32).not_null().default("default"))
                .col(ColumnDef::new(FxSetting::MarginMode).string_len(16).not_null().default("NONE"))
                .col(ColumnDef::new(FxSetting::MarginValue).double().not_null().default(0.0))
                .col(ColumnDef::new(FxSetting::ManualRate).double().null())
                .col(ColumnDef::new(FxSetting::UseManual).boolean().not_null().default(false))
                .col(ColumnDef::new(FxSetting::Note).text().null())
                .col(ColumnDef::new(FxSetting::UpdatedAt).date_time().not_null())
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_fx_setting_pair")
                .table(FxSetting::Table)
                .col(FxSetting::BaseCode)
                .col(FxSetting::QuoteCode)
                .unique()
                .to_owned(),
        )
        .await?;

        // ---- deposits & cash -------------------------------------------------
        m.create_table(
            Table::create()
                .table(Deposit::Table)
                .if_not_exists()
                .col(ColumnDef::new(Deposit::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Deposit::AccountId).integer().not_null())
                .col(ColumnDef::new(Deposit::Kind).string_len(32).not_null())
                .col(ColumnDef::new(Deposit::Institution).string_len(120).not_null())
                .col(ColumnDef::new(Deposit::Reference).string_len(64).null())
                .col(ColumnDef::new(Deposit::Principal).double().not_null())
                .col(ColumnDef::new(Deposit::InterestRate).double().not_null())
                .col(ColumnDef::new(Deposit::Compounding).string_len(16).not_null())
                .col(ColumnDef::new(Deposit::PayoutFrequency).string_len(16).not_null())
                .col(ColumnDef::new(Deposit::Installment).double().null())
                .col(ColumnDef::new(Deposit::StartDate).date().not_null())
                .col(ColumnDef::new(Deposit::MaturityDate).date().not_null())
                .col(ColumnDef::new(Deposit::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(Deposit::Country).string_len(2).not_null())
                .col(ColumnDef::new(Deposit::AutoRenew).boolean().not_null().default(false))
                .col(ColumnDef::new(Deposit::Status).string_len(16).not_null().default("ACTIVE"))
                .col(ColumnDef::new(Deposit::MaturityAmount).double().null())
                .col(ColumnDef::new(Deposit::Notes).text().null())
                .col(ColumnDef::new(Deposit::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(Deposit::UpdatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_deposit_account")
                        .from(Deposit::Table, Deposit::AccountId)
                        .to(Account::Table, Account::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_deposit_maturity")
                .table(Deposit::Table)
                .col(Deposit::MaturityDate)
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(CashBalance::Table)
                .if_not_exists()
                .col(ColumnDef::new(CashBalance::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(CashBalance::AccountId).integer().not_null())
                .col(ColumnDef::new(CashBalance::AsOf).date().not_null())
                .col(ColumnDef::new(CashBalance::Balance).double().not_null())
                .col(ColumnDef::new(CashBalance::CurrencyCode).string_len(3).not_null())
                .col(ColumnDef::new(CashBalance::Source).string_len(24).not_null().default("manual"))
                .col(ColumnDef::new(CashBalance::Notes).text().null())
                .col(ColumnDef::new(CashBalance::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_cash_account")
                        .from(CashBalance::Table, CashBalance::AccountId)
                        .to(Account::Table, Account::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_cash_account_date")
                .table(CashBalance::Table)
                .col(CashBalance::AccountId)
                .col(CashBalance::AsOf)
                .unique()
                .to_owned(),
        )
        .await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // Children first - FK order matters once enforcement is on.
        for stmt in [
            Table::drop().table(CashBalance::Table).if_exists().to_owned(),
            Table::drop().table(Deposit::Table).if_exists().to_owned(),
            Table::drop().table(FxSetting::Table).if_exists().to_owned(),
            Table::drop().table(FxRate::Table).if_exists().to_owned(),
            Table::drop().table(PricePoint::Table).if_exists().to_owned(),
            Table::drop().table(Txn::Table).if_exists().to_owned(),
            Table::drop().table(ImportBatch::Table).if_exists().to_owned(),
            Table::drop().table(Instrument::Table).if_exists().to_owned(),
            Table::drop().table(Account::Table).if_exists().to_owned(),
            Table::drop().table(Exchange::Table).if_exists().to_owned(),
            Table::drop().table(Currency::Table).if_exists().to_owned(),
            Table::drop().table(AppSetting::Table).if_exists().to_owned(),
        ] {
            m.drop_table(stmt).await?;
        }
        Ok(())
    }
}
