//! SeaORM entities.
//!
//! Enum-valued columns are `String` on purpose - see `crate::types`. Parse them
//! with `T::from_str(..)` at the edges of the API layer.
//!
//! Money and quantities are `f64`. Rationale in `docs/schema.md`: SQLite has no
//! NUMERIC type, and a portfolio tracker rounds for display anyway. The Postgres
//! migration path (also in that doc) swaps these for NUMERIC(20,6) when you need
//! ledger-grade exactness.

pub mod account;
pub mod ai_conversation;
pub mod ai_message;
pub mod ai_proposal;
pub mod ai_request;
pub mod ai_service_key;
pub mod api_key;
pub mod app_setting;
pub mod app_user;
pub mod cash_balance;
pub mod currency;
pub mod deposit;
pub mod exchange;
pub mod fx_rate;
pub mod fund_holding;
pub mod fx_fee;
pub mod fx_setting;
pub mod import_batch;
pub mod instrument;
pub mod plan_session;
pub mod plan_share;
pub mod plan_share_session;
pub mod price_point;
pub mod txn;
pub mod upload_audit;
pub mod user_identity;
pub mod user_session;
pub mod user_setting;
pub mod user_token;

pub mod prelude {
    pub use super::account::Entity as Account;
    pub use super::ai_conversation::Entity as AiConversation;
    pub use super::ai_message::Entity as AiMessage;
    pub use super::ai_proposal::Entity as AiProposal;
    pub use super::ai_request::Entity as AiRequest;
    pub use super::ai_service_key::Entity as AiServiceKey;
    pub use super::api_key::Entity as ApiKey;
    pub use super::app_setting::Entity as AppSetting;
    pub use super::app_user::Entity as AppUser;
    pub use super::cash_balance::Entity as CashBalance;
    pub use super::currency::Entity as Currency;
    pub use super::deposit::Entity as Deposit;
    pub use super::exchange::Entity as Exchange;
    pub use super::fx_rate::Entity as FxRate;
    pub use super::fund_holding::Entity as FundHolding;
    pub use super::fx_fee::Entity as FxFee;
    pub use super::fx_setting::Entity as FxSetting;
    pub use super::import_batch::Entity as ImportBatch;
    pub use super::instrument::Entity as Instrument;
    pub use super::plan_session::Entity as PlanSession;
    pub use super::plan_share::Entity as PlanShare;
    pub use super::plan_share_session::Entity as PlanShareSession;
    pub use super::price_point::Entity as PricePoint;
    pub use super::txn::Entity as Txn;
    pub use super::upload_audit::Entity as UploadAudit;
    pub use super::user_identity::Entity as UserIdentity;
    pub use super::user_session::Entity as UserSession;
    pub use super::user_setting::Entity as UserSetting;
    pub use super::user_token::Entity as UserToken;
}
