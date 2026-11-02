//! Table/column identifiers shared by the migrations.

use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
pub enum AppSetting {
    Table,
    Key,
    Value,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Currency {
    Table,
    Code,
    Name,
    Symbol,
    MinorUnits,
    IsActive,
}

#[derive(DeriveIden)]
pub enum Exchange {
    Table,
    Id,
    Code,
    Name,
    Country,
    CurrencyCode,
    Mic,
    QuoteSuffix,
    Timezone,
}

#[derive(DeriveIden)]
pub enum Account {
    Table,
    Id,
    Name,
    Kind,
    Country,
    CurrencyCode,
    Institution,
    AccountRef,
    OpenedOn,
    IsActive,
    Notes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Instrument {
    Table,
    Id,
    Kind,
    Symbol,
    Name,
    Isin,
    ExchangeId,
    CurrencyCode,
    Country,
    QuoteSymbol,
    AmfiSchemeCode,
    Sector,
    HistoryFrom,
    HoldingsRef,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Txn {
    Table,
    Id,
    AccountId,
    InstrumentId,
    TradeDate,
    Kind,
    Quantity,
    Price,
    Fees,
    Taxes,
    NetAmount,
    CurrencyCode,
    FxRate,
    ExternalId,
    ImportBatchId,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum PricePoint {
    Table,
    Id,
    InstrumentId,
    AsOf,
    Close,
    CurrencyCode,
    Source,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum FxRate {
    Table,
    Id,
    BaseCode,
    QuoteCode,
    Rate,
    AsOf,
    Source,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum FxSetting {
    Table,
    Id,
    BaseCode,
    QuoteCode,
    Provider,
    MarginMode,
    MarginValue,
    ManualRate,
    UseManual,
    Note,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Deposit {
    Table,
    Id,
    AccountId,
    Kind,
    Institution,
    Reference,
    Principal,
    InterestRate,
    Compounding,
    PayoutFrequency,
    Installment,
    StartDate,
    MaturityDate,
    CurrencyCode,
    Country,
    AutoRenew,
    Status,
    MaturityAmount,
    TdsPct,
    Notes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum CashBalance {
    Table,
    Id,
    AccountId,
    AsOf,
    Balance,
    CurrencyCode,
    Source,
    Notes,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum ImportBatch {
    Table,
    Id,
    Kind,
    Filename,
    Status,
    RowCount,
    OkCount,
    ErrorCount,
    SkippedCount,
    ErrorsJson,
    Message,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum FundHolding {
    Table,
    Id,
    InstrumentId,
    AsOf,
    Name,
    Nature,
    InstrumentType,
    Sector,
    WeightPct,
    MarketValue,
    SecurityRef,
    Source,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum FxFee {
    Table,
    Id,
    BaseCode,
    QuoteCode,
    Provider,
    Route,
    Points,
    MidRate,
    FetchedAt,
}

#[derive(DeriveIden)]
pub enum UploadAudit {
    Table,
    Id,
    CreatedAt,
    Action,
    Kind,
    OriginalName,
    StoredAs,
    SizeBytes,
    Sha256,
    Status,
    BatchId,
    RowCount,
    OkCount,
    SkippedCount,
    ErrorCount,
    Message,
}

#[derive(DeriveIden)]
pub enum AppUser {
    Table,
    Id,
    Email,
    DisplayName,
    Role,
    PasswordHash,
    EmailVerified,
    IsEnabled,
    LockedUntil,
    LockReason,
    MustSetPassword,
    AwaitingApproval,
    CreatedAt,
    UpdatedAt,
    LastLoginAt,
}

#[derive(DeriveIden)]
pub enum UserToken {
    Table,
    Id,
    UserId,
    Purpose,
    TokenHash,
    ExpiresAt,
    UsedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum UserIdentity {
    Table,
    Id,
    UserId,
    Provider,
    Subject,
    Email,
    CreatedAt,
}

#[derive(DeriveIden)]
pub enum UserSession {
    Table,
    Id,
    UserId,
    TokenHash,
    CreatedAt,
    ExpiresAt,
    LastSeenAt,
    UserAgent,
}

#[derive(DeriveIden)]
pub enum UserSetting {
    Table,
    Id,
    UserId,
    Key,
    Value,
    UpdatedAt,
}

/// The owner column added to each table holding a user's own records.
#[derive(DeriveIden)]
pub enum Owner {
    UserId,
}

#[derive(DeriveIden)]
pub enum ApiKey {
    Table,
    Id,
    UserId,
    Name,
    Prefix,
    TokenHash,
    CreatedAt,
    ExpiresAt,
    LastUsedAt,
}

#[derive(DeriveIden)]
pub enum PlanSession {
    Table,
    Id,
    UserId,
    Name,
    CreatedAt,
    Plan,
    Page,
    UploadedAt,
    Kind,
}

#[derive(DeriveIden)]
pub enum PlanShare {
    Table,
    Id,
    OwnerId,
    Kind,
    RecipientId,
    Token,
    Label,
    CreatedAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
pub enum PlanShareSession {
    Table,
    Id,
    ShareId,
    SessionId,
}

#[derive(DeriveIden)]
pub enum AiServiceKey {
    Table,
    Id,
    Name,
    Prefix,
    TokenHash,
    CreatedBy,
    CreatedAt,
    RotatedAt,
    LastUsedAt,
}

#[derive(DeriveIden)]
pub enum AiRequest {
    Table,
    Id,
    TokenHash,
    UserId,
    Role,
    Capabilities,
    Feature,
    CreatedAt,
    ExpiresAt,
    RevokedAt,
    PageAction,
}

#[derive(DeriveIden)]
pub enum AiConversation {
    Table,
    Id,
    UserId,
    Title,
    LastFeature,
    Bytes,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum AiMessage {
    Table,
    Id,
    ConversationId,
    Role,
    Content,
    Feature,
    Status,
    Page,
    CreatedAt,
    PageAction,
}

#[derive(DeriveIden)]
pub enum AiProposal {
    Table,
    Id,
    UserId,
    RequestId,
    ConversationId,
    MessageId,
    Kind,
    Title,
    Changes,
    Lines,
    Status,
    Outcome,
    CreatedAt,
    ExpiresAt,
    DecidedAt,
}
