//! The published API description, and the Swagger page that renders it.
//!
//! The description is generated from the handlers themselves - each carries
//! a `#[utoipa::path]` and the router is built from those (see
//! [`super::router`]), so a route cannot exist without being described, nor
//! be described without existing.
//!
//! There is no sign-in, so nothing here is cut down per caller: everyone
//! sees every operation.

use utoipa::openapi::OpenApi as OpenApiDoc;
use utoipa::OpenApi;

use crate::error::ErrorBody;

/// The tags, one per group of endpoints.
pub mod tags {
    pub const PUBLIC: &str = "Public";
    pub const PUBLIC_SHARED: &str = "Shared plans";
    pub const REFERENCE: &str = "Reference data";

    pub const USER_HOLDINGS: &str = "Holdings";
    pub const USER_DASHBOARD: &str = "Net worth";
    pub const USER_RETIREMENT: &str = "Retirement";
    pub const USER_DHAN_YATRA: &str = "CaiYatra";
    pub const USER_PLANNER: &str = "RezekiYojana";
    pub const USER_DEPOSITS: &str = "Deposits";
    pub const USER_CASH: &str = "Cash";
    pub const USER_IMPORTS: &str = "Imports";
    pub const USER_SETTINGS: &str = "Settings";
    pub const USER_DATA: &str = "Delete data";

    pub const REFRESH: &str = "Refresh";
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "CaiRezekiYojana API",
        description = "Everything the CaiRezekiYojana web app does, for any other program to do too.

There is no sign-in: the app is a single-owner planner, so every call acts on the one built-in \
account. Put it behind your own access control (an ingress with authentication, a VPN) if it is \
reachable by anyone you do not trust.

## Errors

Every error is JSON: `{\"error\": <code>, \"message\": <text>}`.",
        license(name = "MIT"),
    ),
    servers((url = "/api/v1")),
    components(schemas(ErrorBody)),
    tags(
        (name = "Public", description = "Health."),
        (name = "Shared plans", description = "What a link to shared RezekiYojana sessions opens, read-only. The link is the credential."),
        (name = "Reference data", description = "Currencies, exchanges and every accepted enum value."),
        (name = "Holdings", description = "Positions, valued in the base currency."),
        (name = "Net worth", description = "Net worth in the base currency, and the refresh of prices and rates."),
        (name = "Retirement", description = "How long the corpus lasts at different withdrawals. Nothing is stored."),
        (name = "CaiYatra", description = "The journey of money: the corpus built up from today to retirement, then drawn down - the simulation table, year by year."),
        (name = "RezekiYojana", description = "The allocation planner: plans that move money out of what is held today, or fresh money, into other categories and funds - worked out against today's values."),
        (name = "Deposits", description = "Fixed and recurring deposits."),
        (name = "Cash", description = "Bank balance snapshots."),
        (name = "Imports", description = "The portfolio CSV upload, its batches and the kept files."),
        (name = "Settings", description = "Preferences: base currency, sessions."),
        (name = "Delete data", description = "Deleting the portfolio in bulk. Cannot be undone."),
        (name = "Refresh", description = "Market-data refreshes."),
    ),
)]
pub struct ApiDoc;

/// Stamp the release on the description: the version in the repo's VERSION
/// file - the app's version and its `cairezeki.<version>` tag - not the crate's.
pub fn stamp_version(openapi: &mut OpenApiDoc) {
    openapi.info.version = super::meta::APP_VERSION.to_string();
}
