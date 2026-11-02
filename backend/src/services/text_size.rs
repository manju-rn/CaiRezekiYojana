//! Text size: how much bigger, or smaller, the page and the AI chat are
//! drawn. The size itself is each browser's own; what is kept here are the
//! steps a click on the text-size pill goes through and the limits its
//! slider keeps to.
//!
//! App-wide, for administrators: `text_size_steps` (default `5,10,15` - a
//! click goes 100% -> 105% -> 110% -> 115% -> 100%), and `text_size_min` /
//! `text_size_max` (default 85% and 130%). A user may set their own
//! `text_size_steps`, each within the administrator's largest size; steps
//! that a later, lower limit leaves too big give way to the app-wide ones.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde::Serialize;

use crate::state::AppState;

use super::prefs;

pub const STEPS_KEY: &str = "text_size_steps";
pub const MIN_KEY: &str = "text_size_min";
pub const MAX_KEY: &str = "text_size_max";
pub const KEYS: [&str; 3] = [STEPS_KEY, MIN_KEY, MAX_KEY];

pub const DEFAULT_STEPS: &str = "5,10,15";
pub const DEFAULT_MIN: u32 = 85;
pub const DEFAULT_MAX: u32 = 130;
/// Where an administrator may put the limits, in per cent.
pub const MIN_RANGE: RangeInclusive<u32> = 50..=100;
pub const MAX_RANGE: RangeInclusive<u32> = 100..=200;
/// The most steps a click goes through before it is back at 100%.
pub const MOST_STEPS: usize = 5;

/// What the text-size pills work with, in per cent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct TextSize {
    /// Each click's step up from 100%, smallest first; after the last, back to 100%.
    #[schema(example = json!([5, 10, 15]))]
    pub steps: Vec<u32>,
    /// The smallest size the slider goes to.
    #[schema(example = 85)]
    pub min: u32,
    /// The largest size the slider, or a step, goes to.
    #[schema(example = 130)]
    pub max: u32,
}

/// `"5, 10, 15"` -> `[5, 10, 15]`: 1 to [`MOST_STEPS`] whole numbers, going
/// up, each at least 1 and none past `max` (a step of 15 needs a largest
/// size of 115% or more).
pub fn parse_steps(value: &str, max: u32) -> Result<Vec<u32>, String> {
    let steps = value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('%').trim().parse::<u32>().map_err(|_| format!("{s} is not a whole number of per cent")))
        .collect::<Result<Vec<u32>, String>>()?;
    if steps.is_empty() || steps.len() > MOST_STEPS {
        return Err(format!("give 1 to {MOST_STEPS} steps, such as 5, 10, 15"));
    }
    let most = max.saturating_sub(100);
    if steps.iter().any(|s| *s == 0 || *s > most) {
        return Err(if most == 0 {
            format!("the largest text size is {max}%, so there is no step up")
        } else {
            format!("each step must be 1-{most}%: the largest text size is {max}%")
        });
    }
    if steps.windows(2).any(|w| w[0] >= w[1]) {
        return Err("the steps must go up, such as 5, 10, 15".into());
    }
    Ok(steps)
}

fn number(value: Option<&String>, default: u32, range: &RangeInclusive<u32>) -> u32 {
    value.and_then(|v| v.trim().parse().ok()).filter(|n| range.contains(n)).unwrap_or(default)
}

/// The app-wide settings as stored, and the account's own steps if any.
/// Pure, so it can be tested without a database.
pub fn resolve(app: &BTreeMap<String, String>, own: Option<&str>) -> TextSize {
    let min = number(app.get(MIN_KEY), DEFAULT_MIN, &MIN_RANGE);
    let max = number(app.get(MAX_KEY), DEFAULT_MAX, &MAX_RANGE);
    let steps = own
        .and_then(|v| parse_steps(v, max).ok())
        .or_else(|| app.get(STEPS_KEY).and_then(|v| parse_steps(v, max).ok()))
        .unwrap_or_else(|| {
            // The defaults, less any a low limit leaves too big.
            let most = max.saturating_sub(100);
            parse_steps(DEFAULT_STEPS, 200).unwrap_or_default().into_iter().filter(|s| *s <= most).collect()
        });
    TextSize { steps, min, max }
}

async fn stored(state: &AppState) -> BTreeMap<String, String> {
    let mut app = BTreeMap::new();
    for key in KEYS {
        if let Some(v) = state.setting_opt(key).await.filter(|v| !v.trim().is_empty()) {
            app.insert(key.to_string(), v);
        }
    }
    app
}

/// The app-wide text size, for everyone - visitors included.
pub async fn app(state: &AppState) -> TextSize {
    resolve(&stored(state).await, None)
}

/// The text size for one user: their own steps, if they set any that fit.
pub async fn for_user(state: &AppState, user_id: i32) -> TextSize {
    let own = prefs::get_opt(&state.db, user_id, STEPS_KEY).await;
    resolve(&stored(state).await, own.as_deref())
}

/// An administrator's change to the text-size settings, with the stored
/// ones it leaves as they are: each limit in its range, and the steps
/// under the largest size. A blank value goes back to the default.
pub fn validate_app(changes: &BTreeMap<String, String>, stored: &BTreeMap<String, String>) -> Result<(), String> {
    if !KEYS.iter().any(|k| changes.contains_key(*k)) {
        return Ok(());
    }
    let get = |key: &str| changes.get(key).or_else(|| stored.get(key)).map(|v| v.trim()).filter(|v| !v.is_empty());
    let limit = |key: &str, default: u32, range: &RangeInclusive<u32>| -> Result<u32, String> {
        match get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u32>()
                .ok()
                .filter(|n| range.contains(n))
                .ok_or_else(|| format!("{key} must be a whole number from {} to {}", range.start(), range.end())),
        }
    };
    limit(MIN_KEY, DEFAULT_MIN, &MIN_RANGE)?;
    let max = limit(MAX_KEY, DEFAULT_MAX, &MAX_RANGE)?;
    parse_steps(get(STEPS_KEY).unwrap_or(DEFAULT_STEPS), max).map_err(|e| format!("{STEPS_KEY}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn steps_go_up_within_the_largest_size() {
        assert_eq!(parse_steps("5, 10, 15", 130), Ok(vec![5, 10, 15]));
        assert_eq!(parse_steps("2%,5%,7%", 130), Ok(vec![2, 5, 7]));
        assert!(parse_steps("5, 10, 35", 130).is_err(), "past 130%");
        assert!(parse_steps("10, 5", 130).is_err(), "not going up");
        assert!(parse_steps("0, 5", 130).is_err());
        assert!(parse_steps("", 130).is_err());
        assert!(parse_steps("1,2,3,4,5,6", 130).is_err(), "too many");
        assert!(parse_steps("five", 130).is_err());
        assert!(parse_steps("5", 100).is_err(), "no room above 100%");
    }

    #[test]
    fn the_defaults_and_the_accounts_own_steps() {
        assert_eq!(resolve(&BTreeMap::new(), None), TextSize { steps: vec![5, 10, 15], min: 85, max: 130 });
        let app = map(&[(STEPS_KEY, "2,5,7"), (MIN_KEY, "90"), (MAX_KEY, "120")]);
        assert_eq!(resolve(&app, None), TextSize { steps: vec![2, 5, 7], min: 90, max: 120 });
        assert_eq!(resolve(&app, Some("10,20")).steps, vec![10, 20], "their own");
        assert_eq!(resolve(&app, Some("10,25")).steps, vec![2, 5, 7], "past the limit: the app-wide ones");
        let low = map(&[(MAX_KEY, "108")]);
        assert_eq!(resolve(&low, None).steps, vec![5], "the defaults that fit");
        let bad = map(&[(MIN_KEY, "20"), (MAX_KEY, "500")]);
        assert_eq!((resolve(&bad, None).min, resolve(&bad, None).max), (85, 130), "out of range: the defaults");
    }

    #[test]
    fn an_administrator_keeps_the_steps_under_the_limit() {
        let stored = map(&[(STEPS_KEY, "5,10,15")]);
        assert!(validate_app(&map(&[(MAX_KEY, "120")]), &stored).is_ok());
        assert!(validate_app(&map(&[(MAX_KEY, "110")]), &stored).is_err(), "15 no longer fits");
        assert!(validate_app(&map(&[(MAX_KEY, "110"), (STEPS_KEY, "3,6,9")]), &stored).is_ok(), "both at once");
        assert!(validate_app(&map(&[(MIN_KEY, "40")]), &stored).is_err());
        assert!(validate_app(&map(&[(MAX_KEY, "250")]), &stored).is_err());
        assert!(validate_app(&map(&[(STEPS_KEY, "")]), &stored).is_ok(), "blank: the default");
        assert!(validate_app(&map(&[("session_days", "9")]), &map(&[(MAX_KEY, "x")])).is_ok(), "not touched");
    }
}
