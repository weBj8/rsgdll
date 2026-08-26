use std::path::Path;

use xtask::{Realm, artifact_name, stage_artifact};

#[test]
fn artifact_name_maps_every_gmod_target_variant() {
    // Given: both realms and every target naming variant.
    let cases = [
        (
            Realm::Server,
            "i686-unknown-linux-gnu",
            "gmsv_name_linux.dll",
        ),
        (
            Realm::Server,
            "x86_64-unknown-linux-gnu",
            "gmsv_name_linux64.dll",
        ),
        (Realm::Server, "i686-pc-windows-msvc", "gmsv_name_win32.dll"),
        (
            Realm::Server,
            "x86_64-pc-windows-msvc",
            "gmsv_name_win64.dll",
        ),
        (
            Realm::Client,
            "i686-unknown-linux-gnu",
            "gmcl_name_linux.dll",
        ),
        (
            Realm::Client,
            "x86_64-unknown-linux-gnu",
            "gmcl_name_linux64.dll",
        ),
        (Realm::Client, "i686-pc-windows-msvc", "gmcl_name_win32.dll"),
        (
            Realm::Client,
            "x86_64-pc-windows-msvc",
            "gmcl_name_win64.dll",
        ),
    ];

    // When: each target is converted to its loader-facing name.
    // Then: GMod's exact prefix and suffix are selected.
    for (realm, target, expected) in cases {
        assert_eq!(
            artifact_name(realm, "name", target),
            Ok(expected.to_owned())
        );
    }
}

#[test]
fn artifact_name_rejects_unreviewed_targets() {
    // Given: a target outside the explicit naming matrix.
    // When: its artifact name is requested.
    let result = artifact_name(Realm::Server, "name", "aarch64-unknown-linux-gnu");

    // Then: staging fails instead of implying runtime support.
    assert_eq!(
        result,
        Err("unsupported GMod artifact target: aarch64-unknown-linux-gnu".to_owned())
    );
}

#[test]
fn stage_artifact_copies_to_loader_name() {
    // Given: one built library and an empty staging directory.
    let root = std::env::temp_dir().join(format!("rsgdll-xtask-{}", std::process::id()));
    let source = root.join("librsgdll_example.so");
    let destination = root.join("garrysmod/lua/bin");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&source, b"module").unwrap();

    // When: the artifact is staged for Linux x86_64 server use.
    let staged = stage_artifact(
        Realm::Server,
        "example",
        "x86_64-unknown-linux-gnu",
        &source,
        &destination,
    )
    .unwrap();

    // Then: its bytes exist under the exact GMod loader filename.
    assert_eq!(staged, destination.join("gmsv_example_linux64.dll"));
    assert_eq!(std::fs::read(staged).unwrap(), b"module");
    assert!(Path::new(&source).exists());

    std::fs::remove_dir_all(root).unwrap();
}
