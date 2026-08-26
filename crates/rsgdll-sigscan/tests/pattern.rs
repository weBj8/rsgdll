use std::str::FromStr;

use rsgdll_sigscan::{Pattern, PatternError, ScanError};

#[test]
fn wildcard_pattern_finds_first_matching_offset() {
    let pattern = Pattern::from_str("48 8B ?? FF").expect("pattern parses");
    assert_eq!(
        pattern.find_unique_in(&[0x90, 0x48, 0x8B, 0x12, 0xFF, 0x48]),
        Ok(1)
    );
    assert_eq!(
        pattern.find_unique_in(&[0x48, 0x8B, 0x12]),
        Err(ScanError::NotFound)
    );
}

#[test]
fn ambiguous_patterns_are_rejected() {
    let pattern = Pattern::from_str("AA ??").expect("pattern parses");
    assert_eq!(
        pattern.find_unique_in(&[0xAA, 1, 0xAA, 2]),
        Err(ScanError::Ambiguous)
    );
}

#[test]
fn malformed_patterns_are_rejected() {
    assert_eq!(Pattern::from_str(""), Err(PatternError::Empty));
    assert_eq!(
        Pattern::from_str("48 xyz"),
        Err(PatternError::InvalidByte {
            index: 1,
            token: "xyz".to_owned(),
        })
    );
}
