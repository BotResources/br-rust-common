//! The authoritative ISO 3166-1 alpha-2 country list.
//!
//! The complete list as of the current standard: every sovereign state, every
//! dependent territory, every exceptional reservation. No subset, no "popular
//! countries only". The list changes rarely (~once every few years); a recompile
//! when a code is added is a non-issue.

/// All 249 ISO 3166-1 alpha-2 country codes (uppercase).
pub const COUNTRY_CODES: [&str; 249] = [
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN",
    "CO", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
    "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FK", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF",
    "GG", "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY", "HK", "HM",
    "HN", "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT", "JE", "JM",
    "JO", "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC",
    "LI", "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK",
    "ML", "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA",
    "NC", "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE", "PF", "PG",
    "PH", "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW",
    "SA", "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS",
    "ST", "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO",
    "TR", "TT", "TV", "TW", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE", "VG", "VI",
    "VN", "VU", "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // The list must be sorted: `CountryCode::new` relies on `binary_search`. This
    // guards the precondition so it cannot silently regress on an edit.
    #[test]
    fn codes_are_sorted() {
        assert!(
            COUNTRY_CODES.is_sorted(),
            "COUNTRY_CODES must stay sorted — CountryCode::new binary-searches it"
        );
    }

    #[test]
    fn no_duplicates() {
        let mut seen = HashSet::new();
        for code in &COUNTRY_CODES {
            assert!(
                seen.insert(*code),
                "duplicate entry in COUNTRY_CODES: {code}"
            );
        }
    }

    #[test]
    fn every_entry_is_two_ascii_uppercase_letters() {
        for code in &COUNTRY_CODES {
            assert_eq!(code.len(), 2, "{code} is not 2 chars");
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase()),
                "{code} is not all-uppercase ASCII"
            );
        }
    }

    // Content vectors prove "the list is current", not "the list has the length
    // I typed". PRESENCE of recent additions:
    // - SS (South Sudan, 2011); BQ/CW/SX (successors of the 2010 dissolution of
    //   the Netherlands Antilles).
    #[test]
    fn contains_recently_added_codes() {
        for code in &["SS", "BQ", "CW", "SX"] {
            assert!(
                COUNTRY_CODES.binary_search(code).is_ok(),
                "{code} (a recent ISO 3166-1 addition) is missing"
            );
        }
    }

    // ABSENCE of retired / non-official codes:
    // - AN (Netherlands Antilles, retired 2010), CS (Serbia and Montenegro,
    //   retired 2006), XK (Kosovo — a user-assigned code, NOT an official ISO
    //   3166-1 element; it must never leak into the active list).
    #[test]
    fn excludes_retired_and_unofficial_codes() {
        for code in &["AN", "CS", "XK"] {
            assert!(
                COUNTRY_CODES.binary_search(code).is_err(),
                "{code} is retired/unofficial and must not be in the active list"
            );
        }
    }
}
