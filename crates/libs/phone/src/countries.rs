//! The countries a person can name for a phone number written without its
//! `+` code, with what the number needs to be written in full.
//!
//! A country here is a fact a person or a source states, never a guess: the
//! import form's country for a run, or the country picked for one identity
//! (`docs/architecture/contacts-identities-and-messages.md`, "A phone number
//! carries its country"). Nothing in this crate picks one from the digits.

/// One country a phone number can belong to.
#[derive(Debug, PartialEq, Eq)]
pub struct Country {
    /// ISO 3166-1 alpha-2 code, upper case: `GB`, `US`. `XK` is Kosovo, which
    /// has a calling code and no ISO code of its own.
    pub code: &'static str,
    /// Country calling code without the `+`: `44`, `1`. Several countries
    /// share one (`1` is the United States, Canada and much of the
    /// Caribbean), because they share one numbering plan.
    pub calling_code: &'static str,
    /// The trunk prefix a national number is dialled with inside the country
    /// and dropped from its `+` form: `0` in most countries, `8` in Russia,
    /// `06` in Hungary, empty where the leading digit belongs to the number
    /// (Italy's `06` is part of a Rome number). The calling code `1` is
    /// written by its own rule and does not read this.
    pub trunk_prefix: &'static str,
}

/// Shorthand for a table row.
const fn c(code: &'static str, calling_code: &'static str, trunk_prefix: &'static str) -> Country {
    Country {
        code,
        calling_code,
        trunk_prefix,
    }
}

/// Every country, by ISO code.
///
/// The calling codes and trunk prefixes are the `countryCode` and
/// `nationalPrefix` attributes of each `<territory>` in libphonenumber's
/// `resources/PhoneNumberMetadata.xml`, at commit `d1457634c3d9`
/// (2026-10-07):
/// <https://github.com/google/libphonenumber/blob/d1457634c3d95e570dad433ee0ab400beb343a56/resources/PhoneNumberMetadata.xml>.
/// The table departs from it in two ways. The calling code `1` rows carry no
/// trunk prefix, because [`crate::normalize_checked`] writes a `+1` number by
/// its own rule; libphonenumber gives them `1`. `AC` (Ascension) and `TA`
/// (Tristan da Cunha) are left out, because ISO 3166-1 gives them no code of
/// their own. A trunk prefix is checked against that file before it is
/// changed, because a wrong one gives a number a `+` form that names nobody.
pub const COUNTRIES: &[Country] = &[
    c("AD", "376", ""),
    c("AE", "971", "0"),
    c("AF", "93", "0"),
    c("AG", "1", ""),
    c("AI", "1", ""),
    c("AL", "355", "0"),
    c("AM", "374", "0"),
    c("AO", "244", ""),
    c("AR", "54", "0"),
    c("AS", "1", ""),
    c("AT", "43", "0"),
    c("AU", "61", "0"),
    c("AW", "297", ""),
    c("AX", "358", "0"),
    c("AZ", "994", "0"),
    c("BA", "387", "0"),
    c("BB", "1", ""),
    c("BD", "880", "0"),
    c("BE", "32", "0"),
    c("BF", "226", ""),
    c("BG", "359", "0"),
    c("BH", "973", ""),
    c("BI", "257", ""),
    c("BJ", "229", ""),
    c("BL", "590", "0"),
    c("BM", "1", ""),
    c("BN", "673", ""),
    c("BO", "591", "0"),
    c("BQ", "599", ""),
    c("BR", "55", "0"),
    c("BS", "1", ""),
    c("BT", "975", ""),
    c("BW", "267", ""),
    c("BY", "375", "8"),
    c("BZ", "501", ""),
    c("CA", "1", ""),
    c("CC", "61", "0"),
    c("CD", "243", "0"),
    c("CF", "236", ""),
    c("CG", "242", ""),
    c("CH", "41", "0"),
    c("CI", "225", ""),
    c("CK", "682", ""),
    c("CL", "56", ""),
    c("CM", "237", ""),
    c("CN", "86", "0"),
    c("CO", "57", "0"),
    c("CR", "506", ""),
    c("CU", "53", "0"),
    c("CV", "238", ""),
    c("CW", "599", ""),
    c("CX", "61", "0"),
    c("CY", "357", ""),
    c("CZ", "420", ""),
    c("DE", "49", "0"),
    c("DJ", "253", ""),
    c("DK", "45", ""),
    c("DM", "1", ""),
    c("DO", "1", ""),
    c("DZ", "213", "0"),
    c("EC", "593", "0"),
    c("EE", "372", ""),
    c("EG", "20", "0"),
    c("EH", "212", "0"),
    c("ER", "291", "0"),
    c("ES", "34", ""),
    c("ET", "251", "0"),
    c("FI", "358", "0"),
    c("FJ", "679", ""),
    c("FK", "500", ""),
    c("FM", "691", ""),
    c("FO", "298", ""),
    c("FR", "33", "0"),
    c("GA", "241", ""),
    c("GB", "44", "0"),
    c("GD", "1", ""),
    c("GE", "995", "0"),
    c("GF", "594", "0"),
    c("GG", "44", "0"),
    c("GH", "233", "0"),
    c("GI", "350", ""),
    c("GL", "299", ""),
    c("GM", "220", ""),
    c("GN", "224", ""),
    c("GP", "590", "0"),
    c("GQ", "240", ""),
    c("GR", "30", ""),
    c("GT", "502", ""),
    c("GU", "1", ""),
    c("GW", "245", ""),
    c("GY", "592", ""),
    c("HK", "852", ""),
    c("HN", "504", ""),
    c("HR", "385", "0"),
    c("HT", "509", ""),
    c("HU", "36", "06"),
    c("ID", "62", "0"),
    c("IE", "353", "0"),
    c("IL", "972", "0"),
    c("IM", "44", "0"),
    c("IN", "91", "0"),
    c("IO", "246", ""),
    c("IQ", "964", "0"),
    c("IR", "98", "0"),
    c("IS", "354", ""),
    c("IT", "39", ""),
    c("JE", "44", "0"),
    c("JM", "1", ""),
    c("JO", "962", "0"),
    c("JP", "81", "0"),
    c("KE", "254", "0"),
    c("KG", "996", "0"),
    c("KH", "855", "0"),
    c("KI", "686", "0"),
    c("KM", "269", ""),
    c("KN", "1", ""),
    c("KP", "850", "0"),
    c("KR", "82", "0"),
    c("KW", "965", ""),
    c("KY", "1", ""),
    c("KZ", "7", "8"),
    c("LA", "856", "0"),
    c("LB", "961", "0"),
    c("LC", "1", ""),
    c("LI", "423", "0"),
    c("LK", "94", "0"),
    c("LR", "231", "0"),
    c("LS", "266", ""),
    c("LT", "370", "0"),
    c("LU", "352", ""),
    c("LV", "371", ""),
    c("LY", "218", "0"),
    c("MA", "212", "0"),
    c("MC", "377", "0"),
    c("MD", "373", "0"),
    c("ME", "382", "0"),
    c("MF", "590", "0"),
    c("MG", "261", "0"),
    c("MH", "692", "1"),
    c("MK", "389", "0"),
    c("ML", "223", ""),
    c("MM", "95", "0"),
    c("MN", "976", "0"),
    c("MO", "853", ""),
    c("MP", "1", ""),
    c("MQ", "596", "0"),
    c("MR", "222", ""),
    c("MS", "1", ""),
    c("MT", "356", ""),
    c("MU", "230", ""),
    c("MV", "960", ""),
    c("MW", "265", "0"),
    c("MX", "52", ""),
    c("MY", "60", "0"),
    c("MZ", "258", ""),
    c("NA", "264", "0"),
    c("NC", "687", ""),
    c("NE", "227", ""),
    c("NF", "672", ""),
    c("NG", "234", "0"),
    c("NI", "505", ""),
    c("NL", "31", "0"),
    c("NO", "47", ""),
    c("NP", "977", "0"),
    c("NR", "674", ""),
    c("NU", "683", ""),
    c("NZ", "64", "0"),
    c("OM", "968", ""),
    c("PA", "507", ""),
    c("PE", "51", "0"),
    c("PF", "689", ""),
    c("PG", "675", ""),
    c("PH", "63", "0"),
    c("PK", "92", "0"),
    c("PL", "48", ""),
    c("PM", "508", "0"),
    c("PR", "1", ""),
    c("PS", "970", "0"),
    c("PT", "351", ""),
    c("PW", "680", ""),
    c("PY", "595", "0"),
    c("QA", "974", ""),
    c("RE", "262", "0"),
    c("RO", "40", "0"),
    c("RS", "381", "0"),
    c("RU", "7", "8"),
    c("RW", "250", "0"),
    c("SA", "966", "0"),
    c("SB", "677", ""),
    c("SC", "248", ""),
    c("SD", "249", "0"),
    c("SE", "46", "0"),
    c("SG", "65", ""),
    c("SH", "290", ""),
    c("SI", "386", "0"),
    c("SJ", "47", ""),
    c("SK", "421", "0"),
    c("SL", "232", "0"),
    c("SM", "378", ""),
    c("SN", "221", ""),
    c("SO", "252", "0"),
    c("SR", "597", ""),
    c("SS", "211", "0"),
    c("ST", "239", ""),
    c("SV", "503", ""),
    c("SX", "1", ""),
    c("SY", "963", "0"),
    c("SZ", "268", ""),
    c("TC", "1", ""),
    c("TD", "235", ""),
    c("TG", "228", ""),
    c("TH", "66", "0"),
    c("TJ", "992", ""),
    c("TK", "690", ""),
    c("TL", "670", ""),
    c("TM", "993", "8"),
    c("TN", "216", ""),
    c("TO", "676", ""),
    c("TR", "90", "0"),
    c("TT", "1", ""),
    c("TV", "688", ""),
    c("TW", "886", "0"),
    c("TZ", "255", "0"),
    c("UA", "380", "0"),
    c("UG", "256", "0"),
    c("US", "1", ""),
    c("UY", "598", "0"),
    c("UZ", "998", ""),
    c("VA", "39", ""),
    c("VC", "1", ""),
    c("VE", "58", "0"),
    c("VG", "1", ""),
    c("VI", "1", ""),
    c("VN", "84", "0"),
    c("VU", "678", ""),
    c("WF", "681", ""),
    c("WS", "685", ""),
    c("XK", "383", "0"),
    c("YE", "967", "0"),
    c("YT", "262", "0"),
    c("ZA", "27", "0"),
    c("ZM", "260", "0"),
    c("ZW", "263", "0"),
];

/// The country with ISO code `code`, in any case. `None` for a code the
/// table does not hold.
#[must_use]
pub fn country(code: &str) -> Option<&'static Country> {
    let code = code.trim();
    COUNTRIES
        .iter()
        .find(|country| country.code.eq_ignore_ascii_case(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_code_is_listed_once_in_order() {
        let codes: Vec<&str> = COUNTRIES.iter().map(|c| c.code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn a_country_is_found_by_its_code_in_any_case() {
        assert_eq!(country("gb").map(|c| c.calling_code), Some("44"));
        assert_eq!(country(" US ").map(|c| c.calling_code), Some("1"));
        assert!(country("ZZ").is_none());
    }

    #[test]
    fn every_calling_code_is_digits() {
        let codes: HashSet<&str> = COUNTRIES.iter().map(|c| c.calling_code).collect();
        assert!(codes.iter().all(|c| c.chars().all(|d| d.is_ascii_digit())));
    }
}
