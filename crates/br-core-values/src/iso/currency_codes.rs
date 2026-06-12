//! The authoritative ISO 4217 active alphabetic currency list.
//!
//! The complete list of alphabetic codes from the ISO 4217 active currency
//! list. Numeric codes and precious-metal codes (XAG, XAU, XPD, XPT) are
//! excluded — those are out of scope for monetary amounts. The list changes
//! rarely; a recompile when a code is added is a non-issue.

/// All active ISO 4217 alphabetic currency codes (uppercase).
pub const CURRENCY_CODES: [&str; 169] = [
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
    "BGN", "BHD", "BIF", "BMD", "BND", "BOB", "BOV", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD",
    "CAD", "CDF", "CHE", "CHF", "CHW", "CLF", "CLP", "CNY", "COP", "COU", "CRC", "CUC", "CUP",
    "CVE", "CZK", "DJF", "DKK", "DOP", "DZD", "EGP", "ERN", "ETB", "EUR", "FJD", "FKP", "GBP",
    "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ", "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS",
    "INR", "IQD", "IRR", "ISK", "JMD", "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW",
    "KWD", "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD",
    "MMK", "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN", "MXV", "MYR", "MZN", "NAD", "NGN",
    "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK", "PHP", "PKR", "PLN", "PYG", "QAR",
    "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG", "SEK", "SGD", "SHP", "SLE", "SOS",
    "SRD", "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS", "TMT", "TND", "TOP", "TRY", "TTD",
    "TWD", "TZS", "UAH", "UGX", "USD", "USN", "UYI", "UYU", "UYW", "UZS", "VED", "VES", "VND",
    "VUV", "WST", "XAF", "XCD", "XDR", "XOF", "XPF", "XSU", "XUA", "YER", "ZAR", "ZMW", "ZWG",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn array_has_exactly_169_entries() {
        assert_eq!(
            CURRENCY_CODES.len(),
            169,
            "CURRENCY_CODES must contain exactly 169 ISO 4217 active currency entries"
        );
    }

    #[test]
    fn no_duplicates() {
        let mut seen = HashSet::new();
        for code in &CURRENCY_CODES {
            assert!(
                seen.insert(*code),
                "duplicate entry in CURRENCY_CODES: {code}"
            );
        }
    }

    #[test]
    fn every_entry_is_three_ascii_uppercase_letters() {
        for code in &CURRENCY_CODES {
            assert_eq!(code.len(), 3, "{code} is not 3 chars");
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase()),
                "{code} is not all-uppercase ASCII"
            );
        }
    }
}
