//! Integration tests: load bundled profiles and verify native FB layout generation.

use st_comm_api::*;

fn load_bundled_profile(name: &str) -> DeviceProfile {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap()
        .join("profiles")
        .join(format!("{name}.yaml"));
    DeviceProfile::from_file(&path)
        .unwrap_or_else(|e| panic!("Failed to load profile {name}: {e}"))
}

#[test]
fn load_sim_io_profile() {
    let profile = load_bundled_profile("sim_8di_4ai_4do_2ao");
    assert_eq!(profile.name, "Sim8DI4AI4DO2AO");
    assert_eq!(profile.fields.len(), 18); // 8+4+4+2
    assert_eq!(profile.input_fields().len(), 12); // 8 DI + 4 AI
    assert_eq!(profile.output_fields().len(), 6); // 4 DO + 2 AO
}

#[test]
fn load_sim_vfd_profile() {
    let profile = load_bundled_profile("sim_vfd");
    assert_eq!(profile.name, "SimVfd");
    assert_eq!(profile.fields.len(), 11);
    assert_eq!(profile.input_fields().len(), 7);
    assert_eq!(profile.output_fields().len(), 4);
}

#[test]
fn profile_to_native_fb_layout() {
    let profile = load_bundled_profile("sim_8di_4ai_4do_2ao");
    let layout = profile.to_native_fb_layout();

    assert_eq!(layout.type_name, "Sim8DI4AI4DO2AO");

    // Expected: refresh_rate + 4 diag fields + 18 profile fields = 23
    assert_eq!(layout.fields.len(), 23);

    // First field is refresh_rate (VarInput)
    assert_eq!(layout.fields[0].name, "refresh_rate");
    assert_eq!(layout.fields[0].var_kind, NativeFbVarKind::VarInput);

    // Diagnostic fields
    assert_eq!(layout.fields[1].name, "connected");
    assert_eq!(layout.fields[2].name, "error_code");
    assert_eq!(layout.fields[3].name, "io_cycles");
    assert_eq!(layout.fields[4].name, "last_response_ms");

    // Profile fields start at index 5
    assert_eq!(layout.fields[5].name, "DI_0");
    assert_eq!(layout.fields[5].data_type, FieldDataType::Bool);
    assert_eq!(layout.fields[5].var_kind, NativeFbVarKind::Var);
}

#[test]
fn layout_to_memory_layout_roundtrip() {
    let profile = load_bundled_profile("sim_vfd");
    let layout = profile.to_native_fb_layout();
    let mem = layout_to_memory_layout(&layout);

    // All fields should be present in the memory layout
    assert_eq!(mem.slots.len(), layout.fields.len());

    // Check a few fields
    let (_, slot) = mem.find_slot("connected").expect("connected not found");
    assert_eq!(slot.ty, st_ir::VarType::Bool);

    let (_, slot) = mem.find_slot("SPEED_REF").expect("SPEED_REF not found");
    assert_eq!(slot.ty, st_ir::VarType::Real);

    let (_, slot) = mem.find_slot("RUN").expect("RUN not found");
    assert_eq!(slot.ty, st_ir::VarType::Bool);
}

#[test]
fn native_fb_registry_from_profiles() {
    let io = load_bundled_profile("sim_8di_4ai_4do_2ao");
    let vfd = load_bundled_profile("sim_vfd");

    let mut registry = NativeFbRegistry::new();

    // Use stub NativeFbs for testing (no SimulatedDevice dependency here)
    struct StubFb(NativeFbLayout);
    impl NativeFb for StubFb {
        fn type_name(&self) -> &str { &self.0.type_name }
        fn layout(&self) -> &NativeFbLayout { &self.0 }
        fn execute(&self, _: &mut [st_ir::Value]) {}
    }

    registry.register(Box::new(StubFb(io.to_native_fb_layout())));
    registry.register(Box::new(StubFb(vfd.to_native_fb_layout())));

    assert_eq!(registry.len(), 2);
    assert!(registry.find("Sim8DI4AI4DO2AO").is_some());
    assert!(registry.find("SimVfd").is_some());
    assert!(registry.find("nonexistent").is_none());
}
