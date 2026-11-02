//! Which asset pages a user has hidden, and which of those are also
//! left out of the dashboard.
//!
//! Each page has one setting, `visibility_<page>`, with three values:
//! `visible` (the default), `hidden` - off the navigation but still counted -
//! and `excluded`, which also takes it out of every dashboard figure. Hiding
//! is the frontend's business; only exclusion reaches the numbers.

use crate::entities::instrument;
use crate::error::AppResult;
use crate::state::AppState;

use super::portfolio::Scope;

/// The pages that can be hidden, as their setting suffix. Dashboard and
/// Settings always show.
pub const PAGES: &[&str] = &["india_equity", "india_mf", "global", "deposits", "cash"];

pub const MODES: &[&str] = &["visible", "hidden", "excluded"];

pub fn setting_key(page: &str) -> String {
    format!("visibility_{page}")
}

/// The page a `visibility_<page>` key names, if it is one.
pub fn page_of(key: &str) -> Option<&'static str> {
    let page = key.strip_prefix("visibility_")?;
    PAGES.iter().copied().find(|p| *p == page)
}

/// The pages left out of the dashboard, in `PAGES` order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Excluded(Vec<&'static str>);

impl Excluded {
    /// The pages `user` has left out - or had left out for them by an
    /// administrator's override.
    pub async fn load(state: &AppState, user: i32) -> AppResult<Self> {
        let pages = super::prefs::visibility(&state.db, user).await?;
        Ok(Self(
            PAGES.iter().copied().filter(|p| pages.iter().any(|v| v.page == *p && v.mode == "excluded")).collect(),
        ))
    }

    #[cfg(test)]
    pub fn of(pages: &[&'static str]) -> Self {
        Self(PAGES.iter().copied().filter(|p| pages.contains(p)).collect())
    }

    pub fn page(&self, page: &str) -> bool {
        self.0.contains(&page)
    }

    pub fn scope(&self, scope: Scope) -> bool {
        match scope {
            Scope::All => false,
            Scope::IndiaEquity => self.page("india_equity"),
            Scope::IndiaMf => self.page("india_mf"),
            Scope::Global => self.page("global"),
        }
    }

    /// An instrument whose page is excluded.
    pub fn instrument(&self, inst: &instrument::Model) -> bool {
        [Scope::IndiaEquity, Scope::IndiaMf, Scope::Global]
            .into_iter()
            .any(|s| self.scope(s) && s.matches(inst))
    }

    pub fn pages(&self) -> Vec<String> {
        self.0.iter().map(|p| p.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_pages_have_a_setting() {
        assert_eq!(page_of("visibility_global"), Some("global"));
        assert_eq!(page_of("visibility_dashboard"), None);
        assert_eq!(page_of("base_currency"), None);
    }

    #[test]
    fn exclusion_maps_onto_holdings_scopes() {
        let e = Excluded::of(&["global", "cash"]);
        assert!(e.scope(Scope::Global));
        assert!(!e.scope(Scope::IndiaMf));
        assert!(!e.scope(Scope::All), "the whole is never excluded");
        assert_eq!(e.pages(), vec!["global", "cash"]);
    }
}
