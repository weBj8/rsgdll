#[cfg(feature = "detour")]
#[test]
fn detour_feature_exposes_detour_subsystem() {
    assert_eq!(rsgdll::detour::PATCH_LEN, 14);
}

#[cfg(feature = "engine")]
#[test]
fn engine_feature_exposes_checked_engine_subsystem() {
    assert!(std::mem::size_of::<rsgdll::engine::Engine<'static>>() > 0);
}

#[cfg(feature = "sigscan")]
#[test]
fn sigscan_feature_exposes_signature_subsystem() {
    use std::str::FromStr;

    let pattern = rsgdll::sigscan::Pattern::from_str("AA ?? CC").expect("pattern parses");
    assert_eq!(pattern.find_unique_in(&[0xAA, 0x01, 0xCC]), Ok(0));
}
