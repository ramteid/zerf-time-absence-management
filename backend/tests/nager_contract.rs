//! Contract tests for the Nager.Date public holiday API.
//!
//! These tests make real HTTP requests to <https://date.nager.at> and assert
//! that the response structure and content remain compatible with the
//! assumptions this application makes about the external service.
//!
//! They run on every CI build and on a weekly schedule so that a breaking
//! change in the external API is caught even when no application code is
//! being changed.
//!
//! Run locally with:
//! ```sh
//! cargo test --test nager_contract --nocapture
//! ```

use chrono::Datelike as _;

const BASE: &str = "https://date.nager.at/api/v3";

async fn get_json(url: &str) -> serde_json::Value {
    let resp = reqwest::get(url).await.expect("HTTP request failed");
    assert!(
        resp.status().is_success(),
        "unexpected HTTP {} for {url}",
        resp.status()
    );
    resp.json().await.expect("response is not valid JSON")
}

// ── /AvailableCountries ──────────────────────────────────────────────────────

/// The endpoint must return a non-empty JSON array.
/// Each element must have a 2-letter uppercase `countryCode` and a non-empty
/// `name`.  Countries the app specifically relies on must be present.
#[tokio::test]
async fn available_countries_structure() {
    let body = get_json(&format!("{BASE}/AvailableCountries")).await;
    let arr = body
        .as_array()
        .expect("AvailableCountries must be a JSON array");
    assert!(!arr.is_empty(), "AvailableCountries must not be empty");

    for item in arr {
        let code = item["countryCode"]
            .as_str()
            .expect("each country must have a 'countryCode' string field");
        let name = item["name"]
            .as_str()
            .expect("each country must have a 'name' string field");

        assert_eq!(
            code.len(),
            2,
            "countryCode must be exactly 2 characters: {code}"
        );
        assert!(
            code.chars().all(|c| c.is_ascii_uppercase()),
            "countryCode must be uppercase ASCII: {code}"
        );
        assert!(
            !name.is_empty(),
            "name must not be empty for countryCode {code}"
        );
    }
}

/// Countries the application relies on must be present in the response.
#[tokio::test]
async fn available_countries_required_entries_present() {
    const REQUIRED: &[&str] = &["AT", "CH", "CZ", "DE", "FR", "GB", "IT", "NL", "PL", "US"];

    let body = get_json(&format!("{BASE}/AvailableCountries")).await;
    let arr = body.as_array().expect("array");

    let codes: std::collections::HashSet<&str> = arr
        .iter()
        .filter_map(|o| o["countryCode"].as_str())
        .collect();

    for expected in REQUIRED {
        assert!(
            codes.contains(expected),
            "required country '{expected}' is missing from AvailableCountries"
        );
    }
}

// ── /PublicHolidays/{year}/{country} ────────────────────────────────────────

/// Every field the application reads must be present and have the correct type.
#[tokio::test]
async fn public_holidays_de_field_types() {
    let year = chrono::Utc::now().year();
    let body = get_json(&format!("{BASE}/PublicHolidays/{year}/DE")).await;
    let arr = body
        .as_array()
        .expect("PublicHolidays must be a JSON array");
    assert!(
        !arr.is_empty(),
        "PublicHolidays for DE/{year} must not be empty"
    );

    for item in arr {
        // date — must be a parseable YYYY-MM-DD string whose year matches the request
        let date_str = item["date"].as_str().expect("date must be a string");
        let parsed_date = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .unwrap_or_else(|_| panic!("date is not a valid YYYY-MM-DD: {date_str}"));
        assert_eq!(
            parsed_date.year(),
            year,
            "holiday date {date_str} is not in the requested year {year}"
        );

        // localName / name — must be non-empty strings
        let local_name = item["localName"]
            .as_str()
            .expect("localName must be a string");
        assert!(
            !local_name.is_empty(),
            "localName must not be empty ({date_str})"
        );
        let name = item["name"].as_str().expect("name must be a string");
        assert!(!name.is_empty(), "name must not be empty ({date_str})");

        // countryCode — must be the requested country
        assert_eq!(
            item["countryCode"]
                .as_str()
                .expect("countryCode must be a string"),
            "DE",
            "countryCode must be 'DE' ({date_str})"
        );

        // global — boolean
        assert!(
            item["global"].is_boolean(),
            "global must be a boolean ({date_str})"
        );

        // types — non-empty array of strings
        let types = item["types"].as_array().expect("types must be an array");
        assert!(!types.is_empty(), "types must not be empty ({date_str})");
        for t in types {
            assert!(
                t.as_str().is_some(),
                "each type entry must be a string ({date_str})"
            );
        }

        // counties — null or array of ISO 3166-2 strings starting with "DE-"
        match &item["counties"] {
            serde_json::Value::Null => {}
            serde_json::Value::Array(counties) => {
                assert!(
                    !counties.is_empty(),
                    "counties must not be an empty array ({date_str}) — use null for nation-wide"
                );
                for c in counties {
                    let code = c.as_str().expect("county entry must be a string");
                    assert!(
                        code.starts_with("DE-") && code.len() > 3,
                        "county code must follow 'DE-<subdivision>' format: {code}"
                    );
                }
            }
            other => panic!("counties must be null or an array, got: {other:?}"),
        }
    }
}

/// All county codes appearing in DE holidays must be valid Bundesland codes.
/// Every one of the 16 Bundesländer must appear in at least one holiday's
/// county list.  If Nager removes or renames a subdivision code, the region
/// filter in `fetch_holidays_from_api` would silently return wrong results
/// for users who have that region saved in their settings.
///
/// Note: this test intentionally does NOT fail when Nager adds new codes —
/// new subdivisions are handled automatically by the dynamic region endpoint.
#[tokio::test]
async fn public_holidays_de_all_bundeslaender_codes_present() {
    // The 16 German Bundesländer as used by Nager.Date (ISO 3166-2:DE).
    const EXPECTED: &[&str] = &[
        "DE-BB", "DE-BE", "DE-BW", "DE-BY", "DE-HB", "DE-HE", "DE-HH", "DE-MV", "DE-NI", "DE-NW",
        "DE-RP", "DE-SH", "DE-SL", "DE-SN", "DE-ST", "DE-TH",
    ];

    let year = chrono::Utc::now().year();
    let body = get_json(&format!("{BASE}/PublicHolidays/{year}/DE")).await;
    let arr = body.as_array().expect("array");

    let mut present: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for item in arr {
        if let Some(counties) = item["counties"].as_array() {
            for c in counties {
                if let Some(s) = c.as_str() {
                    present.insert(s);
                }
            }
        }
    }

    // Every known Bundesland code must still appear somewhere in the data.
    // A missing code means Nager removed or renamed it, which would silently
    // break holiday filtering for users with that region configured.
    for code in EXPECTED {
        assert!(
            present.contains(code),
            "Bundesland code '{code}' is no longer present in DE/{year} holidays — \
             Nager may have removed or renamed this subdivision"
        );
    }
}

/// Well-known fixed German public holidays must appear every year.
#[tokio::test]
async fn public_holidays_de_fixed_holidays_present() {
    let year = chrono::Utc::now().year();
    let body = get_json(&format!("{BASE}/PublicHolidays/{year}/DE")).await;
    let arr = body.as_array().expect("array");

    let dates: std::collections::HashSet<&str> =
        arr.iter().filter_map(|h| h["date"].as_str()).collect();

    // New Year's Day and Christmas Day are fixed public holidays in all states.
    for expected in [format!("{year}-01-01"), format!("{year}-12-25")] {
        assert!(
            dates.contains(expected.as_str()),
            "fixed public holiday {expected} is missing from DE/{year}"
        );
    }
}

/// New Year's Day must have `counties: null`.
///
/// The production code uses `counties: null` — not the `global` flag — to
/// decide that a holiday applies nation-wide.  If Nager ever changed the
/// `counties` field for a known global holiday, filtering would silently
/// exclude it for all region-specific users.  We test `counties`, not
/// `global`, because that is the exact field the production code reads.
#[tokio::test]
async fn public_holidays_de_new_years_day_has_null_counties() {
    let year = chrono::Utc::now().year();
    let body = get_json(&format!("{BASE}/PublicHolidays/{year}/DE")).await;
    let arr = body.as_array().expect("array");

    let new_year = format!("{year}-01-01");
    let entry = arr
        .iter()
        .find(|h| h["date"].as_str() == Some(new_year.as_str()))
        .unwrap_or_else(|| panic!("New Year's Day ({new_year}) not found"));

    assert!(
        entry["counties"].is_null(),
        "New Year's Day must have counties: null (nation-wide); \
         got: {:?}",
        entry["counties"]
    );
}

// ── /PublicHolidays for additional app-required countries ───────────────────

/// The holiday endpoint must respond successfully for every country the app
/// lists as supported, and must return at least one holiday.
#[tokio::test]
async fn public_holidays_all_required_countries_reachable() {
    const REQUIRED: &[&str] = &["AT", "CH", "CZ", "DE", "FR", "GB", "IT", "NL", "PL", "US"];
    let year = chrono::Utc::now().year();

    for cc in REQUIRED {
        let url = format!("{BASE}/PublicHolidays/{year}/{cc}");
        let resp = reqwest::get(&url).await.expect("request failed");
        assert!(
            resp.status().is_success(),
            "unexpected HTTP {} for {cc} holidays",
            resp.status()
        );
        let arr: Vec<serde_json::Value> = resp.json().await.expect("JSON parse failed");
        assert!(
            !arr.is_empty(),
            "PublicHolidays for {cc}/{year} must not be empty"
        );
    }
}
