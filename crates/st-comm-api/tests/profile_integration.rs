//! Integration tests: load bundled profiles, generate ST code, verify it parses.

use st_comm_api::*;
use std::collections::HashMap;

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
fn generate_and_parse_sim_io_code() {
    let profile = load_bundled_profile("sim_8di_4ai_4do_2ao");
    let mut profiles = HashMap::new();
    profiles.insert("sim_8di_4ai_4do_2ao".to_string(), profile);

    let devices = vec![DeviceConfig {
        name: "io_rack".to_string(),
        link: "sim_link".to_string(),
        protocol: "simulated".to_string(),
        unit_id: None,
        mode: "cyclic".to_string(),
        cycle_time: None,
        device_profile: "sim_8di_4ai_4do_2ao".to_string(),
        extra: Default::default(),
    }];

    let code = generate_st_code(&profiles, &devices);
    eprintln!("Generated ST code:\n{code}");

    // Each profile field becomes a flat global named {device}_{field}
    assert!(code.contains("io_rack_DI_0 : BOOL;"));
    assert!(code.contains("io_rack_DO_0 : BOOL;"));
    assert!(code.contains("io_rack_AI_0 : INT;"));

    let full = format!(
        "{code}\nPROGRAM Main\nVAR x : INT; END_VAR\n    IF io_rack_DI_0 THEN io_rack_DO_0 := TRUE; END_IF;\n    x := io_rack_AI_0;\nEND_PROGRAM\n"
    );

    let result = st_syntax::parse(&full);
    assert!(result.errors.is_empty(),
        "Generated code has parse errors: {:?}\n\n{full}", result.errors);
}

#[test]
fn generate_and_parse_multi_device_code() {
    let io_profile = load_bundled_profile("sim_8di_4ai_4do_2ao");
    let vfd_profile = load_bundled_profile("sim_vfd");

    let mut profiles = HashMap::new();
    profiles.insert("sim_8di_4ai_4do_2ao".to_string(), io_profile);
    profiles.insert("sim_vfd".to_string(), vfd_profile);

    let devices = vec![
        DeviceConfig {
            name: "rack_left".to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: "sim_8di_4ai_4do_2ao".to_string(),
            extra: Default::default(),
        },
        DeviceConfig {
            name: "rack_right".to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: "sim_8di_4ai_4do_2ao".to_string(),
            extra: Default::default(),
        },
        DeviceConfig {
            name: "pump_vfd".to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: "sim_vfd".to_string(),
            extra: Default::default(),
        },
    ];

    let code = generate_st_code(&profiles, &devices);

    let full = format!(
        "{code}\nPROGRAM Main\nVAR motor_on : BOOL; END_VAR\n\
        IF rack_left_DI_0 THEN rack_right_DO_0 := TRUE; END_IF;\n\
        pump_vfd_RUN := motor_on;\n\
        pump_vfd_SPEED_REF := 45.0;\n\
        END_PROGRAM\n"
    );

    let result = st_syntax::parse(&full);
    assert!(result.errors.is_empty(),
        "Generated code has parse errors: {:?}", result.errors);
}

#[test]
fn config_to_codegen_roundtrip() {
    let yaml = r#"
name: TestProject
entryPoint: Main

links:
  - name: sim_link
    type: simulated

devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
  - name: vfd
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_vfd
"#;
    let config = CommConfig::from_project_yaml(yaml).unwrap();
    assert_eq!(config.devices.len(), 2);

    // Load profiles
    let mut profiles = HashMap::new();
    for dev in &config.devices {
        let profile = load_bundled_profile(&dev.device_profile);
        profiles.insert(dev.device_profile.clone(), profile);
    }

    // Generate code: each profile field becomes a flat global named {device}_{field}
    let code = generate_st_code(&profiles, &config.devices);
    assert!(code.contains("io_rack_DI_0 : BOOL;"));
    assert!(code.contains("io_rack_AO_1 : INT;"));
    assert!(code.contains("vfd_SPEED_REF : REAL;"));
    assert!(code.contains("vfd_RUN : BOOL;"));
}
