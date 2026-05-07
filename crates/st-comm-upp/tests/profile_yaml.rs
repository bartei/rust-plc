//! Acceptance tests that load the **shipped** reference profile
//! YAML from `profiles/impac_igar_6_smart.yaml` and walk it all the
//! way through the resolver pipeline.
//!
//! These pin the contract between the YAML on disk and the runtime
//! types. If a future change drops a decoder name from
//! `Decoder::resolve` or rewords a command mnemonic, these tests
//! break loudly instead of letting a silently-broken profile ship.

use st_comm_api::native_fb::NativeFb;
use st_comm_api::profile::{DeviceProfile, FieldDirection};
use st_comm_upp::device_fb::UppDeviceNativeFb;
use st_comm_upp::profile_binding;
use std::path::PathBuf;
use std::sync::Arc;

fn fixture_path() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for a test binary is the crate dir
    // (`crates/st-comm-upp`). Walk up two levels to the workspace
    // root, then into `profiles/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("profiles")
        .join("impac_igar_6_smart.yaml")
}

#[test]
fn shipped_igar_profile_parses_and_resolves() {
    let path = fixture_path();
    assert!(
        path.exists(),
        "expected the shipped reference profile at {}",
        path.display()
    );
    let yaml = std::fs::read_to_string(&path).expect("read shipped profile");
    let profile = DeviceProfile::from_yaml(&yaml).expect("YAML parses");

    assert_eq!(profile.protocol.as_deref(), Some("upp"));
    assert!(!profile.fields.is_empty(), "profile must have fields");

    // Every field's `upp:` block must resolve cleanly. A typo or
    // unsupported decoder name MUST fail this test rather than
    // surface as ERR_PROFILE at runtime.
    for pf in &profile.fields {
        assert!(
            pf.register.is_none(),
            "UPP field {:?} must not carry register:",
            pf.name
        );
        assert!(
            pf.upp.is_some(),
            "UPP field {:?} must carry upp: binding",
            pf.name
        );
        let _binding = profile_binding::resolve(pf).unwrap_or_else(|e| {
            panic!("field {:?} failed to resolve: {e}", pf.name)
        });
    }
}

#[test]
fn shipped_profile_constructs_a_valid_fb() {
    // Full smoke: parse the YAML, build the FB, check the layout
    // shape. Constructing the FB exercises every binding via
    // `profile_binding::resolve` in the FB constructor.
    let yaml = std::fs::read_to_string(fixture_path()).unwrap();
    let profile = DeviceProfile::from_yaml(&yaml).unwrap();
    let bus = Arc::new(st_comm_serial::BusManager::new(
        st_comm_serial::new_transport_map(),
    ));
    let fb = UppDeviceNativeFb::new(profile.clone(), bus);

    let layout = fb.layout();
    // Layout must include the 10-field fixed prefix plus one slot
    // per profile field.
    assert_eq!(
        layout.fields.len(),
        10 + profile.fields.len(),
        "layout = 10 fixed + {} profile fields",
        profile.fields.len(),
    );
    // The very first field is always `link`.
    assert_eq!(layout.fields[0].name, "link");
    // Diagnostics block.
    assert_eq!(layout.fields[5].name, "connected");
    assert_eq!(layout.fields[6].name, "error_code");
    assert_eq!(layout.fields[9].name, "last_response_ms");
    // Profile fields appear after slot 9.
    let layout_field_names: Vec<&str> = layout
        .fields
        .iter()
        .skip(10)
        .map(|f| f.name.as_str())
        .collect();
    let profile_field_names: Vec<&str> =
        profile.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(layout_field_names, profile_field_names);
}

/// The reference profile must include at least one `inout` writable
/// field — pyrometer programs commonly tune emissivity from ST code.
/// Pinning this lets us catch a future profile shrink that
/// accidentally drops every writable field (the FB would still build
/// but the write path would never get exercised).
#[test]
fn shipped_profile_has_at_least_one_writable_field() {
    let yaml = std::fs::read_to_string(fixture_path()).unwrap();
    let profile = DeviceProfile::from_yaml(&yaml).unwrap();
    let writable = profile
        .fields
        .iter()
        .filter(|f| {
            matches!(
                f.direction,
                FieldDirection::Output | FieldDirection::Inout
            )
        })
        .count();
    assert!(
        writable > 0,
        "shipped profile has zero writable fields — at least `emissivity` should be inout"
    );
    let has_em = profile
        .fields
        .iter()
        .any(|f| f.name == "emissivity" && f.direction == FieldDirection::Inout);
    assert!(has_em, "shipped profile must expose emissivity as inout");
}
