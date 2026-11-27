//! Hegel property tests for `bajan::query` date encoding — the
//! `date-roundtrip-and-canonical-acceptance` property
//! (antithesis/scratchbook/date-roundtrip-and-canonical-acceptance.md).
//!
//! Purpose: prove the two halves of the dates property: (1) exhaustive
//! roundtrip, `days_from_iso(iso_from_days(d)) == Some(d)` for every day in
//! the years 1..=9999 domain, and (2) canonical acceptance, the generated
//! half — if `days_from_iso(s) == Some(d)` then `s == iso_from_days(d)`:
//! the parser accepts *only* the canonical `YYYY-MM-DD` calendar form
//! `iso_from_days` produces.
//!
//! Responsibilities: own the dates half of the `hegel_test` target; keep the
//! exhaustive half live (predicted green — both directions are Hinnant's
//! civil-days algorithms); gate the canonical-acceptance half and the two
//! targeted red probes behind `#[ignore = "bug bajan-4rj: ..."]` because
//! they fail today (confirmed by reading `src/query.rs:62-79`).
//!
//! Rationale: derived domain — `iso_from_days` is "days after 1970-01-01"
//! (epoch checked at `src/query.rs:40-44`); proleptic Gregorian day 0 is
//! 1970-01-01, `0001-01-01` is day `-719_162`, and `9999-12-31` is day
//! `2_932_896`, so the full years-1..=9999 domain spans 3,652,059 days,
//! matching the plan's ~3,650,000-iteration figure. The generated half
//! draws length-10 byte strings whose bytes 4 and 7 are `-` (the pre-filter
//! at `src/query.rs:64-65`; other shapes exit on the first branch and never
//! reach the parsing logic) with leading `+`/`-` year forms, non-ASCII
//! filler in digit slots, and month/day values 0..=99, plus canonical-form
//! mutations with impossible (month, day) pairs. Downstream effect of the
//! defect: `data_cutoff` filtering (`src/query.rs:243,380`) feeds
//! `days_from_iso` output into staleness arithmetic, so an accepted
//! non-canonical date shifts cutoff comparisons silently.

use bajan::query::{days_from_iso, iso_from_days};
use hegel::generators as gs;

// ---- half 1: exhaustive roundtrip (live, predicted green) ----

/// Proleptic Gregorian days since 1970-01-01 for `0001-01-01` (year 1
/// starts 1,971,162 days before the Unix epoch).
const YEAR_1_FIRST_DAY: i64 = -719_162;

/// Proleptic Gregorian days since 1970-01-01 for `9999-12-31`.
const YEAR_9999_LAST_DAY: i64 = 2_932_896;

/// Exhaustive roundtrip over the full years 1..=9999 day domain
/// (3,652,059 iterations — milliseconds): every encoding decodes back.
/// Both directions are Hinnant's civil-from-days / days-from-civil
/// algorithms (`src/query.rs:44-60` and `:72-79`), which claim exact
/// inversion; the loop pins that claim for the whole supported domain.
#[test]
fn every_day_in_years_1_to_9999_roundtrips_through_canonical_iso() {
    for d in YEAR_1_FIRST_DAY..=YEAR_9999_LAST_DAY {
        let text = iso_from_days(d);
        assert_eq!(
            days_from_iso(&text),
            Some(d),
            "roundtrip failed for day {d} encoded as {text:?}"
        );
    }
}

// ---- half 2: canonical acceptance (generated, ignored red: bajan-4rj) ----

/// The invariant: if `days_from_iso` accepts `text`, `text` is exactly the
/// canonical form `iso_from_days` produces for the decoded value.
fn assert_accepted_form_is_canonical(text: &str) {
    if let Some(d) = days_from_iso(text) {
        let canonical = iso_from_days(d);
        assert_eq!(
            text, canonical,
            "days_from_iso accepted non-canonical date {text:?} (decodes to {d}, \
             canonical form is {canonical:?})"
        );
    }
}

/// Length-10 byte strings whose bytes 4 and 7 are `-` (the pre-filter
/// passes them to the parsing logic), built per-slot so digits, leading
/// `+`/`-` year forms, non-ASCII filler, and month/day values 0..=99 are
/// all reachable. Because the two dash bytes are ASCII, the three subslice
/// boundaries are char boundaries regardless of filler, so non-ASCII cases
/// can only yield `None` (vacuous) or expose a defect — no panic to hunt.
///
/// Canonical-form mutations: valid `iso_from_days` outputs with the
/// month/day pair moved to an impossible calendar date — `02-30` (February
/// never has 30 days, even in leap years), `31` in the 30-day months
/// (April, June, September, November), and `02-30` again for the 31-day
/// months (whose only impossible *day* values fall outside the parser's
/// 1..=31 gate, so the month is mutated instead to keep the defect
/// reachable).
///
/// Ignored red: `"+026".parse::<i64>()` succeeds (leading `+`) so the
/// non-canonical year form is accepted (`src/query.rs:67-68`), and the
/// only calendar validation is `month in 1..=12 && day in 1..=31`
/// (`src/query.rs:70-71`) so impossible calendar dates are accepted.
#[hegel::test]
#[ignore = "bug bajan-4rj: days_from_iso accepts non-canonical dates"]
fn generated_iso_parses_are_canonical(tc: hegel::TestCase) {
    const DIGITS: [char; 10] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
    // Non-ASCII filler mixed into digit slots: guaranteed char-boundary
    // safe (dashes at bytes 4 and 7), so these either parse-fail to `None`
    // or expose a defect.
    const FILLER: [char; 13] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'é', 'β', '日',
    ];

    // Generators are single-use (moved into each `tc.draw`), so every slot
    // builds a fresh one.
    let mut chars: Vec<char> = Vec::with_capacity(10);
    // Year slot 0: leading `+`/`-` sign forms mixed with digits.
    chars.push(tc.draw(gs::sampled_from(
        vec!['+', '-'].into_iter().chain(DIGITS).collect::<Vec<_>>(),
    )));
    for _ in 0..3 {
        chars.push(tc.draw(gs::sampled_from(FILLER.to_vec())));
    }
    chars.push(tc.draw(gs::just('-')));
    for _ in 0..2 {
        chars.push(tc.draw(gs::sampled_from(FILLER.to_vec())));
    }
    chars.push(tc.draw(gs::just('-')));
    for _ in 0..2 {
        chars.push(tc.draw(gs::sampled_from(FILLER.to_vec())));
    }
    let arbitrary: String = chars.into_iter().collect();
    tc.note(&format!("arbitrary shape: {arbitrary:?}"));
    assert_accepted_form_is_canonical(&arbitrary);

    // ---- canonical-form mutations with impossible (month, day) pairs ----
    let days = gs::integers::<i64>()
        .min_value(YEAR_1_FIRST_DAY)
        .max_value(YEAR_9999_LAST_DAY);
    let d = tc.draw(days);
    let text = iso_from_days(d);
    let year = &text[0..4];
    let month = &text[5..7];
    // Month lengths (proleptic Gregorian, non-leap February = 28).
    let bad_pair: (i64, i64) = match month.parse::<i64>().expect("canonical month") {
        2 => (2, 30), // February never has 30 days.
        4 | 6 | 9 | 11 => (month.parse().expect("canonical month"), 31),
        // 31-day months have no impossible day inside the 1..=31 gate;
        // mutate to February 30 (impossible in any year) to stay reachable.
        _ => (2, 30),
    };
    let mutated = format!("{year}-{:02}-{:02}", bad_pair.0, bad_pair.1);
    tc.note(&format!(
        "canonical {text:?} mutated to impossible {mutated:?}"
    ));
    assert_accepted_form_is_canonical(&mutated);
}

// ---- half 3: targeted red probes (one per defect, ignored red: bajan-4rj) ----

/// A leading `+` year form is not canonical output of `iso_from_days` (which
/// emits `{y:04}`), so `days_from_iso("+026-01-01")` must be `None`. Today
/// the pre-filter (`src/query.rs:64-65`) passes it and
/// `"+026".parse::<i64>()` succeeds (`src/query.rs:67-68`), so it returns
/// `Some` — the defect this probe documents.
#[test]
#[ignore = "bug bajan-4rj: days_from_iso accepts non-canonical dates"]
fn days_from_iso_rejects_leading_plus_year_forms() {
    assert_eq!(days_from_iso("+026-01-01"), None);
}

/// `2024-02-30` is not a real calendar date, so `days_from_iso` must be
/// `None` (the doc contract: "malformed values are never guessed into a
/// date"). Today the only calendar gate is `month in 1..=12 && day in
/// 1..=31` (`src/query.rs:70-71`) with no month-length or leap-year
/// validation, so it returns `Some` — the defect this probe documents.
#[test]
#[ignore = "bug bajan-4rj: days_from_iso accepts non-canonical dates"]
fn days_from_iso_rejects_non_calendar_dates() {
    assert_eq!(days_from_iso("2024-02-30"), None);
}
