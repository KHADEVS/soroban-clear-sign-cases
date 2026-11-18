use soroban_clear_sign_cases::case::{to_text, Case};
use soroban_clear_sign_cases::descriptor::parse_strict;
use soroban_clear_sign_cases::render::{clean_symbol, duration, rfc3339, scaled};
use std::fs;
use std::path::Path;

#[test]
fn every_case_still_renders_as_recorded() {
    let mut n = 0;
    for entry in fs::read_dir("cases").unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let case: Case = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let outcome = case.run(Path::new(".")).unwrap();
        assert_eq!(case.expected.as_ref(), Some(&outcome), "{}", case.id);
        assert_eq!(
            fs::read_to_string(path.with_extension("txt")).unwrap(),
            to_text(&outcome),
            "{}",
            case.id
        );
        n += 1;
    }
    assert!(n >= 13);
}

#[test]
fn amounts_scale_exactly() {
    assert_eq!(scaled(false, 1_000_000_000, 7), "100");
    assert_eq!(scaled(false, 27_796_236, 7), "2.7796236");
    assert_eq!(scaled(false, 5, 7), "0.0000005");
    assert_eq!(scaled(true, 1_250_000, 7), "-0.125");
    assert_eq!(scaled(false, 0, 7), "0");
    assert_eq!(
        scaled(false, u128::MAX, 38),
        "3.40282366920938463463374607431768211455"
    );
}

#[test]
fn percent_examples_of_the_draft() {
    // §3.4: decimals 4, value 30 -> 0.3%; decimals 7, value 1250000 -> 12.5%
    assert_eq!(format!("{}%", scaled(false, 30, 4 - 2)), "0.3%");
    assert_eq!(format!("{}%", scaled(false, 1_250_000, 7 - 2)), "12.5%");
}

#[test]
fn timestamps_and_durations() {
    assert_eq!(
        rfc3339(1_795_442_400).as_deref(),
        Some("2026-11-23T14:00:00Z")
    );
    assert_eq!(rfc3339(0).as_deref(), Some("1970-01-01T00:00:00Z"));
    assert_eq!(
        rfc3339(253_402_300_799).as_deref(),
        Some("9999-12-31T23:59:59Z")
    );
    assert_eq!(rfc3339(253_402_300_800), None);
    assert_eq!(rfc3339(u64::MAX), None);
    // §3.3's examples
    assert_eq!(duration(2_592_000), "30d");
    assert_eq!(duration(5_400), "1h 30m");
    assert_eq!(duration(90_061), "1d 1h 1m 1s");
    assert_eq!(duration(0), "0s");
}

#[test]
fn duplicate_keys_are_refused_at_any_depth() {
    assert!(parse_strict(r#"{"a":1,"b":{"c":1,"c":2}}"#).is_err());
    assert!(parse_strict(r#"{"a":[{"x":1},{"x":2}]}"#).is_ok());
}

#[test]
fn symbols_are_sanitized() {
    assert_eq!(clean_symbol("USDC").as_deref(), Some("USDC"));
    assert_eq!(clean_symbol("USD\u{202e}C"), None);
    assert_eq!(clean_symbol("A\nB"), None);
    assert_eq!(
        clean_symbol(&"X".repeat(40)).as_deref(),
        Some(format!("{}…", "X".repeat(32)).as_str())
    );
}
