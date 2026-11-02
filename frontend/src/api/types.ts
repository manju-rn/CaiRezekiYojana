/** Mirrors the Rust API types. Kept hand-written and small on purpose. */

export interface FxView {
  base_currency: string
  rate: number
  source: string
  as_of: string | null
  adjusted: boolean
  /** Transfer fee taken off this figure, as a % of it, where the pair charges by amount. */
  fee_pct: number | null
}

export interface Position {
  account_id: number
  account_name: string
  instrument_id: number
  symbol: string
  name: string
  kind: string
  isin: string | null
  exchange_code: string | null
  country: string | null
  /** Funds: the AMFI category heading. Equities: the sector. */
  sector: string | null
  /** Funds only: `Equity`, `Debt`, `Hybrid` or `Other`. */
  fund_class: string | null
  /** Funds only: SEBI sub-category, e.g. `Large Cap`, `Money Market`. */
  fund_category: string | null
  currency: string
  quantity: number
  average_cost: number
  invested: number
  last_price: number | null
  last_price_date: string | null
  price_source: string | null
  market_value: number | null
  unrealised_pnl: number | null
  unrealised_pct: number | null
  realised_pnl: number
  dividends: number
  fees_paid: number
  first_trade: string | null
  xirr: number | null
  fx: FxView
  invested_base: number
  market_value_base: number | null
  unrealised_pnl_base: number | null
  realised_pnl_base: number
  dividends_base: number
  weight_pct: number | null
}

export interface Bucket {
  key: string
  label: string
  value_base: number
  invested_base: number
  weight_pct: number
  fx_rate: number | null
}

/** One unit of a currency in a display currency. */
export interface DisplayRate {
  currency: string
  /** Null when no rate path exists. */
  rate: number | null
  source: string | null
  adjusted: boolean
}

/** The holdings trading in one currency, summed in that currency. */
export interface CurrencyTotals {
  currency: string
  positions: number
  market_value: number
  invested: number
  unrealised_pnl: number
  unrealised_pct: number | null
  realised_pnl: number
  dividends: number
  market_value_base: number
  rates: DisplayRate[]
}

export interface PortfolioSummary {
  base_currency: string
  scope: string
  positions: number
  invested_base: number
  market_value_base: number
  unrealised_pnl_base: number
  unrealised_pct: number | null
  realised_pnl_base: number
  dividends_base: number
  xirr: number | null
  by_currency: Bucket[]
  by_asset_class: Bucket[]
  by_country: Bucket[]
  /** Largest first. */
  by_source_currency: CurrencyTotals[]
  unconverted_currencies: string[]
}

export interface PortfolioView {
  summary: PortfolioSummary
  positions: Position[]
}

/** Keyed by currency code; null where part of the figure has no rate. */
export type CurrencyValues = Record<string, number | null>

export interface Segment {
  key: string
  label: string
  value_base: number
  invested_base: number
  gain_base: number
  gain_pct: number | null
  weight_pct: number
  /** The value in each display currency, each holding converted from its own. */
  values: CurrencyValues
  /** What the row is made of, by segment; absent where not broken down. */
  parts?: SegmentPart[]
}

/** One segment's share of a class, e.g. the global ETFs in equity funds. */
export interface SegmentPart {
  key: string
  label: string
  value_base: number
  /** Of net worth, like the row's own weight. */
  weight_pct: number
  values: CurrencyValues
}

export interface CurrencyExposure {
  currency: string
  value_base: number
  weight_pct: number
  fx_rate: number
  fx_source: string
  fx_as_of: string | null
  fx_adjusted: boolean
}

export interface DepositValuation {
  deposit_id: number
  currency: string
  principal_invested: number
  current_value: number
  accrued_interest: number
  interest_paid_out: number
  maturity_value: number
  interest_at_maturity: number
  /** Tax withheld at source so far, out of the deposit or its payouts. */
  tax_withheld: number
  /** Tax withheld over the whole term. */
  tax_at_maturity: number
  /** What comes back at maturity: `maturity_value` less the tax withheld. */
  maturity_payout: number
  days_elapsed: number
  days_to_maturity: number
  term_days: number
  progress_pct: number
  effective_annual_yield: number
  is_matured: boolean
  maturity_is_bank_quoted: boolean
}

export interface Deposit {
  id: number
  account_id: number
  kind: string
  institution: string
  reference: string | null
  principal: number
  interest_rate: number
  compounding: string
  payout_frequency: string
  installment: number | null
  start_date: string
  maturity_date: string
  currency_code: string
  country: string
  auto_renew: boolean
  status: string
  maturity_amount: number | null
  /** Tax withheld at source, as a percentage - 31.2 for an NRO deposit. */
  tds_pct: number | null
  notes: string | null
  account_name: string
  valuation: DepositValuation
  fx: FxView
  current_value_base: number
  principal_base: number
  maturity_value_base: number
  maturity_payout_base: number
  accrued_interest_base: number
}

export interface MaturingDeposit {
  deposit_id: number
  institution: string
  currency: string
  maturity_date: string
  days_to_maturity: number
  maturity_value: number
  maturity_value_base: number
  auto_renew: boolean
}

export interface DepositSummary {
  base_currency: string
  count: number
  active_count: number
  principal_base: number
  current_value_base: number
  accrued_interest_base: number
  maturity_value_base: number
  /** What comes back at maturity, after tax withheld. */
  maturity_payout_base: number
  tax_withheld_base: number
  weighted_average_rate: number
  /** The currencies `current_values` carries, in display order. */
  display_currencies: string[]
  /** Current value in each display currency, each deposit converted from its own. */
  current_values: CurrencyValues
  /** Current value per bank, largest first. */
  by_institution: BankSplit[]
  by_currency: {
    currency: string
    country: string
    count: number
    principal: number
    current_value: number
    fx_rate: number
    current_value_base: number
  }[]
  maturing_soon: MaturingDeposit[]
}

export interface CashAccount {
  account_id: number
  name: string
  institution: string | null
  country: string
  currency: string
  balance: number
  as_of: string | null
  is_stale: boolean
  fx: FxView
  balance_base: number
}

/** One bank's share of a page's total, in the base and every display currency. */
export interface BankSplit {
  institution: string
  /** Accounts on the cash page, deposits on the deposits page. */
  count: number
  value_base: number
  weight_pct: number
  values: CurrencyValues
}

export interface CashSummary {
  base_currency: string
  total_base: number
  /** The currencies `total_values` carries, in display order. */
  display_currencies: string[]
  /** The total in each display currency, each balance converted from its own. */
  total_values: CurrencyValues
  accounts: number
  stale_accounts: number
  /** Balances per bank, largest first. */
  by_institution: BankSplit[]
  by_currency: {
    currency: string
    country: string
    balance: number
    fx_rate: number
    balance_base: number
    accounts: number
  }[]
}

export interface NetWorth {
  base_currency: string
  as_of: string
  total_base: number
  invested_base: number
  gain_base: number
  gain_pct: number | null
  /** The currencies `values` carry, in display order. */
  display_currencies: string[]
  /** Net worth in each display currency. */
  total_values: CurrencyValues
  /** What went in, in each display currency, each holding converted from
   *  its own currency. */
  invested_values: CurrencyValues
  /** Funds, ETFs, stocks, deposits, cash. */
  segments: Segment[]
  /** The same total on the equity-debt line, deposits and cash included. */
  by_class: Segment[]
  currency_exposure: CurrencyExposure[]
  india_equity: PortfolioSummary
  india_mf: PortfolioSummary
  global: PortfolioSummary
  deposits: DepositSummary
  cash: CashSummary
  warnings: string[]
  /** Pages left out of every figure above, by key (`global`, `cash`, ...). */
  excluded: string[]
}

export interface FxRow {
  base_code: string
  quote_code: string
  raw_rate: number | null
  effective_rate: number | null
  margin_mode: string
  margin_value: number
  manual_rate: number | null
  use_manual: boolean
  provider: string
  source: string | null
  as_of: string | null
  margin_effect: number | null
  setting_id: number | null
  note: string | null
  /** What Wise charges on the pair, once fetched. */
  fee: FxFee | null
}

export interface FxFee {
  provider: string
  route: string
  fetched_at: string
  mid_rate: number | null
  /** The pair is set to Wise fees and no manual rate overrides it. */
  applied: boolean
  /** Sampled amounts, in the pair's base currency. */
  examples: { amount: number; fee: number; fee_pct: number; rate: number | null }[]
}

export interface FxProviderInfo {
  id: string
  label: string
  configured: boolean
  notes: string
}

export interface Account {
  id: number
  name: string
  kind: string
  country: string
  currency_code: string
  institution: string | null
  account_ref: string | null
  opened_on: string | null
  is_active: boolean
  notes: string | null
}

export interface ColumnSpec {
  name: string
  heading: string
  aliases: string[]
  required: boolean
  example: string
  description: string
}

export interface ImportSpec {
  kind: string
  title: string
  description: string
  columns: ColumnSpec[]
  sample_rows: string[][]
  notes: string[]
}

export interface ImportOutcome {
  batch_id: number | null
  kind: string
  filename: string
  status: string
  row_count: number
  ok_count: number
  skipped_count: number
  error_count: number
  errors: { row: number; message: string }[]
  created_accounts: string[]
  created_instruments: string[]
  unrecognised_columns: string[]
  dry_run: boolean
  /** The file's name in the uploads folder, when one is configured. */
  stored_as: string | null
  /** This upload's number in the upload audit. */
  audit_seq: number | null
}

/** One step in the upload audit. Only ever added to; `id` is the sequence. */
export interface UploadAudit {
  id: number
  created_at: string
  action: 'VALIDATE' | 'IMPORT' | 'UNDO'
  kind: string
  original_name: string
  stored_as: string | null
  size_bytes: number
  sha256: string | null
  status: string
  batch_id: number | null
  row_count: number
  ok_count: number
  skipped_count: number
  error_count: number
  message: string | null
}

export interface ImportBatch {
  id: number
  kind: string
  filename: string
  status: string
  row_count: number
  ok_count: number
  error_count: number
  skipped_count: number
  errors_json: string | null
  message: string | null
  created_at: string
}

/** Mirrors `instrument::Model`; sent back whole on update. */
export interface Instrument {
  id: number
  kind: string
  symbol: string
  name: string
  isin: string | null
  exchange_id: number | null
  currency_code: string
  country: string | null
  quote_symbol: string | null
  amfi_scheme_code: string | null
  sector: string | null
  /** `YYYY-MM-DD`: how far back the NAV backfill has filled history. */
  history_from: string | null
  /** Funds: the portfolio feed's reference for the scheme (a Groww slug),
   *  learned on a confirmed match. Edit to override; blank to search again. */
  holdings_ref: string | null
  is_active: boolean
  created_at: string
  updated_at: string
}

export interface PricePoint {
  id: number
  instrument_id: number
  /** `YYYY-MM-DD`. */
  as_of: string
  close: number
  currency_code: string
  source: string
  created_at: string
}

export interface RefreshReport {
  source: string
  requested: number
  updated: number
  missing: number
  warnings: string[]
  as_of: string | null
  /** FX pairs whose Wise fees were re-sampled. */
  fees_updated?: number
}

/** A page's visibility for the signed-in user, and whether an administrator pinned it. */
export interface PageVisibility {
  page: string
  mode: string
  own: string
  overridden: string | null
}

/** The signed-in user's own preferences, plus app-wide choices shown read-only. */
export interface SettingsResponse {
  settings: Record<string, string>
  visibility: PageVisibility[]
  app: Record<string, string>
  secrets_set: string[]
  /** The text-size pills' steps (their own, when set and within the limit) and the slider's limits. */
  text_size: TextSize
}

/** In per cent: each click's step up from 100%, and the slider's limits. */
export interface TextSize {
  steps: number[]
  min: number
  max: number
}

/** App-wide settings, for administrators. */
export interface AdminSettingsResponse {
  settings: Record<string, string>
  secrets_set: string[]
  oauth_redirect_uris: Record<string, string>
}

export type Role = 'ADMIN' | 'USER' | 'EXPORTER'

export interface CurrentUser {
  id: number
  email: string
  display_name: string
  role: Role
  email_verified: boolean
  has_password: boolean
  base_currency: string | null
  identities: string[]
  created_at: string
  last_login_at: string | null
}

/** A personal API key, as its owner sees it after creation. */
export interface ApiKey {
  id: number
  name: string
  prefix: string
  created_at: string
  expires_at: string | null
  last_used_at: string | null
}

/** A new key: the only time `key` is ever shown. */
export interface CreatedApiKey extends ApiKey {
  key: string
}

export interface AuthConfig {
  setup_required: boolean
  /** The administrator has turned the sign-up page on. */
  signup_enabled: boolean
  email_verification_required: boolean
  mail_configured: boolean
  providers: { id: string; name: string }[]
  default_base_currency: string
  /** The app-wide text-size steps and limits, for everyone. */
  text_size: TextSize
}

export interface AdminUser {
  id: number
  email: string
  display_name: string
  role: Role
  email_verified: boolean
  is_enabled: boolean
  locked_until: string | null
  lock_reason: string | null
  pending_activation: boolean
  /** Signed up by themselves, and waiting to be enabled. */
  awaiting_approval: boolean
  has_password: boolean
  activation_expires_at: string | null
  identities: string[]
  active_sessions: number
  page_overrides: Record<string, string>
  /** What the administrator has switched off of AI mode for this account. */
  ai: AiOverrides
  created_at: string
  last_login_at: string | null
}

export interface IssuedToken {
  user: AdminUser
  token: string
  expires_at: string
  activation_url: string
}

export interface ExportableUser {
  id: number
  email: string
  display_name: string
  is_enabled: boolean
  created_at: string
  accounts: number
}

export interface TransactionRow {
  id: number
  account_id: number
  instrument_id: number
  trade_date: string
  kind: string
  quantity: number
  price: number
  fees: number
  taxes: number
  net_amount: number
  currency_code: string
  fx_rate: number | null
  external_id: string | null
  notes: string | null
  symbol: string
  instrument_name: string
  instrument_kind: string
}

/* ------------------------------------------------------------------ *
 * Look-through: the securities inside the mutual funds
 * ------------------------------------------------------------------ */

/** One fund's contribution to a security. */
export interface FundSlice {
  instrument_id: number
  fund_name: string
  /** What your units in the fund are worth, in base currency. */
  fund_value_base: number
  /** The security's share of the fund's assets, 0-100. */
  weight_pct: number
  /** Your slice: fund value times weight. */
  value_base: number
  /** `YYYY-MM-DD`: the disclosure the weight came from. */
  as_of: string
  /** Not a fund: the security itself, held outright. */
  direct: boolean
}

/** One security, summed across every fund that holds it. */
export interface Exposure {
  key: string
  name: string
  /** `EQUITY`, `DEBT`, `CASH`, `MF` or `OTHER`. */
  nature: string
  instrument_type: string | null
  sector: string | null
  value_base: number
  /** Share of the covered fund value, 0-100. */
  weight_pct: number
  fund_count: number
  /** Largest contribution first. */
  funds: FundSlice[]
  /** The part of a portfolio the feed does not itemise, not one security. */
  remainder: boolean
}

export interface FundCoverage {
  instrument_id: number
  name: string
  market_value_base: number
  /** Rows stored; zero means the fund's portfolio is not known yet. */
  holdings: number
  as_of: string | null
  source: string | null
  fetched_at: string | null
}

export interface LookThroughSlice {
  key: string
  label: string
  value_base: number
  weight_pct: number
}

export type LookThroughScope = 'india_mf' | 'global'

export interface LookThrough {
  scope: LookThroughScope
  /** What every figure is in: the base, or the currency asked for. */
  base_currency: string
  /** Valuation date: today's fund values, not the disclosure date. */
  as_of: string
  /** Every priced fund position, covered or not. */
  fund_value_base: number
  /** The part with a disclosed portfolio. */
  covered_value_base: number
  /** Securities held outright beside the funds. Exposures sum to this
   *  plus the covered value. */
  direct_value_base: number
  funds: FundCoverage[]
  exposures: Exposure[]
  by_nature: LookThroughSlice[]
  by_sector: LookThroughSlice[]
  warnings: string[]
}

/* ------------------------------------------------------------------ *
 * Portfolio history: what the invested portfolio has been worth
 * ------------------------------------------------------------------ */

export interface HistoryPoint {
  /** `YYYY-MM-DD`. */
  date: string
  value: number
}

/** One day's dealing in one direction, summed across every holding traded. */
export interface PortfolioTrade {
  date: string
  /** `BUY` or `SELL`. */
  kind: string
  amount_base: number
  holdings: number
}

export interface PortfolioHistory {
  /** The currency every figure is in: the one asked for, else the base. */
  base_currency: string
  /** First transaction date; null when nothing has been bought yet. */
  from: string | null
  to: string
  points: HistoryPoint[]
  trades: PortfolioTrade[]
  warnings: string[]
}

/** One page's records that could be deleted, and the institutions they are at. */
export interface DeletableCategory {
  key: 'india_equity' | 'india_mf' | 'global' | 'deposits' | 'cash'
  label: string
  /** What `count` counts: `transactions`, `deposits` or `balances`. */
  unit: string
  count: number
  institutions: { name: string; count: number }[]
}

/** What a bulk delete removed. */
export interface Purged {
  transactions: number
  deposits: number
  cash_balances: number
  accounts: number
  import_batches: number
}

// ---------------------------------------------------------------------------
// AI mode
// ---------------------------------------------------------------------------

/** Who switched AI mode, or a capability, off. */
export type AiOffBy = 'app' | 'admin' | 'owner'

/** Where the chat opens: a window over the page, or the `/ai` page. */
export type AiLayout = 'floating' | 'page'

/** One conversation, gone on with until cleared, or several to pick from. */
export type AiConversations = 'single' | 'multiple'

export interface AiCapability {
  id: 'portfolio_qa' | 'planner' | 'unrestrictive' | 'admin'
  title: string
  description: string
  enabled: boolean
  off_by: AiOffBy | null
}

/** `GET /ai/features`: what AI mode is for the caller. */
export interface AiFeatures {
  enabled: boolean
  off_by: AiOffBy | null
  /** The caller's own switch, in their profile. */
  own_enabled: boolean
  capabilities: AiCapability[]
  layout: AiLayout
  layout_pinned: boolean
  conversations: AiConversations
  /** Who decided it, when the account cannot: the administrator for everyone (`app`) or for this account (`admin`). */
  conversations_set_by: Exclude<AiOffBy, 'owner'> | null
}

export interface AiOverrides {
  disabled: boolean
  capabilities_off: string[]
  /** Granted to this account, of those off until an administrator grants them (`unrestrictive`). */
  capabilities_granted: string[]
  /** Set for this account; `null` while the app-wide setting and their own decide. */
  conversations: AiConversations | null
}

export interface AiConversation {
  id: number
  title: string
  last_feature: string | null
  bytes: number
  created_at: string
  updated_at: string
}

export interface AiConversationList {
  conversations: AiConversation[]
  used_bytes: number
  cap_bytes: number
}

export interface AiMessage {
  id: number
  role: 'user' | 'assistant'
  content: string
  feature: string | null
  /** `error`: the answer failed, and its content says why. */
  status: 'done' | 'error'
  page: string | null
  created_at: string
  /** The changes this answer proposed, for the user to approve or cancel. */
  proposal: AiProposal | null
  page_action: AiPageAction | null
}

/**
 * What an answer did to one of the user's pages, as the API takes it: a whole
 * journey (`inputs`), and rope changes made after it - or, without `inputs`,
 * to the page as it stood.
 */
export interface AiPageAction {
  page: 'dhan-yatra'
  inputs?: Record<string, unknown>
  rope?: AiRopeChange[]
}

/** A change to the CaiYatra page's rope, as `set_dhan_yatra_rope` takes it. */
export interface AiRopeChange {
  straighten?: ('return' | 'inflation' | 'withdrawal')[]
  mode?: 'set' | 'add'
  adjustments?: { assumption: string; from_year: number; to_year?: number; points: number }[]
}

/** One change of a proposal, in words. */
export interface AiChangeLine {
  op: string
  text: string
  /** In the base currency. */
  amount: number | null
}

export type AiProposalStatus = 'pending' | 'approved' | 'cancelled' | 'expired' | 'failed'

/** Changes an assistant proposed: all approved, or all cancelled, together. */
export interface AiProposal {
  id: number
  kind: 'plan'
  title: string
  lines: AiChangeLine[]
  status: AiProposalStatus
  /** Why approving failed. */
  outcome: string | null
  conversation_id: number | null
  message_id: number | null
  created_at: string
  expires_at: string
  decided_at: string | null
}

export interface AiConversationDetail extends AiConversation {
  messages: AiMessage[]
}

/** An AI service key as listed: never the key itself. */
export interface AiServiceKey {
  id: number
  name: string
  prefix: string
  created_by: number | null
  created_at: string
  rotated_at: string | null
  last_used_at: string | null
}

/** A key just minted or rotated: shown this once. */
export interface CreatedAiServiceKey extends AiServiceKey {
  key: string
}

/** Whether the open CaiYatra page offers the assistant. */
export interface GuestAiStatus {
  enabled: boolean
}

/** The assistant's answer on the open CaiYatra page. */
export interface GuestAiAnswer {
  /** Sent back with the next question. */
  session: string
  answer: string
  /** The page's inputs as the answer leaves them, when it changed them. */
  page: Record<string, unknown> | null
}
