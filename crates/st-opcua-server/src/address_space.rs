//! OPC-UA address space builder.
//!
//! Builds the OPC-UA node hierarchy from the PLC variable catalog.
//! Variables without dots go under a "Globals" folder; variables with
//! dots (e.g., `Main.counter`) get intermediate folders.

use crate::CatalogEntry;
use crate::type_map::iec_type_to_opcua_data_type;
use opcua_types::NodeId;
use std::collections::HashSet;

/// Namespace index for PLC variables (0 = OPC-UA standard, 1 = server, 2 = ours).
pub const PLC_NAMESPACE: u16 = 2;

/// Information about a variable node to be created in the address space.
#[derive(Debug, Clone)]
pub struct VariableNodeInfo {
    /// The OPC-UA NodeId for this variable.
    pub node_id: NodeId,
    /// The browse name (leaf segment of the dotted path).
    pub browse_name: String,
    /// The display name (same as browse_name).
    pub display_name: String,
    /// The full PLC variable name (used to look up values).
    pub plc_name: String,
    /// The IEC 61131-3 type string.
    pub iec_type: String,
    /// The OPC-UA data type node ID.
    pub data_type: opcua_types::DataTypeId,
    /// The parent folder NodeId.
    pub parent_folder: NodeId,
}

/// Information about a folder node to be created in the address space.
#[derive(Debug, Clone)]
pub struct FolderNodeInfo {
    /// The OPC-UA NodeId for this folder.
    pub node_id: NodeId,
    /// The browse/display name.
    pub name: String,
    /// The parent folder NodeId.
    pub parent: NodeId,
}

/// The complete address space layout derived from a variable catalog.
#[derive(Debug, Clone)]
pub struct AddressSpaceLayout {
    /// The root "PLCRuntime" folder under Objects.
    pub root_folder: NodeId,
    /// All intermediate folders to create.
    pub folders: Vec<FolderNodeInfo>,
    /// All variable nodes to create.
    pub variables: Vec<VariableNodeInfo>,
}

/// Well-known folder NodeIds.
pub fn plc_runtime_folder() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "PLCRuntime")
}

pub fn globals_folder() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "Globals")
}

pub fn programs_folder() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "Programs")
}

/// Status variable NodeIds.
pub fn status_node_id() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "_status")
}

pub fn cycle_count_node_id() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "_cycle_count")
}

pub fn cycle_time_node_id() -> NodeId {
    NodeId::new(PLC_NAMESPACE, "_cycle_time_us")
}

/// Build the address space layout from a variable catalog.
///
/// Variables without dots go under `Globals/`.
/// Variables with dots (e.g., `Main.counter`, `Main.fb.field`) get
/// intermediate folders built from the dot segments.
pub fn build_layout(catalog: &[CatalogEntry]) -> AddressSpaceLayout {
    let root = plc_runtime_folder();
    let globals = globals_folder();
    let programs = programs_folder();

    let mut folders = Vec::new();
    let mut variables = Vec::new();
    let mut created_folders: HashSet<String> = HashSet::new();

    // Always create the root structure folders
    folders.push(FolderNodeInfo {
        node_id: globals.clone(),
        name: "Globals".to_string(),
        parent: root.clone(),
    });
    folders.push(FolderNodeInfo {
        node_id: programs.clone(),
        name: "Programs".to_string(),
        parent: root.clone(),
    });
    created_folders.insert("Globals".to_string());
    created_folders.insert("Programs".to_string());

    for entry in catalog {
        if entry.name.contains('.') {
            // Hierarchical variable: Main.counter, Main.fb.field
            let segments: Vec<&str> = entry.name.split('.').collect();
            let leaf = *segments.last().unwrap();

            // Create intermediate folders
            let mut current_parent = programs.clone();
            for i in 0..segments.len() - 1 {
                let folder_path: String = segments[..=i].join(".");
                if !created_folders.contains(&folder_path) {
                    let folder_node_id = NodeId::new(PLC_NAMESPACE, folder_path.clone());
                    folders.push(FolderNodeInfo {
                        node_id: folder_node_id.clone(),
                        name: segments[i].to_string(),
                        parent: current_parent.clone(),
                    });
                    created_folders.insert(folder_path.clone());
                    current_parent = folder_node_id;
                } else {
                    current_parent = NodeId::new(PLC_NAMESPACE, folder_path);
                }
            }

            variables.push(VariableNodeInfo {
                node_id: NodeId::new(PLC_NAMESPACE, entry.name.clone()),
                browse_name: leaf.to_string(),
                display_name: leaf.to_string(),
                plc_name: entry.name.clone(),
                iec_type: entry.iec_type.clone(),
                data_type: iec_type_to_opcua_data_type(&entry.iec_type),
                parent_folder: current_parent,
            });
        } else {
            // Flat global variable: io_rack_DI_0, pump_vfd_SPEED_REF
            variables.push(VariableNodeInfo {
                node_id: NodeId::new(PLC_NAMESPACE, entry.name.clone()),
                browse_name: entry.name.clone(),
                display_name: entry.name.clone(),
                plc_name: entry.name.clone(),
                iec_type: entry.iec_type.clone(),
                data_type: iec_type_to_opcua_data_type(&entry.iec_type),
                parent_folder: globals.clone(),
            });
        }
    }

    AddressSpaceLayout {
        root_folder: root,
        folders,
        variables,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, ty: &str) -> CatalogEntry {
        CatalogEntry {
            name: name.to_string(),
            iec_type: ty.to_string(),
        }
    }

    #[test]
    fn empty_catalog() {
        let layout = build_layout(&[]);
        // Should still have Globals and Programs folders
        assert_eq!(layout.folders.len(), 2);
        assert!(layout.variables.is_empty());
    }

    #[test]
    fn flat_globals_go_under_globals_folder() {
        let catalog = vec![
            entry("io_rack_DI_0", "BOOL"),
            entry("pump_vfd_SPEED_REF", "REAL"),
        ];
        let layout = build_layout(&catalog);

        assert_eq!(layout.variables.len(), 2);
        for var in &layout.variables {
            assert_eq!(var.parent_folder, globals_folder());
        }
    }

    #[test]
    fn dotted_names_create_folders() {
        let catalog = vec![
            entry("Main.counter", "INT"),
            entry("Main.running", "BOOL"),
        ];
        let layout = build_layout(&catalog);

        // Globals + Programs + Main = 3 folders
        assert_eq!(layout.folders.len(), 3);

        // Main folder should be under Programs
        let main_folder = layout.folders.iter().find(|f| f.name == "Main").unwrap();
        assert_eq!(main_folder.parent, programs_folder());

        // Variables should be under Main
        for var in &layout.variables {
            assert_eq!(var.parent_folder, NodeId::new(PLC_NAMESPACE, "Main"));
        }
    }

    #[test]
    fn nested_dotted_names() {
        let catalog = vec![
            entry("Main.fb_instance.field1", "INT"),
            entry("Main.fb_instance.field2", "REAL"),
        ];
        let layout = build_layout(&catalog);

        // Globals + Programs + Main + Main.fb_instance = 4 folders
        assert_eq!(layout.folders.len(), 4);

        // fb_instance folder should be under Main
        let fb_folder = layout
            .folders
            .iter()
            .find(|f| f.name == "fb_instance")
            .unwrap();
        assert_eq!(fb_folder.parent, NodeId::new(PLC_NAMESPACE, "Main"));

        // Variables should be under Main.fb_instance
        for var in &layout.variables {
            assert_eq!(
                var.parent_folder,
                NodeId::new(PLC_NAMESPACE, "Main.fb_instance")
            );
        }
    }

    #[test]
    fn mixed_flat_and_dotted() {
        let catalog = vec![
            entry("io_rack_DI_0", "BOOL"),
            entry("Main.counter", "DINT"),
            entry("Main.fb.out", "REAL"),
        ];
        let layout = build_layout(&catalog);

        // 3 variables
        assert_eq!(layout.variables.len(), 3);

        // io_rack_DI_0 under Globals
        let flat = layout
            .variables
            .iter()
            .find(|v| v.plc_name == "io_rack_DI_0")
            .unwrap();
        assert_eq!(flat.parent_folder, globals_folder());

        // Main.counter under Main
        let counter = layout
            .variables
            .iter()
            .find(|v| v.plc_name == "Main.counter")
            .unwrap();
        assert_eq!(counter.parent_folder, NodeId::new(PLC_NAMESPACE, "Main"));

        // Main.fb.out under Main.fb
        let fb_out = layout
            .variables
            .iter()
            .find(|v| v.plc_name == "Main.fb.out")
            .unwrap();
        assert_eq!(
            fb_out.parent_folder,
            NodeId::new(PLC_NAMESPACE, "Main.fb")
        );
    }

    #[test]
    fn no_duplicate_folders() {
        let catalog = vec![
            entry("Main.a", "INT"),
            entry("Main.b", "INT"),
            entry("Main.c", "INT"),
        ];
        let layout = build_layout(&catalog);

        // Only one Main folder should be created (+ Globals + Programs)
        let main_count = layout.folders.iter().filter(|f| f.name == "Main").count();
        assert_eq!(main_count, 1);
    }

    #[test]
    fn variable_node_ids_use_full_name() {
        let catalog = vec![
            entry("io_rack_DI_0", "BOOL"),
            entry("Main.counter", "INT"),
        ];
        let layout = build_layout(&catalog);

        let flat = layout
            .variables
            .iter()
            .find(|v| v.plc_name == "io_rack_DI_0")
            .unwrap();
        assert_eq!(flat.node_id, NodeId::new(PLC_NAMESPACE, "io_rack_DI_0"));

        let dotted = layout
            .variables
            .iter()
            .find(|v| v.plc_name == "Main.counter")
            .unwrap();
        assert_eq!(
            dotted.node_id,
            NodeId::new(PLC_NAMESPACE, "Main.counter")
        );
    }

    #[test]
    fn browse_name_is_leaf_segment() {
        let catalog = vec![entry("Main.fb_instance.field1", "INT")];
        let layout = build_layout(&catalog);

        assert_eq!(layout.variables[0].browse_name, "field1");
        assert_eq!(layout.variables[0].display_name, "field1");
    }

    #[test]
    fn data_type_mapping() {
        let catalog = vec![
            entry("bool_var", "BOOL"),
            entry("int_var", "INT"),
            entry("real_var", "REAL"),
            entry("str_var", "STRING"),
        ];
        let layout = build_layout(&catalog);

        let find_var = |name: &str| {
            layout
                .variables
                .iter()
                .find(|v| v.plc_name == name)
                .unwrap()
        };

        assert_eq!(find_var("bool_var").data_type, opcua_types::DataTypeId::Boolean);
        assert_eq!(find_var("int_var").data_type, opcua_types::DataTypeId::Int16);
        assert_eq!(find_var("real_var").data_type, opcua_types::DataTypeId::Float);
        assert_eq!(find_var("str_var").data_type, opcua_types::DataTypeId::String);
    }
}
