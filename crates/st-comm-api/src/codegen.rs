//! ST code generator: converts device profiles + project config into
//! auto-generated TYPE declarations and VAR_GLOBAL instances.

use crate::config::DeviceConfig;
use crate::profile::DeviceProfile;
use std::collections::HashMap;

/// Generate ST source code from device profiles and device configs.
///
/// Produces:
/// 1. `TYPE CommDiag` struct (diagnostics, included in every device struct)
/// 2. One `TYPE` struct per unique device profile
/// 3. `VAR_GLOBAL` instances — one per configured device
pub fn generate_st_code(
    profiles: &HashMap<String, DeviceProfile>,
    devices: &[DeviceConfig],
) -> String {
    let mut out = String::new();

    out.push_str("(* Auto-generated from device profiles — DO NOT EDIT *)\n\n");

    // CommDiag struct (included in every device)
    out.push_str("TYPE CommDiag : STRUCT\n");
    out.push_str("    connected    : BOOL;\n");
    out.push_str("    error        : BOOL;\n");
    out.push_str("    error_count  : DINT;\n");
    out.push_str("    response_ms  : INT;\n");
    out.push_str("END_STRUCT;\n\n");

    // Collect unique profiles used by devices
    let mut used_profiles: Vec<&str> = Vec::new();
    for dev in devices {
        if !used_profiles.contains(&dev.device_profile.as_str()) {
            used_profiles.push(&dev.device_profile);
        }
    }

    // Generate TYPE struct for each profile
    for profile_name in &used_profiles {
        if let Some(profile) = profiles.get(*profile_name) {
            out.push_str(&format!(
                "(* Profile: {} *)\n",
                profile.description.as_deref().unwrap_or(&profile.name)
            ));
            out.push_str(&format!("{} : STRUCT\n", profile.name));
            for field in &profile.fields {
                let comment = match (&field.register.unit, &field.description) {
                    (Some(unit), _) => format!("    (* {}, {:?} *)", unit, field.direction),
                    (_, Some(desc)) => format!("    (* {desc} *)"),
                    _ => format!("    (* {:?} *)", field.direction),
                };
                out.push_str(&format!(
                    "    {} : {};{}\n",
                    field.name,
                    field.data_type.st_type_name(),
                    comment
                ));
            }
            out.push_str("    _diag : CommDiag;\n");
            out.push_str("END_STRUCT;\n\n");
        }
    }

    out.push_str("END_TYPE\n\n");

    // Generate VAR_GLOBAL instances
    out.push_str("VAR_GLOBAL\n");
    for dev in devices {
        if let Some(profile) = profiles.get(&dev.device_profile) {
            let comment = format!(
                "(* {}, {} *)",
                dev.link,
                dev.protocol
            );
            out.push_str(&format!(
                "    {} : {};    {}\n",
                dev.name, profile.name, comment
            ));
        }
    }
    out.push_str("END_VAR\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::*;

    fn make_test_profile() -> DeviceProfile {
        DeviceProfile::from_yaml(r#"
name: TestIO
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: coil } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: coil } }
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: input_register } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, kind: coil } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#).unwrap()
    }

    #[test]
    fn generate_basic_code() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);

        let devices = vec![
            DeviceConfig {
                name: "rack_left".to_string(),
                link: "sim_link".to_string(),
                protocol: "simulated".to_string(),
                unit_id: None,
                mode: "cyclic".to_string(),
                cycle_time: None,
                device_profile: "test_io".to_string(),
                extra: Default::default(),
            },
        ];

        let code = generate_st_code(&profiles, &devices);
        assert!(code.contains("TYPE CommDiag : STRUCT"));
        assert!(code.contains("TestIO : STRUCT"));
        assert!(code.contains("DI_0 : BOOL;"));
        assert!(code.contains("AI_0 : INT;"));
        assert!(code.contains("_diag : CommDiag;"));
        assert!(code.contains("rack_left : TestIO;"));
    }

    #[test]
    fn generate_multiple_instances_same_profile() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);

        let devices = vec![
            DeviceConfig {
                name: "rack_a".to_string(),
                link: "link1".to_string(),
                protocol: "simulated".to_string(),
                unit_id: None,
                mode: "cyclic".to_string(),
                cycle_time: None,
                device_profile: "test_io".to_string(),
                extra: Default::default(),
            },
            DeviceConfig {
                name: "rack_b".to_string(),
                link: "link2".to_string(),
                protocol: "simulated".to_string(),
                unit_id: None,
                mode: "cyclic".to_string(),
                cycle_time: None,
                device_profile: "test_io".to_string(),
                extra: Default::default(),
            },
        ];

        let code = generate_st_code(&profiles, &devices);
        // Profile TYPE should appear only once
        assert_eq!(code.matches("TestIO : STRUCT").count(), 1);
        // But two instances
        assert!(code.contains("rack_a : TestIO;"));
        assert!(code.contains("rack_b : TestIO;"));
    }

    #[test]
    fn generated_code_parses_as_valid_st() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);

        let devices = vec![DeviceConfig {
            name: "io".to_string(),
            link: "sim".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: "test_io".to_string(),
            extra: Default::default(),
        }];

        let code = generate_st_code(&profiles, &devices);

        // Add a simple program that uses the generated types
        let full_source = format!(
            "{code}\nPROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := io.DI_0;\nEND_PROGRAM\n"
        );

        // Verify it parses without errors
        let result = st_syntax::parse(&full_source);
        assert!(
            result.errors.is_empty(),
            "Generated ST code has parse errors: {:?}\n\nGenerated code:\n{full_source}",
            result.errors
        );
    }
}
