//! ST code generator: converts device profiles + project config into
//! an auto-generated I/O mapping file with `VAR_GLOBAL` declarations.
//!
//! The output is intended to be written to disk (e.g. `_io_map.st`) so
//! that the LSP, semantic checker, compiler, and runtime all see the
//! same set of communication globals. The file doubles as a human-
//! readable symbols/mapping table — every field is annotated with its
//! source profile, direction, register address/kind, and engineering
//! units, similar to the symbol mapping tables in Codesys/TwinCAT.
//!
//! # Naming convention
//!
//! Each profile field becomes a flat global named `{instance}_{field}`.
//! Flat globals are used (instead of struct types) because the
//! compiler currently maps fields by global slot, not by struct field
//! offset for VAR_GLOBAL instances.

use crate::config::DeviceConfig;
use crate::profile::{DeviceProfile, FieldDirection, ProfileField};
use std::collections::HashMap;
use std::path::Path;

/// Filename for the auto-generated I/O map. Lives at the project root.
pub const IO_MAP_FILENAME: &str = "_io_map.st";

/// Write the generated I/O map to `{project_root}/_io_map.st`.
///
/// Skips the write if the file already exists with identical content,
/// so it doesn't trigger LSP re-parses or filesystem watchers needlessly.
/// Returns the absolute path of the file written (or that already matched).
pub fn write_io_map_file(
    project_root: &Path,
    profiles: &HashMap<String, DeviceProfile>,
    devices: &[DeviceConfig],
) -> Result<std::path::PathBuf, String> {
    let path = project_root.join(IO_MAP_FILENAME);
    let new_contents = generate_st_code(profiles, devices);

    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == new_contents {
            return Ok(path);
        }
    }

    std::fs::write(&path, &new_contents)
        .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// Build the flat global name for a device field.
pub fn global_name(instance: &str, field: &str) -> String {
    format!("{instance}_{field}")
}

/// Generate ST source code for all configured devices.
///
/// Produces an annotated `VAR_GLOBAL ... END_VAR` block. Each device gets
/// its own section with a banner comment, a column-aligned mapping table
/// (in comments), and one global declaration per profile field.
pub fn generate_st_code(
    profiles: &HashMap<String, DeviceProfile>,
    devices: &[DeviceConfig],
) -> String {
    let mut out = String::new();

    out.push_str("(* ============================================================ *)\n");
    out.push_str("(*  AUTO-GENERATED I/O MAP - DO NOT EDIT BY HAND                *)\n");
    out.push_str("(*                                                              *)\n");
    out.push_str("(*  This file is regenerated from plc-project.yaml + device     *)\n");
    out.push_str("(*  profiles every time `st-cli run` or the debugger starts.    *)\n");
    out.push_str("(*  It declares one VAR_GLOBAL per profile field, named         *)\n");
    out.push_str("(*  `{device}_{field}`. Use these globals in your ST code to    *)\n");
    out.push_str("(*  read inputs and drive outputs.                              *)\n");
    out.push_str("(* ============================================================ *)\n\n");

    if devices.is_empty() {
        out.push_str("(* No devices configured. *)\n");
        return out;
    }

    out.push_str("VAR_GLOBAL\n");

    for (i, dev) in devices.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let Some(profile) = profiles.get(&dev.device_profile) else {
            out.push_str(&format!(
                "    (* device '{}' references unknown profile '{}' *)\n",
                dev.name, dev.device_profile
            ));
            continue;
        };

        write_device_header(&mut out, dev, profile);
        write_mapping_table(&mut out, dev, profile);

        for field in &profile.fields {
            write_field_declaration(&mut out, dev, field);
        }
    }

    out.push_str("END_VAR\n");
    out
}

fn write_device_header(out: &mut String, dev: &DeviceConfig, profile: &DeviceProfile) {
    out.push_str("    (* -------------------------------------------------------- *)\n");
    out.push_str(&format!(
        "    (*  DEVICE: {:<20} PROFILE: {} *)\n",
        dev.name, profile.name
    ));
    out.push_str(&format!(
        "    (*  link={}  protocol={}  mode={} *)\n",
        dev.link, dev.protocol, dev.mode
    ));
    if let Some(ref desc) = profile.description {
        out.push_str(&format!("    (*  {desc} *)\n"));
    }
    if let Some(ref vendor) = profile.vendor {
        out.push_str(&format!("    (*  vendor: {vendor} *)\n"));
    }
    out.push_str("    (* -------------------------------------------------------- *)\n");
}

fn write_mapping_table(out: &mut String, dev: &DeviceConfig, profile: &DeviceProfile) {
    let max_field = profile
        .fields
        .iter()
        .map(|f| f.name.len())
        .max()
        .unwrap_or(8);
    let max_global = profile
        .fields
        .iter()
        .map(|f| dev.name.len() + 1 + f.name.len())
        .max()
        .unwrap_or(16);

    out.push_str("    (*\n");
    out.push_str(&format!(
        "        {:<g_w$}  {:<f_w$}  {:<6}  {:<4}  {:<14}  {}\n",
        "GLOBAL",
        "FIELD",
        "DIR",
        "TYPE",
        "REGISTER",
        "UNIT",
        g_w = max_global,
        f_w = max_field,
    ));
    out.push_str(&format!(
        "        {}  {}  {}  {}  {}  {}\n",
        "-".repeat(max_global),
        "-".repeat(max_field),
        "------",
        "----",
        "--------------",
        "----",
    ));
    for field in &profile.fields {
        let global = global_name(&dev.name, &field.name);
        let dir = match field.direction {
            FieldDirection::Input => "in",
            FieldDirection::Output => "out",
            FieldDirection::Inout => "inout",
        };
        let reg = format!("{:?}@{}", field.register.kind, field.register.address);
        let unit = field.register.unit.as_deref().unwrap_or("-");
        out.push_str(&format!(
            "        {:<g_w$}  {:<f_w$}  {:<6}  {:<4}  {:<14}  {}\n",
            global,
            field.name,
            dir,
            field.data_type.st_type_name(),
            reg,
            unit,
            g_w = max_global,
            f_w = max_field,
        ));
    }
    out.push_str("    *)\n");
}

fn write_field_declaration(out: &mut String, dev: &DeviceConfig, field: &ProfileField) {
    let global = global_name(&dev.name, &field.name);
    let dir = match field.direction {
        FieldDirection::Input => "in",
        FieldDirection::Output => "out",
        FieldDirection::Inout => "inout",
    };
    let unit = field
        .register
        .unit
        .as_deref()
        .map(|u| format!(", {u}"))
        .unwrap_or_default();
    let scale = field
        .register
        .scale
        .map(|s| format!(", scale={s}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "    {} : {};    (* {dir}{unit}{scale} *)\n",
        global,
        field.data_type.st_type_name(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::*;

    fn make_test_profile() -> DeviceProfile {
        DeviceProfile::from_yaml(r#"
name: TestIO
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: virtual } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: virtual } }
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: virtual } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, kind: virtual } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: virtual } }
"#).unwrap()
    }

    fn make_device(name: &str, profile: &str) -> DeviceConfig {
        DeviceConfig {
            name: name.to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: profile.to_string(),
            extra: Default::default(),
        }
    }

    #[test]
    fn generate_basic_code() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);
        let devices = vec![make_device("rack_left", "test_io")];

        let code = generate_st_code(&profiles, &devices);
        assert!(code.contains("AUTO-GENERATED I/O MAP"));
        assert!(code.contains("VAR_GLOBAL"));
        assert!(code.contains("rack_left_DI_0 : BOOL;"));
        assert!(code.contains("rack_left_AI_0 : INT;"));
        assert!(code.contains("rack_left_DO_0 : BOOL;"));
        assert!(code.contains("rack_left_AO_0 : INT;"));
        assert!(code.contains("END_VAR"));
        // The mapping table should be present (in comment form)
        assert!(code.contains("GLOBAL"));
        assert!(code.contains("REGISTER"));
    }

    #[test]
    fn generate_multiple_instances_same_profile() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);
        let devices = vec![
            make_device("rack_a", "test_io"),
            make_device("rack_b", "test_io"),
        ];

        let code = generate_st_code(&profiles, &devices);
        assert!(code.contains("rack_a_DI_0 : BOOL;"));
        assert!(code.contains("rack_b_DI_0 : BOOL;"));
    }

    #[test]
    fn generated_code_parses_as_valid_st() {
        let profile = make_test_profile();
        let mut profiles = HashMap::new();
        profiles.insert("test_io".to_string(), profile);
        let devices = vec![make_device("io", "test_io")];

        let code = generate_st_code(&profiles, &devices);
        let full_source = format!(
            "{code}\nPROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := io_AI_0;\n    io_DO_0 := io_DI_0;\nEND_PROGRAM\n"
        );

        let result = st_syntax::parse(&full_source);
        assert!(
            result.errors.is_empty(),
            "Generated ST code has parse errors: {:?}\n\nGenerated code:\n{full_source}",
            result.errors
        );
    }

    #[test]
    fn empty_devices_produces_valid_code() {
        let profiles = HashMap::new();
        let devices: Vec<DeviceConfig> = Vec::new();
        let code = generate_st_code(&profiles, &devices);
        // Should still parse as valid ST when followed by a real program
        let prog = format!("{code}\nPROGRAM Main\nVAR x : INT; END_VAR\n    x := 1;\nEND_PROGRAM\n");
        let result = st_syntax::parse(&prog);
        assert!(
            result.errors.is_empty(),
            "errors: {:?}\n--- source ---\n{prog}",
            result.errors
        );
    }
}
