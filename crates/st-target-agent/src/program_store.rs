//! Program bundle storage and management on disk.

use crate::error::ApiError;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Persisted metadata (written to disk alongside bytecode for reboot survival).
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedMeta {
    name: String,
    version: String,
    mode: String,
    compiled_at: String,
    entry_point: Option<String>,
    bytecode_checksum: String,
    deployed_at: String,
    has_debug_map: bool,
}

/// Manages program bundles on the target's filesystem.
pub struct ProgramStore {
    program_dir: PathBuf,
    current: Option<StoredProgram>,
}

/// A program stored on disk with its metadata and raw bytecode.
struct StoredProgram {
    metadata: ProgramMetadata,
    bytecode: Vec<u8>,
    entry_point: String,
    /// Path to extracted source files on disk (for DAP debugging).
    source_dir: Option<PathBuf>,
}

/// Metadata about the currently deployed program (serializable for API responses).
#[derive(Debug, Clone, Serialize)]
pub struct ProgramMetadata {
    pub name: String,
    pub version: String,
    pub mode: String,
    pub compiled_at: String,
    pub entry_point: Option<String>,
    pub bytecode_checksum: String,
    pub deployed_at: String,
    pub has_debug_map: bool,
}

impl ProgramStore {
    /// Create a new program store using the given directory.
    /// Loads the previously deployed program from disk if present.
    pub fn new(program_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(program_dir)
            .map_err(|e| format!("Cannot create program dir {}: {e}", program_dir.display()))?;
        let mut store = ProgramStore {
            program_dir: program_dir.to_path_buf(),
            current: None,
        };
        // Try to load a previously deployed program from disk
        store.load_persisted();
        Ok(store)
    }

    /// Load a previously deployed program from disk (bytecode + metadata).
    /// Called on startup to restore state after a reboot.
    fn load_persisted(&mut self) {
        let bytecode_path = self.program_dir.join("current.bytecode");
        let meta_path = self.program_dir.join("current.meta.json");
        let source_dir = self.program_dir.join("current_source");

        let Ok(bytecode) = fs::read(&bytecode_path) else { return };
        let Ok(meta_json) = fs::read_to_string(&meta_path) else { return };
        let Ok(persisted) = serde_json::from_str::<PersistedMeta>(&meta_json) else { return };

        self.current = Some(StoredProgram {
            metadata: ProgramMetadata {
                name: persisted.name.clone(),
                version: persisted.version.clone(),
                mode: persisted.mode.clone(),
                compiled_at: persisted.compiled_at.clone(),
                entry_point: persisted.entry_point.clone(),
                bytecode_checksum: persisted.bytecode_checksum.clone(),
                deployed_at: persisted.deployed_at.clone(),
                has_debug_map: persisted.has_debug_map,
            },
            bytecode,
            entry_point: persisted.entry_point.clone().unwrap_or_else(|| "Main".to_string()),
            source_dir: if source_dir.is_dir() { Some(source_dir) } else { None },
        });
    }

    /// Store a program bundle from raw bytes. Validates the bundle, extracts
    /// metadata, and holds the bytecode in memory for runtime loading.
    pub fn store_bundle(&mut self, data: &[u8]) -> Result<ProgramMetadata, ApiError> {
        // Write to temp file for extraction
        let temp_path = self.program_dir.join("_upload.st-bundle");
        fs::write(&temp_path, data)
            .map_err(|e| ApiError::internal(format!("Cannot write temp bundle: {e}")))?;

        // Extract and verify (checksums, manifest)
        let bundle = st_deploy::bundle::extract_bundle(&temp_path)
            .map_err(ApiError::invalid_bundle)?;

        // Clean up temp file
        let _ = fs::remove_file(&temp_path);

        let manifest = &bundle.manifest;
        let entry_point = manifest
            .entry_point
            .clone()
            .unwrap_or_else(|| "Main".to_string());

        let metadata = ProgramMetadata {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            mode: manifest.mode.to_string(),
            compiled_at: manifest.compiled_at.clone(),
            entry_point: manifest.entry_point.clone(),
            bytecode_checksum: manifest.bytecode_checksum.clone(),
            deployed_at: chrono::Utc::now().to_rfc3339(),
            has_debug_map: manifest.has_debug_map,
        };

        // Extract source files to disk for DAP debugging
        let source_dir = if !bundle.sources.is_empty() {
            let dir = self.program_dir.join("current_source");
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir)
                .map_err(|e| ApiError::internal(format!("Cannot create source dir: {e}")))?;

            // Write project YAML if present
            if let Some(ref yaml) = bundle.project_yaml {
                fs::write(dir.join("plc-project.yaml"), yaml)
                    .map_err(|e| ApiError::internal(format!("Cannot write project yaml: {e}")))?;
            }

            // Write source files
            for (rel_path, content) in &bundle.sources {
                let target = dir.join(rel_path);
                if let Some(parent) = target.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::write(&target, content)
                    .map_err(|e| ApiError::internal(format!("Cannot write source {rel_path}: {e}")))?;
            }

            Some(dir)
        } else {
            None
        };

        // Persist bytecode and metadata to disk for reboot survival
        let _ = fs::write(
            self.program_dir.join("current.bytecode"),
            &bundle.bytecode,
        );
        let persisted = PersistedMeta {
            name: metadata.name.clone(),
            version: metadata.version.clone(),
            mode: metadata.mode.clone(),
            compiled_at: metadata.compiled_at.clone(),
            entry_point: metadata.entry_point.clone(),
            bytecode_checksum: metadata.bytecode_checksum.clone(),
            deployed_at: metadata.deployed_at.clone(),
            has_debug_map: metadata.has_debug_map,
        };
        if let Ok(json) = serde_json::to_string_pretty(&persisted) {
            let _ = fs::write(self.program_dir.join("current.meta.json"), json);
        }

        self.current = Some(StoredProgram {
            metadata: metadata.clone(),
            bytecode: bundle.bytecode,
            entry_point,
            source_dir,
        });

        Ok(metadata)
    }

    /// Get metadata of the currently deployed program.
    pub fn current_program(&self) -> Option<&ProgramMetadata> {
        self.current.as_ref().map(|s| &s.metadata)
    }

    /// Get the path to the extracted source directory (for DAP debugging).
    /// Returns None if no program is deployed or no source files are available
    /// (release mode bundles).
    pub fn source_path(&self) -> Option<PathBuf> {
        self.current
            .as_ref()
            .and_then(|s| s.source_dir.clone())
    }

    /// Load the compiled Module from the current program's bytecode.
    pub fn load_module(&self) -> Result<(st_ir::Module, String), ApiError> {
        let stored = self.current.as_ref().ok_or_else(|| {
            ApiError::not_found("No program deployed")
        })?;

        let module: st_ir::Module = serde_json::from_slice(&stored.bytecode)
            .map_err(|e| ApiError::internal(format!("Cannot deserialize bytecode: {e}")))?;

        Ok((module, stored.entry_point.clone()))
    }

    /// Remove the currently deployed program.
    pub fn remove_current(&mut self) -> Result<(), ApiError> {
        if self.current.is_none() {
            return Err(ApiError::not_found("No program deployed"));
        }
        self.current = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use st_deploy::bundle::{create_bundle, write_bundle, BundleOptions};

    fn make_test_bundle() -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("plc-project.yaml"),
            "name: StoreTest\nversion: '1.0.0'\nentryPoint: Main\n",
        )
        .unwrap();
        fs::write(
            root.join("main.st"),
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n",
        )
        .unwrap();

        let bundle = create_bundle(root, &BundleOptions::default()).unwrap();
        let bundle_path = root.join("test.st-bundle");
        write_bundle(&bundle, &bundle_path).unwrap();
        fs::read(&bundle_path).unwrap()
    }

    #[test]
    fn store_and_retrieve_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgramStore::new(dir.path()).unwrap();
        let data = make_test_bundle();

        let meta = store.store_bundle(&data).unwrap();
        assert_eq!(meta.name, "StoreTest");
        assert_eq!(meta.version, "1.0.0");

        let current = store.current_program().unwrap();
        assert_eq!(current.name, "StoreTest");
    }

    #[test]
    fn load_module_from_stored_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgramStore::new(dir.path()).unwrap();
        let data = make_test_bundle();
        store.store_bundle(&data).unwrap();

        let (module, entry) = store.load_module().unwrap();
        assert_eq!(entry, "Main");
        assert!(module.find_function("Main").is_some());
    }

    #[test]
    fn store_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgramStore::new(dir.path()).unwrap();
        let data = make_test_bundle();

        store.store_bundle(&data).unwrap();
        store.store_bundle(&data).unwrap(); // second upload
        assert!(store.current_program().is_some());
    }

    #[test]
    fn remove_clears_current() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgramStore::new(dir.path()).unwrap();
        let data = make_test_bundle();

        store.store_bundle(&data).unwrap();
        store.remove_current().unwrap();
        assert!(store.current_program().is_none());
    }

    #[test]
    fn invalid_bundle_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ProgramStore::new(dir.path()).unwrap();
        let result = store.store_bundle(b"not a valid bundle");
        assert!(result.is_err());
    }

    #[test]
    fn load_module_without_program_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProgramStore::new(dir.path()).unwrap();
        let result = store.load_module();
        assert!(result.is_err());
    }
}
