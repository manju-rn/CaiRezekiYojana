//! RezekiYojana: each user's allocation plan.
//!
//! A plan is a list of moves out of what is held today - a category, or one
//! fund in it, or fresh money not yet invested - into a category held today
//! or one the plan makes, with the categories and funds the plan makes, the
//! categories it empties and the classifications it chooses. It is always
//! worked out against today's values: nothing in it is a transaction.
//!
//! There is one plan, made by segment. The equity-debt view (`classes`) is
//! that plan regrouped by class, which the page works out; here it reads the
//! segment plan and cannot change it.
//!
//! The plan is kept as JSON in `user_setting` under `plan_segments`, with a
//! revision that goes up by one with every change. (`plan_classes`, from
//! when each view had a plan of its own, is no longer read.)
//! The web app, the API and the MCP tools all edit the same plan: the page
//! polls for a newer revision and shows it, and a whole-plan write made
//! against an older revision is refused with `409`, so no one silently
//! overwrites a change made elsewhere.

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use rand::RngCore;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::entities::{account, instrument};
use crate::error::{AppError, AppResult};
use crate::services::{networth, prefs};
use crate::state::AppState;

/// The source key for money not yet invested.
pub const FRESH: &str = "__fresh__";

/// Classifications a category or fund in a plan may take.
pub const CLASSES: &[&str] =
    &["equity_fund", "equity_stock", "debt_high", "debt_low", "gold", "real_estate", "other"];

/// Below this, an amount is rounding noise - as on the page.
const EPS: f64 = 0.5;

const MAX_MOVES: usize = 2000;
const MAX_ITEMS: usize = 500;
const MAX_TEXT: usize = 500;

/// Which split of the portfolio a plan works on: the dashboard's segments
/// (funds, ETFs, stocks, deposits, cash) or its equity-debt classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Segments,
    Classes,
}

impl View {
    fn setting_key(self) -> &'static str {
        match self {
            View::Segments => "plan_segments",
            View::Classes => "plan_classes",
        }
    }
}

/// A category the plan makes, not held today.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Created {
    /// `custom-...`; the `to` of a move into it.
    pub key: String,
    pub label: String,
    /// One of `equity_fund`, `equity_stock`, `debt_high`, `debt_low`, `gold`, `real_estate`, `other`.
    pub cls: String,
}

/// A fund or holding the plan makes inside a category.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewSub {
    /// `sub-...`; the `to_sub` of a move into it.
    pub key: String,
    /// The category it is in.
    pub bucket: String,
    pub label: String,
    pub cls: String,
}

/// Money planned out of one place and into another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Move {
    pub id: String,
    /// A category held today, or `__fresh__` for fresh money.
    pub from: String,
    /// The fund or account within `from` it is taken out of (`i:<instrument id>`,
    /// `b:<institution>` for a bank's deposits, `c:<cash account id>`); absent: the category as a whole.
    #[serde(default, alias = "fromSub", skip_serializing_if = "Option::is_none")]
    pub from_sub: Option<String>,
    /// A category held today, or one the plan made.
    pub to: String,
    /// The fund within `to` it goes into - one held today (`i:...`) or one the
    /// plan made (`sub-...`); absent: the category as a whole.
    #[serde(default, alias = "toSub", skip_serializing_if = "Option::is_none")]
    pub to_sub: Option<String>,
    /// In the base currency.
    pub amount: f64,
    /// Why: shown in the decision list and the CSV.
    #[serde(default)]
    pub note: String,
    /// When it was planned, RFC 3339.
    #[serde(default)]
    pub at: String,
}

/// A whole plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Plan {
    /// Categories made in the plan that do not exist today.
    pub created: Vec<Created>,
    /// Funds made in the plan, in any category.
    pub subs: Vec<NewSub>,
    pub moves: Vec<Move>,
    /// Money to be invested on top of today's portfolio, in the base currency.
    pub fresh: f64,
    /// Today's categories deleted from the plan once emptied.
    pub removed: Vec<String>,
    /// A classification chosen for one of today's categories, over the one it has.
    pub cls: BTreeMap<String, String>,
}

/// What is kept in `user_setting`.
#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    revision: i64,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    plan: Plan,
}

/// A plan, and the revision it is at.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct PlanState {
    pub view: View,
    /// Goes up by one with every change, whoever makes it. 0: never saved.
    pub revision: i64,
    pub updated_at: Option<DateTime<Utc>>,
    pub plan: Plan,
    /// The key of the category or fund just made, or the id of the move just added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub made: Option<String>,
    /// The plan worked out against today's values; with `summary=true`, and
    /// after every change but a whole-plan write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
}

/// Today against planned, category by category.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Summary {
    pub base_currency: String,
    pub today_total: f64,
    /// Today's total and the fresh money moved in.
    pub planned_total: f64,
    /// Fresh money not yet moved anywhere.
    pub fresh_left: f64,
    pub categories: Vec<CategoryLine>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct CategoryLine {
    pub key: String,
    pub label: String,
    /// Made in the plan, not held today.
    pub is_new: bool,
    /// Deleted from the plan once emptied.
    pub removed: bool,
    /// The classification chosen in the plan, if any.
    pub cls: Option<String>,
    pub today: f64,
    pub moved_in: f64,
    pub moved_out: f64,
    pub planned: f64,
    /// What can still be moved out of it: today's value less what is already
    /// planned out. Money moved in stays where it was put.
    pub available: f64,
    /// `new`, `added`, `reduced`, `exited` or `unchanged`.
    pub status: String,
}

/// Plans are read, changed and written back under one lock, so two changes
/// at once cannot lose either.
fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn load(state: &AppState, owner: i32, view: View) -> Stored {
    match prefs::get_opt(&state.db, owner, view.setting_key()).await {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            tracing::warn!(owner, view = view.setting_key(), error = %e, "stored plan does not parse; starting afresh");
            Stored::default()
        }),
        None => Stored::default(),
    }
}

async fn store(state: &AppState, owner: i32, view: View, revision: i64, plan: Plan) -> AppResult<Stored> {
    let stored = Stored { revision: revision + 1, updated_at: Some(Utc::now()), plan };
    let raw = serde_json::to_string(&stored).map_err(|e| AppError::Other(e.into()))?;
    prefs::set(&state.db, owner, view.setting_key(), &raw).await?;
    Ok(stored)
}

fn answer(view: View, s: Stored, made: Option<String>, summary: Option<Summary>) -> PlanState {
    PlanState { view, revision: s.revision, updated_at: s.updated_at, plan: s.plan, made, summary }
}

/// Today's categories for a view: key, label and value in the base currency.
struct Today {
    base_currency: String,
    rows: Vec<(String, String, f64)>,
}

impl Today {
    fn has(&self, key: &str) -> bool {
        self.rows.iter().any(|(k, _, _)| k == key)
    }
    fn names(&self) -> String {
        self.rows.iter().map(|(k, _, _)| k.as_str()).collect::<Vec<_>>().join(", ")
    }
}

async fn today(state: &AppState, owner: i32, view: View) -> AppResult<Today> {
    let nw = networth::build(state, owner).await?;
    let rows = match view {
        View::Segments => nw.segments,
        View::Classes => nw.by_class,
    };
    Ok(Today {
        base_currency: nw.base_currency,
        rows: rows
            .into_iter()
            .filter(|s| s.value_base.abs() > EPS)
            .map(|s| (s.key, s.label, s.value_base))
            .collect(),
    })
}

/// Today's values with the moves applied - the page's own arithmetic. A move
/// out of or into a category not in this view is skipped, not guessed at.
struct Model {
    current: BTreeMap<String, f64>,
    planned: BTreeMap<String, f64>,
    inflow: BTreeMap<String, f64>,
    outflow: BTreeMap<String, f64>,
    fresh_used: f64,
}

impl Model {
    fn of(today: &Today, plan: &Plan) -> Model {
        let current: BTreeMap<String, f64> = today.rows.iter().map(|(k, _, v)| (k.clone(), *v)).collect();
        let mut planned = current.clone();
        for c in &plan.created {
            planned.insert(c.key.clone(), 0.0);
        }
        let mut inflow = BTreeMap::new();
        let mut outflow = BTreeMap::new();
        let mut fresh_used = 0.0;
        for m in &plan.moves {
            let known = |k: &str| k == FRESH || planned.contains_key(k);
            if !known(&m.from) || !known(&m.to) || m.to == FRESH {
                continue;
            }
            if m.from == FRESH {
                fresh_used += m.amount;
            } else {
                *planned.get_mut(&m.from).unwrap() -= m.amount;
                *outflow.entry(m.from.clone()).or_insert(0.0) += m.amount;
            }
            *planned.get_mut(&m.to).unwrap() += m.amount;
            *inflow.entry(m.to.clone()).or_insert(0.0) += m.amount;
        }
        Model { current, planned, inflow, outflow, fresh_used }
    }

    /// What can still leave a category, or fresh money.
    fn available(&self, plan: &Plan, key: &str) -> f64 {
        if key == FRESH {
            return (plan.fresh - self.fresh_used).max(0.0);
        }
        let held = self.current.get(key).copied().unwrap_or(0.0);
        (held - self.outflow.get(key).copied().unwrap_or(0.0)).max(0.0)
    }
}

fn summarise(today: &Today, plan: &Plan) -> Summary {
    let model = Model::of(today, plan);
    let today_total: f64 = today.rows.iter().map(|(_, _, v)| v).sum();
    let removed: HashSet<&str> = plan.removed.iter().map(String::as_str).collect();
    let line = |key: &str, label: &str, is_new: bool, cls: Option<String>| {
        let cur = model.current.get(key).copied().unwrap_or(0.0);
        let planned = model.planned.get(key).copied().unwrap_or(0.0);
        let change = planned - cur;
        let status = if is_new {
            "new"
        } else if change.abs() <= EPS {
            "unchanged"
        } else if planned <= EPS {
            "exited"
        } else if change > 0.0 {
            "added"
        } else {
            "reduced"
        };
        CategoryLine {
            key: key.to_string(),
            label: label.to_string(),
            is_new,
            removed: removed.contains(key),
            cls,
            today: cur,
            moved_in: model.inflow.get(key).copied().unwrap_or(0.0),
            moved_out: model.outflow.get(key).copied().unwrap_or(0.0),
            planned,
            available: if is_new { 0.0 } else { model.available(plan, key) },
            status: status.to_string(),
        }
    };
    let mut categories: Vec<CategoryLine> =
        today.rows.iter().map(|(k, l, _)| line(k, l, false, plan.cls.get(k).cloned())).collect();
    categories.extend(plan.created.iter().map(|c| line(&c.key, &c.label, true, Some(c.cls.clone()))));
    Summary {
        base_currency: today.base_currency.clone(),
        today_total,
        planned_total: today_total + model.fresh_used,
        fresh_left: model.available(plan, FRESH),
        categories,
    }
}

fn new_key(prefix: &str) -> String {
    let mut buf = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    format!("{prefix}-{}", buf.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn check_cls(cls: &str) -> AppResult<()> {
    if CLASSES.contains(&cls) {
        Ok(())
    } else {
        Err(AppError::bad(format!("cls '{cls}' must be one of: {}", CLASSES.join(", "))))
    }
}

fn check_text(what: &str, s: &str, required: bool) -> AppResult<()> {
    if required && s.trim().is_empty() {
        return Err(AppError::bad(format!("{what} must not be empty")));
    }
    if s.chars().count() > MAX_TEXT {
        return Err(AppError::bad(format!("{what} is longer than {MAX_TEXT} characters")));
    }
    Ok(())
}

/// A plan's own consistency: valid classifications, unique keys and ids,
/// amounts that are amounts. Whether a category exists today is not checked
/// here - a plan outlives the holdings it was made against.
pub fn validate(plan: &Plan) -> AppResult<()> {
    if plan.moves.len() > MAX_MOVES {
        return Err(AppError::bad(format!("a plan holds at most {MAX_MOVES} moves")));
    }
    if plan.created.len() > MAX_ITEMS || plan.subs.len() > MAX_ITEMS || plan.removed.len() > MAX_ITEMS || plan.cls.len() > MAX_ITEMS {
        return Err(AppError::bad(format!("a plan holds at most {MAX_ITEMS} categories and funds of each kind")));
    }
    if !plan.fresh.is_finite() || plan.fresh < 0.0 {
        return Err(AppError::bad("fresh must be zero or more"));
    }
    let mut keys = HashSet::new();
    for c in &plan.created {
        check_text("a category's key", &c.key, true)?;
        check_text("a category's label", &c.label, true)?;
        check_cls(&c.cls)?;
        if c.key == FRESH || !keys.insert(c.key.as_str()) {
            return Err(AppError::bad(format!("category key '{}' is used twice", c.key)));
        }
    }
    let mut sub_keys = HashSet::new();
    for s in &plan.subs {
        check_text("a fund's key", &s.key, true)?;
        check_text("a fund's label", &s.label, true)?;
        check_text("a fund's category", &s.bucket, true)?;
        check_cls(&s.cls)?;
        if !sub_keys.insert(s.key.as_str()) {
            return Err(AppError::bad(format!("fund key '{}' is used twice", s.key)));
        }
    }
    for (key, cls) in &plan.cls {
        check_text("a classified category's key", key, true)?;
        check_cls(cls)?;
    }
    for key in &plan.removed {
        check_text("a removed category's key", key, true)?;
    }
    let mut ids = HashSet::new();
    for m in &plan.moves {
        check_text("a move's id", &m.id, true)?;
        if !ids.insert(m.id.as_str()) {
            return Err(AppError::bad(format!("move id '{}' is used twice", m.id)));
        }
        check_text("a move's from", &m.from, true)?;
        check_text("a move's to", &m.to, true)?;
        check_text("a move's note", &m.note, false)?;
        check_text("a move's at", &m.at, false)?;
        for sub in [&m.from_sub, &m.to_sub].into_iter().flatten() {
            check_text("a move's fund", sub, true)?;
        }
        if m.to == FRESH {
            return Err(AppError::bad("a move cannot go into fresh money"));
        }
        if !m.amount.is_finite() || m.amount <= 0.0 {
            return Err(AppError::bad(format!("move {}: amount must be more than zero", m.id)));
        }
    }
    Ok(())
}

/// The plan, with or without today's values worked into it. Either view
/// reads the one plan, made by segment.
pub async fn get(state: &AppState, owner: i32, _view: View, summary: bool) -> AppResult<PlanState> {
    let view = View::Segments;
    let stored = load(state, owner, view).await;
    let summary = if summary { Some(summarise(&today(state, owner, view).await?, &stored.plan)) } else { None };
    Ok(answer(view, stored, None, summary))
}

/// Replace a whole plan. With `revision`, only if the plan is still at it:
/// a write made against an older one is refused, so a change made elsewhere
/// in the meantime is not overwritten.
pub async fn replace(state: &AppState, owner: i32, view: View, plan: Plan, revision: Option<i64>) -> AppResult<PlanState> {
    writable(view)?;
    validate(&plan)?;
    let _guard = lock().lock().await;
    let stored = load(state, owner, view).await;
    if let Some(expected) = revision {
        if expected != stored.revision {
            return Err(AppError::Conflict(format!(
                "the plan has changed since revision {expected} (it is at {}); read it again and reapply the change",
                stored.revision
            )));
        }
    }
    Ok(answer(view, store(state, owner, view, stored.revision, plan).await?, None, None))
}

/// The plan is changed by segment only: the equity-debt view is the same plan
/// regrouped by class, to look at.
fn writable(view: View) -> AppResult<()> {
    match view {
        View::Segments => Ok(()),
        View::Classes => Err(AppError::BadRequest(
            "the equity-debt view (`classes`) is the segment plan regrouped by class and cannot be changed; \
             make changes with view `segments`"
                .into(),
        )),
    }
}

/// Change a plan in place, against today's values, under the lock. `change`
/// answers what it made, if anything.
async fn change(
    state: &AppState,
    owner: i32,
    view: View,
    change: impl FnOnce(&mut Plan, &Today) -> AppResult<Option<String>>,
) -> AppResult<PlanState> {
    writable(view)?;
    let today = today(state, owner, view).await?;
    let _guard = lock().lock().await;
    let stored = load(state, owner, view).await;
    let mut plan = stored.plan.clone();
    let made = change(&mut plan, &today)?;
    validate(&plan)?;
    let summary = summarise(&today, &plan);
    Ok(answer(view, store(state, owner, view, stored.revision, plan).await?, made, Some(summary)))
}

pub async fn reset(state: &AppState, owner: i32, view: View) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| reset_in(plan, today)).await
}

fn reset_in(plan: &mut Plan, _today: &Today) -> AppResult<Option<String>> {
    *plan = Plan::default();
    Ok(None)
}

/// A move to add. Exactly one of `amount` and `pct`.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct NewMove {
    /// A category held today (a key from `summary.categories` with `is_new` false), or
    /// `__fresh__` for fresh money (set it first with `fresh`). Every move starts from
    /// what is held today: money moved into a category stays there.
    pub from: String,
    /// One fund or account within `from` to take it out of: `i:<instrument id>`,
    /// `b:<institution>` (a bank's deposits) or `c:<cash account id>`. Leave out for the category as a whole.
    #[serde(default)]
    pub from_sub: Option<String>,
    /// The category it goes into: one held today, or one the plan made (`custom-...`).
    pub to: String,
    /// The fund within `to` it goes into: one held today (`i:<instrument id>`, ...) or one the
    /// plan made (`sub-...`). Leave out for the category as a whole.
    #[serde(default)]
    pub to_sub: Option<String>,
    /// How much, in the base currency.
    #[serde(default)]
    pub amount: Option<f64>,
    /// How much, as a percentage (0-100] of what `from` holds today - two 25% moves take half.
    /// Only for a whole category or fresh money; for one fund give `amount`.
    #[serde(default)]
    pub pct: Option<f64>,
    /// Why, shown in the decisions list.
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn add_move(state: &AppState, owner: i32, view: View, input: NewMove) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| add_move_in(plan, today, input)).await
}

fn add_move_in(plan: &mut Plan, today: &Today, input: NewMove) -> AppResult<Option<String>> {
    let model = Model::of(today, plan);
    let created = |k: &str| plan.created.iter().any(|c| c.key == k);
    if input.from != FRESH && !today.has(&input.from) {
        return Err(AppError::bad(format!(
            "from '{}' must be {FRESH} or one of today's categories: {}",
            input.from,
            today.names()
        )));
    }
    if input.from == FRESH && input.from_sub.is_some() {
        return Err(AppError::bad("fresh money has no funds; leave out from_sub"));
    }
    if !today.has(&input.to) && !created(&input.to) {
        return Err(AppError::bad(format!(
            "to '{}' must be one of today's categories ({}) or one the plan made",
            input.to,
            today.names()
        )));
    }
    if plan.removed.contains(&input.to) {
        return Err(AppError::bad(format!("'{}' is deleted from the plan; restore it first", input.to)));
    }
    if input.from == input.to && (input.from_sub.is_none() || input.from_sub == input.to_sub) {
        return Err(AppError::bad("a move must go somewhere else: another category, or another fund in this one"));
    }
    if let Some(sub) = input.to_sub.as_deref().filter(|s| s.starts_with("sub-")) {
        if !plan.subs.iter().any(|s| s.key == sub && s.bucket == input.to) {
            return Err(AppError::bad(format!("'{sub}' is not a fund the plan made in '{}'", input.to)));
        }
    }
    let held = if input.from == FRESH { plan.fresh } else { model.current.get(&input.from).copied().unwrap_or(0.0) };
    let amount = match (input.amount, input.pct) {
        (Some(a), None) => a,
        (None, Some(p)) => {
            if input.from_sub.is_some() {
                return Err(AppError::bad("give amount, not pct, to move out of one fund"));
            }
            if !(p > 0.0 && p <= 100.0) {
                return Err(AppError::bad("pct must be more than 0 and at most 100"));
            }
            held * p / 100.0
        }
        _ => return Err(AppError::bad("give exactly one of amount and pct")),
    };
    if !amount.is_finite() || amount <= 0.0 {
        return Err(AppError::bad("amount must be more than zero"));
    }
    let available = model.available(plan, &input.from);
    if amount > available + EPS {
        let what = if input.from == FRESH { "fresh money".to_string() } else { format!("'{}'", input.from) };
        return Err(AppError::bad(format!(
            "{what} has {available:.2} {} left to move, less than {amount:.2}",
            today.base_currency
        )));
    }
    let note = input.note.unwrap_or_default();
    let id = new_key("move");
    plan.moves.push(Move {
        id: id.clone(),
        from: input.from,
        from_sub: input.from_sub.filter(|s| !s.is_empty()),
        to: input.to,
        to_sub: input.to_sub.filter(|s| !s.is_empty()),
        // Within rounding of all that is left: all that is left.
        amount: amount.min(available),
        note: note.trim().to_string(),
        at: Utc::now().to_rfc3339(),
    });
    Ok(Some(id))
}

pub async fn remove_move(state: &AppState, owner: i32, view: View, id: &str) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| remove_move_in(plan, today, id)).await
}

fn remove_move_in(plan: &mut Plan, _today: &Today, id: &str) -> AppResult<Option<String>> {
    let before = plan.moves.len();
    plan.moves.retain(|m| m.id != id);
    if plan.moves.len() == before {
        return Err(AppError::not_found("move", id));
    }
    Ok(None)
}

/// A category or fund to make.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct NewItem {
    pub label: String,
    /// One of `equity_fund`, `equity_stock`, `debt_high`, `debt_low`, `gold`, `real_estate`, `other`.
    pub cls: String,
}

pub async fn add_category(state: &AppState, owner: i32, view: View, input: NewItem) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| add_category_in(plan, today, input)).await
}

fn add_category_in(plan: &mut Plan, _today: &Today, input: NewItem) -> AppResult<Option<String>> {
    check_text("label", &input.label, true)?;
    check_cls(&input.cls)?;
    let key = new_key("custom");
    plan.created.push(Created { key: key.clone(), label: input.label.trim().to_string(), cls: input.cls });
    Ok(Some(key))
}

/// Delete a category from the plan. One the plan made goes with every move
/// into or out of it, and its funds. One of today's has the moves into it
/// undone and is set aside - only once everything in it is planned out.
pub async fn remove_category(state: &AppState, owner: i32, view: View, key: &str) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| remove_category_in(plan, today, key)).await
}

fn remove_category_in(plan: &mut Plan, today: &Today, key: &str) -> AppResult<Option<String>> {
    if plan.created.iter().any(|c| c.key == key) {
        plan.created.retain(|c| c.key != key);
        plan.subs.retain(|s| s.bucket != key);
        plan.moves.retain(|m| m.from != key && m.to != key);
        return Ok(None);
    }
    if !today.has(key) {
        return Err(AppError::not_found("category", key));
    }
    let left = Model::of(today, plan).available(plan, key);
    if left > EPS {
        return Err(AppError::Conflict(format!(
            "'{key}' still holds {left:.2} {} not planned elsewhere; move it out first",
            today.base_currency
        )));
    }
    plan.moves.retain(|m| m.to != key);
    if !plan.removed.iter().any(|k| k == key) {
        plan.removed.push(key.to_string());
    }
    Ok(None)
}

pub async fn restore_category(state: &AppState, owner: i32, view: View, key: &str) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| restore_category_in(plan, today, key)).await
}

fn restore_category_in(plan: &mut Plan, _today: &Today, key: &str) -> AppResult<Option<String>> {
    let before = plan.removed.len();
    plan.removed.retain(|k| k != key);
    if plan.removed.len() == before {
        return Err(AppError::NotFound(format!("category {key} is not deleted from the plan")));
    }
    Ok(None)
}

/// Set a category's classification, or with `None` go back to what it holds
/// today. A category the plan made always has one.
pub async fn set_class(state: &AppState, owner: i32, view: View, key: &str, cls: Option<String>) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| set_class_in(plan, today, key, cls)).await
}

fn set_class_in(plan: &mut Plan, today: &Today, key: &str, cls: Option<String>) -> AppResult<Option<String>> {
    if let Some(c) = &cls {
        check_cls(c)?;
    }
    if let Some(made) = plan.created.iter_mut().find(|c| c.key == key) {
        made.cls = cls.ok_or_else(|| AppError::bad("a category the plan made needs a classification"))?;
        return Ok(None);
    }
    if !today.has(key) {
        return Err(AppError::not_found("category", key));
    }
    match cls {
        Some(c) => plan.cls.insert(key.to_string(), c),
        None => plan.cls.remove(key),
    };
    Ok(None)
}

/// A fund to make inside a category.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
pub struct NewSubInput {
    /// The category it goes in: one held today, or one the plan made.
    pub category: String,
    pub label: String,
    /// One of `equity_fund`, `equity_stock`, `debt_high`, `debt_low`, `gold`, `real_estate`, `other`.
    pub cls: String,
}

pub async fn add_sub(state: &AppState, owner: i32, view: View, input: NewSubInput) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| add_sub_in(plan, today, input)).await
}

fn add_sub_in(plan: &mut Plan, today: &Today, input: NewSubInput) -> AppResult<Option<String>> {
    if !today.has(&input.category) && !plan.created.iter().any(|c| c.key == input.category) {
        return Err(AppError::bad(format!(
            "category '{}' must be one of today's ({}) or one the plan made",
            input.category,
            today.names()
        )));
    }
    check_text("label", &input.label, true)?;
    check_cls(&input.cls)?;
    let key = new_key("sub");
    plan.subs.push(NewSub { key: key.clone(), bucket: input.category, label: input.label.trim().to_string(), cls: input.cls });
    Ok(Some(key))
}

/// Delete a fund the plan made, and undo the moves into it.
pub async fn remove_sub(state: &AppState, owner: i32, view: View, key: &str) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| remove_sub_in(plan, today, key)).await
}

fn remove_sub_in(plan: &mut Plan, _today: &Today, key: &str) -> AppResult<Option<String>> {
    let Some(bucket) = plan.subs.iter().find(|s| s.key == key).map(|s| s.bucket.clone()) else {
        return Err(AppError::not_found("fund the plan made", key));
    };
    plan.subs.retain(|s| s.key != key);
    plan.moves.retain(|m| !(m.to == bucket && m.to_sub.as_deref() == Some(key)));
    Ok(None)
}

pub async fn set_fresh(state: &AppState, owner: i32, view: View, amount: f64) -> AppResult<PlanState> {
    change(state, owner, view, |plan, today| set_fresh_in(plan, today, amount)).await
}

fn set_fresh_in(plan: &mut Plan, _today: &Today, amount: f64) -> AppResult<Option<String>> {
    if !amount.is_finite() || amount < 0.0 {
        return Err(AppError::bad("amount must be zero or more"));
    }
    plan.fresh = amount;
    Ok(None)
}

// ---------------------------------------------------------------------------
// Several changes at once: what an AI mode proposal holds
// ---------------------------------------------------------------------------

/// The most changes one batch may hold.
pub const MAX_CHANGES: usize = 50;

/// One change of a batch - the same as the single calls, named by `op`.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum PlanChange {
    AddMove(NewMove),
    RemoveMove { id: String },
    AddCategory(NewItem),
    RemoveCategory { key: String },
    RestoreCategory { key: String },
    SetClass { key: String, cls: Option<String> },
    AddSubcategory(NewSubInput),
    RemoveSubcategory { key: String },
    SetFresh { amount: f64 },
}

/// One change of a batch, in words, as the person approving it sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChangeLine {
    /// The change's `op`.
    #[schema(example = "add_move")]
    pub op: String,
    #[schema(example = "Cash · KOTAK BANK → Indian Mutual Funds")]
    pub text: String,
    /// In the base currency: what a move moves (a `pct` worked out), or fresh money set.
    pub amount: Option<f64>,
}

/// A change applied, with the keys its words are made of.
struct Step {
    op: &'static str,
    from: Option<(String, Option<String>)>,
    to: Option<(String, Option<String>)>,
    label: Option<String>,
    amount: Option<f64>,
}

/// Apply `changes` in order to `plan`, against today's values. A change may
/// name what it makes (`"as": "gold"`) and a later one use it (`"to":
/// "$gold"`). The first that fails fails them all, saying which.
fn batch_in(plan: &mut Plan, today: &Today, changes: &[Value]) -> AppResult<Vec<Step>> {
    if changes.is_empty() {
        return Err(AppError::bad("no changes: give at least one"));
    }
    if changes.len() > MAX_CHANGES {
        return Err(AppError::bad(format!("at most {MAX_CHANGES} changes at once")));
    }
    let mut named: BTreeMap<String, String> = BTreeMap::new();
    let mut steps = Vec::new();
    for (i, raw) in changes.iter().enumerate() {
        let n = i + 1;
        let fail = |e: AppError| AppError::bad(format!("change {n}: {e}"));
        let mut raw = raw.clone();
        let Some(obj) = raw.as_object_mut() else {
            return Err(fail(AppError::bad("must be an object with an `op`")));
        };
        let name = match obj.remove("as") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().trim_start_matches('$').to_string()),
            Some(_) => return Err(fail(AppError::bad("`as` must be a name"))),
        };
        for v in obj.values_mut() {
            if let Some(r) = v.as_str().and_then(|s| s.strip_prefix('$')) {
                let key = named.get(r).ok_or_else(|| fail(AppError::bad(format!("${r} is not named by an earlier change"))))?;
                *v = Value::String(key.clone());
            }
        }
        let change: PlanChange = serde_json::from_value(raw).map_err(|e| fail(AppError::bad(e.to_string())))?;
        let (made, step) = match change {
            PlanChange::AddMove(m) => {
                let (from, to) = ((m.from.clone(), m.from_sub.clone()), (m.to.clone(), m.to_sub.clone()));
                let made = add_move_in(plan, today, m).map_err(fail)?;
                let amount = plan.moves.last().map(|mv| mv.amount);
                (made, Step { op: "add_move", from: Some(from), to: Some(to), label: None, amount })
            }
            PlanChange::RemoveMove { id } => {
                let gone = plan.moves.iter().find(|m| m.id == id).map(|m| ((m.from.clone(), m.from_sub.clone()), (m.to.clone(), m.to_sub.clone()), m.amount));
                let made = remove_move_in(plan, today, &id).map_err(fail)?;
                let (from, to, amount) = gone.map_or((None, None, None), |(f, t, a)| (Some(f), Some(t), Some(a)));
                (made, Step { op: "remove_move", from, to, label: None, amount })
            }
            PlanChange::AddCategory(c) => {
                let label = format!("{} ({})", c.label.trim(), c.cls);
                (add_category_in(plan, today, c).map_err(fail)?, Step { op: "add_category", from: None, to: None, label: Some(label), amount: None })
            }
            PlanChange::RemoveCategory { key } => {
                let to = Some((key.clone(), None));
                (remove_category_in(plan, today, &key).map_err(fail)?, Step { op: "remove_category", from: None, to, label: None, amount: None })
            }
            PlanChange::RestoreCategory { key } => {
                let to = Some((key.clone(), None));
                (restore_category_in(plan, today, &key).map_err(fail)?, Step { op: "restore_category", from: None, to, label: None, amount: None })
            }
            PlanChange::SetClass { key, cls } => {
                let label = cls.clone().unwrap_or_else(|| "what it holds".to_string());
                let to = Some((key.clone(), None));
                (set_class_in(plan, today, &key, cls).map_err(fail)?, Step { op: "set_class", from: None, to, label: Some(label), amount: None })
            }
            PlanChange::AddSubcategory(s) => {
                let label = format!("{} ({})", s.label.trim(), s.cls);
                let to = Some((s.category.clone(), None));
                (add_sub_in(plan, today, s).map_err(fail)?, Step { op: "add_subcategory", from: None, to, label: Some(label), amount: None })
            }
            PlanChange::RemoveSubcategory { key } => {
                let to = plan.subs.iter().find(|s| s.key == key).map(|s| (s.bucket.clone(), Some(key.clone())));
                (remove_sub_in(plan, today, &key).map_err(fail)?, Step { op: "remove_subcategory", from: None, to, label: None, amount: None })
            }
            PlanChange::SetFresh { amount } => {
                (set_fresh_in(plan, today, amount).map_err(fail)?, Step { op: "set_fresh", from: None, to: None, label: None, amount: Some(amount) })
            }
        };
        if let Some(name) = name {
            let key = made.ok_or_else(|| fail(AppError::bad("`as` names what a change makes, and this one makes nothing")))?;
            named.insert(name, key);
        }
        steps.push(step);
    }
    validate(plan)?;
    Ok(steps)
}

/// Whether `changes` would apply to the plan as it is now, and what they
/// say in words. Nothing is written.
pub async fn check_changes(state: &AppState, owner: i32, changes: &[Value]) -> AppResult<Vec<ChangeLine>> {
    let view = View::Segments;
    let today = today(state, owner, view).await?;
    let mut plan = load(state, owner, view).await.plan;
    let steps = batch_in(&mut plan, &today, changes)?;
    describe(state, owner, &today, &plan, steps).await
}

/// Apply `changes` to the plan, all or none, checked again against today's
/// values and the plan as it is now.
pub async fn apply_changes(state: &AppState, owner: i32, changes: &[Value]) -> AppResult<PlanState> {
    change(state, owner, View::Segments, |plan, today| batch_in(plan, today, changes).map(|_| None)).await
}

/// The words for each step: categories and funds by their names.
async fn describe(state: &AppState, owner: i32, today: &Today, plan: &Plan, steps: Vec<Step>) -> AppResult<Vec<ChangeLine>> {
    let mut lines = Vec::with_capacity(steps.len());
    for s in steps {
        let from = place_label(state, owner, today, plan, s.from.as_ref()).await;
        let to = place_label(state, owner, today, plan, s.to.as_ref()).await;
        let text = match (s.op, from, to, s.label) {
            ("add_move", Some(f), Some(t), _) => format!("{f} → {t}"),
            ("remove_move", Some(f), Some(t), _) => format!("Undo the move {f} → {t}"),
            ("remove_move", ..) => "Undo a planned move".to_string(),
            ("add_category", _, _, Some(l)) => format!("New category: {l}"),
            ("remove_category", _, Some(t), _) => format!("Delete the category {t}"),
            ("restore_category", _, Some(t), _) => format!("Bring back the category {t}"),
            ("set_class", _, Some(t), Some(l)) => format!("Classify {t} as {l}"),
            ("add_subcategory", _, Some(t), Some(l)) => format!("New fund in {t}: {l}"),
            ("remove_subcategory", _, Some(t), _) => format!("Delete the fund {t}"),
            ("set_fresh", ..) => "Fresh money to invest".to_string(),
            (op, ..) => op.replace('_', " "),
        };
        lines.push(ChangeLine { op: s.op.to_string(), text, amount: s.amount });
    }
    Ok(lines)
}

/// A category, and the fund in it if one is named: "Cash · KOTAK BANK".
async fn place_label(
    state: &AppState,
    owner: i32,
    today: &Today,
    plan: &Plan,
    place: Option<&(String, Option<String>)>,
) -> Option<String> {
    let (key, sub) = place?;
    let category = if key == FRESH {
        "Fresh money".to_string()
    } else {
        today
            .rows
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, label, _)| label.clone())
            .or_else(|| plan.created.iter().find(|c| &c.key == key).map(|c| c.label.clone()))
            .unwrap_or_else(|| key.clone())
    };
    Some(match sub {
        Some(sub) => format!("{category} · {}", fund_label(state, owner, plan, sub).await),
        None => category,
    })
}

/// A fund or account in a category, by name: one the plan made, an
/// instrument, a cash account of the owner's, or a bank's deposits.
async fn fund_label(state: &AppState, owner: i32, plan: &Plan, key: &str) -> String {
    if let Some(s) = plan.subs.iter().find(|s| s.key == key) {
        return s.label.clone();
    }
    if let Some(bank) = key.strip_prefix("b:") {
        return format!("{bank} deposits");
    }
    let id = |p: &str| key.strip_prefix(p).and_then(|v| v.parse::<i32>().ok());
    if let Some(id) = id("i:") {
        if let Ok(Some(i)) = instrument::Entity::find_by_id(id).one(&state.db).await {
            return i.name;
        }
    }
    if let Some(id) = id("c:") {
        if let Ok(Some(a)) = account::Entity::find_by_id(id).one(&state.db).await {
            if a.user_id == owner {
                return a.name;
            }
        }
    }
    key.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Today {
        Today {
            base_currency: "INR".into(),
            rows: vec![("india_mf".into(), "Indian Mutual Funds".into(), 1000.0), ("cash".into(), "Cash".into(), 500.0)],
        }
    }

    fn mv(id: &str, from: &str, to: &str, amount: f64) -> Move {
        Move { id: id.into(), from: from.into(), from_sub: None, to: to.into(), to_sub: None, amount, note: String::new(), at: String::new() }
    }

    #[test]
    fn the_summary_applies_the_moves_as_the_page_does() {
        let plan = Plan {
            created: vec![Created { key: "custom-1".into(), label: "Gold".into(), cls: "gold".into() }],
            moves: vec![mv("a", "india_mf", "custom-1", 250.0), mv("b", FRESH, "cash", 100.0), mv("c", "gone", "cash", 9.0)],
            fresh: 300.0,
            ..Default::default()
        };
        let s = summarise(&today(), &plan);
        let line = |k: &str| s.categories.iter().find(|c| c.key == k).unwrap();
        assert_eq!((line("india_mf").planned, line("india_mf").available, line("india_mf").status.as_str()), (750.0, 750.0, "reduced"));
        assert_eq!((line("cash").planned, line("cash").status.as_str()), (600.0, "added"));
        assert_eq!((line("custom-1").planned, line("custom-1").status.as_str()), (250.0, "new"));
        assert_eq!((s.today_total, s.planned_total, s.fresh_left), (1500.0, 1600.0, 200.0));
    }

    #[test]
    fn a_plan_is_checked_for_its_own_consistency() {
        assert!(validate(&Plan::default()).is_ok());
        let bad_cls = Plan { cls: [("cash".to_string(), "crypto".to_string())].into(), ..Default::default() };
        assert!(validate(&bad_cls).is_err());
        let twice = Plan { moves: vec![mv("a", "cash", "india_mf", 1.0), mv("a", "cash", "india_mf", 1.0)], ..Default::default() };
        assert!(validate(&twice).is_err());
        let negative = Plan { moves: vec![mv("a", "cash", "india_mf", -1.0)], ..Default::default() };
        assert!(validate(&negative).is_err());
    }

    #[test]
    fn a_batch_applies_in_order_and_later_changes_use_what_earlier_ones_made() {
        let mut plan = Plan::default();
        let changes = vec![
            serde_json::json!({ "op": "add_category", "label": "Gold", "cls": "gold", "as": "gold" }),
            serde_json::json!({ "op": "add_move", "from": "cash", "to": "$gold", "amount": 200.0 }),
            serde_json::json!({ "op": "set_fresh", "amount": 100.0 }),
            serde_json::json!({ "op": "add_move", "from": FRESH, "to": "india_mf", "pct": 50.0 }),
        ];
        let steps = batch_in(&mut plan, &today(), &changes).unwrap();
        assert_eq!(steps.iter().map(|s| s.op).collect::<Vec<_>>(), vec!["add_category", "add_move", "set_fresh", "add_move"]);
        let gold = &plan.created[0].key;
        assert_eq!((plan.moves[0].to.as_str(), plan.moves[0].amount), (gold.as_str(), 200.0));
        assert_eq!(steps[3].amount, Some(50.0), "the pct worked out");

        // The second change asks for more than cash has: the batch fails, saying which.
        let mut plan = Plan::default();
        let too_much = vec![
            serde_json::json!({ "op": "add_move", "from": "cash", "to": "india_mf", "amount": 400.0 }),
            serde_json::json!({ "op": "add_move", "from": "cash", "to": "india_mf", "amount": 400.0 }),
        ];
        let e = batch_in(&mut plan, &today(), &too_much).err().unwrap().to_string();
        assert!(e.starts_with("change 2:"), "{e}");
        let unknown = vec![serde_json::json!({ "op": "add_move", "from": "cash", "to": "$nowhere", "amount": 1.0 })];
        assert!(batch_in(&mut Plan::default(), &today(), &unknown).is_err());
        assert!(batch_in(&mut Plan::default(), &today(), &[]).is_err());
        let fly = vec![serde_json::json!({ "op": "sell_everything" })];
        assert!(batch_in(&mut Plan::default(), &today(), &fly).is_err());
    }

    #[test]
    fn plans_saved_by_the_browser_still_read() {
        let raw = r#"{"created":[],"subs":[],"moves":[{"id":"x","from":"cash","fromSub":"c:1","to":"india_mf","amount":5,"note":"","at":""}],"fresh":0,"removed":[],"cls":{}}"#;
        let plan: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(plan.moves[0].from_sub.as_deref(), Some("c:1"));
        assert!(serde_json::to_string(&plan).unwrap().contains("\"from_sub\":\"c:1\""));
    }
}
