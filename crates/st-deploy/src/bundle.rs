//! Program bundle creation, extraction, and verification.
//!
//! A `.st-bundle` is a tar.gz archive containing compiled bytecode, project
//! configuration, device profiles, and optionally source files and debug info.
//! Three modes control what is included:
//!
//! - **Development**: full source + debug info (for internal development)
//! - **Release**: bytecode only, no source, no debug info (IP protection)
//! - **ReleaseDebug**: bytecode + obfuscated debug info (field diagnostics)

use crate::debug_info;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};

/// Bundle mode controls what is included for IP protection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleMode {
    /// Full source + debug info. For internal development and debugging.
    #[default]
    Development,
    /// No source, no debug info. Bytecode only. For production/customer delivery.
    Release,
    /// No source, but includes obfuscated debug info (line maps, no variable names).
    /// For field diagnostics — stack traces with line numbers but no source code.
    #[serde(rename = "release-debug")]
    ReleaseDebug,
}

impl std::fmt::Display for BundleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleMode::Development => write!(f, "development"),
            BundleMode::Release => write!(f, "release"),
            BundleMode::ReleaseDebug => write!(f, "release-debug"),
        }
    }
}

/// Metadata about a program bundle, stored as `manifest.yaml` in the archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Project name.
    pub name: String,
    /// Project version (from plc-project.yaml or "0.0.0").
    pub version: String,
    /// Bundle mode (development / release / release-debug).
    pub mode: BundleMode,
    /// ISO 8601 timestamp of when the bundle was created.
    pub compiled_at: String,
    /// Version of the compiler that produced this bundle.
    pub compiler_version: String,
    /// SHA-256 checksum of the compiled bytecode file.
    pub bytecode_checksum: String,
    /// Whether a debug.map file is included in the bundle.
    #[serde(default)]
    pub has_debug_map: bool,
    /// Entry point PROGRAM name (if configured).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
    /// List of source files included (empty for release mode).
    pub source_files: Vec<String>,
    /// List of device profile files included.
    pub profile_files: Vec<String>,
}

/// Options for creating a bundle.
#[derive(Debug, Clone)]
pub struct BundleOptions {
    /// Bundle mode (controls what is included).
    pub mode: BundleMode,
    /// Output path for the .st-bundle file. If None, uses `{name}.st-bundle` in the project root.
    pub output: Option<PathBuf>,
}

impl Default for BundleOptions {
    fn default() -> Self {
        BundleOptions {
            mode: BundleMode::Development,
            output: None,
        }
    }
}

/// A program bundle ready to be written to disk or inspected.
#[derive(Debug)]
pub struct ProgramBundle {
    /// Bundle manifest.
    pub manifest: BundleManifest,
    /// Compiled bytecode (JSON-serialized st_ir::Module, possibly stripped).
    pub bytecode: Vec<u8>,
    /// Debug map (JSON-serialized DebugMap). None for release mode.
    pub debug_map: Option<Vec<u8>>,
    /// Source files: (relative path, contents). Empty in release mode.
    pub sources: Vec<(String, Vec<u8>)>,
    /// Project YAML contents.
    pub project_yaml: Option<Vec<u8>>,
    /// Device profile files: (filename, contents).
    pub profiles: Vec<(String, Vec<u8>)>,
}

/// Information about a bundle for display purposes.
#[derive(Debug)]
pub struct BundleInfo {
    pub manifest: BundleManifest,
    /// List of files in the archive with their sizes.
    pub files: Vec<(String, u64)>,
    /// Total archive size in bytes.
    pub archive_size: u64,
    /// Whether the bytecode checksum is valid.
    pub checksum_valid: bool,
}

/// Compile a project and create a program bundle.
///
/// This runs the full compilation pipeline (parse → analyze → compile) and
/// packages the result into a `ProgramBundle`.
pub fn create_bundle(
    project_root: &Path,
    options: &BundleOptions,
) -> Result<ProgramBundle, String> {
    // Discover the project
    let project = st_syntax::project::discover_project(Some(project_root))?;

    // Read the project YAML if it exists
    let project_yaml = read_optional_file(&project_root.join("plc-project.yaml"))
        .or_else(|| read_optional_file(&project_root.join("plc-project.yml")));

    // Parse project version from YAML
    let version = project_yaml
        .as_ref()
        .and_then(|yaml| {
            let text = std::str::from_utf8(yaml).ok()?;
            let val: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
            val.get("version")?.as_str().map(String::from)
        })
        .unwrap_or_else(|| "0.0.0".to_string());

    // Parse with stdlib
    let sources = st_syntax::project::load_project_sources(&project)?;
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    let source_strs: Vec<&str> = sources.iter().map(|(_, s)| s.as_str()).collect();
    all.extend(&source_strs);
    let parse_result = st_syntax::multi_file::parse_multi(&all);

    if !parse_result.errors.is_empty() {
        let msgs: Vec<String> = parse_result.errors.iter().map(|e| e.message.clone()).collect();
        return Err(format!("Parse errors:\n  {}", msgs.join("\n  ")));
    }

    // Build native FB registry from device profiles (if any exist in the project).
    let native_registry = build_native_fb_registry_for_bundle(project_root);

    // Semantic analysis
    let analysis = st_semantics::analyze::analyze_with_native_fbs(
        &parse_result.source_file,
        native_registry.as_ref(),
    );
    let errors: Vec<_> = analysis
        .diagnostics
        .iter()
        .filter(|d| d.severity == st_semantics::diagnostic::Severity::Error)
        .collect();
    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
        return Err(format!("Semantic errors:\n  {}", msgs.join("\n  ")));
    }

    // Compile
    let mut module = st_compiler::compile_with_native_fbs(
        &parse_result.source_file,
        native_registry.as_ref(),
    )
    .map_err(|e| format!("Compilation error: {e}"))?;

    // Extract debug info before any stripping
    let full_debug_map = debug_info::extract_debug_map(&module);

    // Apply stripping and prepare debug map based on bundle mode
    let debug_map_bytes = match options.mode {
        BundleMode::Development => {
            // Full debug map, module untouched
            let bytes = serde_json::to_vec_pretty(&full_debug_map)
                .map_err(|e| format!("Debug map serialization error: {e}"))?;
            Some(bytes)
        }
        BundleMode::ReleaseDebug => {
            // Obfuscated debug map (line maps kept, var names replaced)
            let obfuscated = debug_info::obfuscate_debug_map(&full_debug_map);
            let bytes = serde_json::to_vec_pretty(&obfuscated)
                .map_err(|e| format!("Debug map serialization error: {e}"))?;
            // Strip the module but keep source maps for line-based breakpoints
            debug_info::strip_module_keep_source_maps(&mut module);
            Some(bytes)
        }
        BundleMode::Release => {
            // No debug map, strip everything from the module
            debug_info::strip_module(&mut module);
            None
        }
    };

    // Serialize bytecode (after stripping)
    let bytecode = serde_json::to_vec_pretty(&module)
        .map_err(|e| format!("Serialization error: {e}"))?;

    // Compute bytecode checksum
    let bytecode_checksum = sha256_hex(&bytecode);

    // Collect source files (only in development mode)
    let source_entries = if options.mode == BundleMode::Development {
        sources
            .iter()
            .filter_map(|(path, content)| {
                let rel = path.strip_prefix(&project.root).ok()?;
                Some((rel.to_string_lossy().to_string(), content.as_bytes().to_vec()))
            })
            .collect()
    } else {
        Vec::new()
    };

    let source_file_names: Vec<String> = source_entries
        .iter()
        .map(|(name, _)| name.clone())
        .collect();

    // Collect device profiles
    let profiles = collect_profiles(project_root)?;
    let profile_names: Vec<String> = profiles.iter().map(|(name, _)| name.clone()).collect();

    // Read I/O map

    // Build manifest
    let manifest = BundleManifest {
        name: project.name.clone(),
        version,
        mode: options.mode,
        compiled_at: chrono::Utc::now().to_rfc3339(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        bytecode_checksum,
        has_debug_map: debug_map_bytes.is_some(),
        entry_point: project.entry_point.clone(),
        source_files: source_file_names,
        profile_files: profile_names,
    };

    Ok(ProgramBundle {
        manifest,
        bytecode,
        debug_map: debug_map_bytes,
        sources: source_entries,
        project_yaml,
        profiles,
    })
}

/// Write a program bundle to a `.st-bundle` file (tar.gz).
pub fn write_bundle(bundle: &ProgramBundle, output: &Path) -> Result<u64, String> {
    let file = std::fs::File::create(output)
        .map_err(|e| format!("Cannot create {}: {e}", output.display()))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    // manifest.yaml
    let manifest_bytes = serde_yaml::to_string(&bundle.manifest)
        .map_err(|e| format!("Manifest serialization error: {e}"))?;
    append_bytes(&mut tar, "manifest.yaml", manifest_bytes.as_bytes())?;

    // program.stc (compiled bytecode)
    append_bytes(&mut tar, "program.stc", &bundle.bytecode)?;

    // debug.map (development and release-debug only)
    if let Some(ref debug_map) = bundle.debug_map {
        append_bytes(&mut tar, "debug.map", debug_map)?;
    }

    // plc-project.yaml
    if let Some(ref yaml) = bundle.project_yaml {
        append_bytes(&mut tar, "plc-project.yaml", yaml)?;
    }


    // source/ directory (development mode only)
    for (rel_path, content) in &bundle.sources {
        let archive_path = format!("source/{rel_path}");
        append_bytes(&mut tar, &archive_path, content)?;
    }

    // profiles/ directory
    for (filename, content) in &bundle.profiles {
        let archive_path = format!("profiles/{filename}");
        append_bytes(&mut tar, &archive_path, content)?;
    }

    let enc = tar.into_inner().map_err(|e| format!("Tar finalize error: {e}"))?;
    enc.finish().map_err(|e| format!("Gzip finalize error: {e}"))?;

    let size = std::fs::metadata(output)
        .map_err(|e| format!("Cannot stat {}: {e}", output.display()))?
        .len();
    Ok(size)
}

/// Read and verify a `.st-bundle` file. Returns metadata + file listing.
pub fn inspect_bundle(path: &Path) -> Result<BundleInfo, String> {
    let archive_size = std::fs::metadata(path)
        .map_err(|e| format!("Cannot stat {}: {e}", path.display()))?
        .len();

    let file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);

    let mut manifest: Option<BundleManifest> = None;
    let mut bytecode: Option<Vec<u8>> = None;
    let mut files = Vec::new();

    for entry in archive.entries().map_err(|e| format!("Tar read error: {e}"))? {
        let mut entry = entry.map_err(|e| format!("Tar entry error: {e}"))?;
        let path_str = entry
            .path()
            .map_err(|e| format!("Tar path error: {e}"))?
            .to_string_lossy()
            .to_string();
        let size = entry.size();

        if path_str == "manifest.yaml" {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| format!("Cannot read manifest: {e}"))?;
            manifest = Some(
                serde_yaml::from_str(&content)
                    .map_err(|e| format!("Invalid manifest: {e}"))?,
            );
        } else if path_str == "program.stc" {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("Cannot read bytecode: {e}"))?;
            bytecode = Some(content);
        }

        files.push((path_str, size));
    }

    let manifest = manifest.ok_or("Bundle is missing manifest.yaml")?;

    // Verify bytecode checksum
    let checksum_valid = if let Some(ref bc) = bytecode {
        sha256_hex(bc) == manifest.bytecode_checksum
    } else {
        false
    };

    // Sort files for deterministic display
    files.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(BundleInfo {
        manifest,
        files,
        archive_size,
        checksum_valid,
    })
}

/// Extract a bundle from a `.st-bundle` file into a `ProgramBundle`.
pub fn extract_bundle(path: &Path) -> Result<ProgramBundle, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);

    let mut manifest: Option<BundleManifest> = None;
    let mut bytecode: Option<Vec<u8>> = None;
    let mut debug_map: Option<Vec<u8>> = None;
    let mut sources: Vec<(String, Vec<u8>)> = Vec::new();
    let mut project_yaml: Option<Vec<u8>> = None;
    let mut profiles: Vec<(String, Vec<u8>)> = Vec::new();

    for entry in archive.entries().map_err(|e| format!("Tar read error: {e}"))? {
        let mut entry = entry.map_err(|e| format!("Tar entry error: {e}"))?;
        let path_str = entry
            .path()
            .map_err(|e| format!("Tar path error: {e}"))?
            .to_string_lossy()
            .to_string();

        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|e| format!("Cannot read {path_str}: {e}"))?;

        match path_str.as_str() {
            "manifest.yaml" => {
                let text = String::from_utf8(content.clone())
                    .map_err(|e| format!("manifest.yaml is not UTF-8: {e}"))?;
                manifest = Some(
                    serde_yaml::from_str(&text)
                        .map_err(|e| format!("Invalid manifest: {e}"))?,
                );
            }
            "program.stc" => {
                bytecode = Some(content);
            }
            "debug.map" => {
                debug_map = Some(content);
            }
            "plc-project.yaml" => {
                project_yaml = Some(content);
            }
            "_io_map.st" => {
                // Legacy — ignored
            }
            p if p.starts_with("source/") => {
                let rel = p.strip_prefix("source/").unwrap().to_string();
                sources.push((rel, content));
            }
            p if p.starts_with("profiles/") => {
                let filename = p.strip_prefix("profiles/").unwrap().to_string();
                profiles.push((filename, content));
            }
            _ => {
                // Unknown entry — skip
            }
        }
    }

    let manifest = manifest.ok_or("Bundle is missing manifest.yaml")?;
    let bytecode = bytecode.ok_or("Bundle is missing program.stc")?;

    // Verify checksum
    let actual_checksum = sha256_hex(&bytecode);
    if actual_checksum != manifest.bytecode_checksum {
        return Err(format!(
            "Bytecode checksum mismatch: expected {}, got {}",
            manifest.bytecode_checksum, actual_checksum
        ));
    }

    Ok(ProgramBundle {
        manifest,
        bytecode,
        debug_map,
        sources,
        project_yaml,
        profiles,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn read_optional_file(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn collect_profiles(project_root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    // Search for profiles in the project's profiles/ directory and parent
    // directories (workspace root pattern), matching the registry builder.
    let mut search_dirs = vec![project_root.join("profiles")];
    let mut cur = project_root.to_path_buf();
    for _ in 0..6 {
        if let Some(parent) = cur.parent() {
            let candidate = parent.join("profiles");
            if candidate.is_dir() && candidate != project_root.join("profiles") {
                search_dirs.push(candidate);
            }
            cur = parent.to_path_buf();
        } else {
            break;
        }
    }

    let mut result = BTreeMap::new();
    for profiles_dir in &search_dirs {
        if !profiles_dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(profiles_dir)
            .map_err(|e| format!("Cannot read {}: {e}", profiles_dir.display()))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Profile dir entry error: {e}"))?;
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "yaml" || ext == "yml" {
                    let filename = path.file_name().unwrap().to_string_lossy().to_string();
                    // Don't overwrite — first found wins (local profiles take priority)
                    result.entry(filename).or_insert_with(|| {
                        std::fs::read(&path).unwrap_or_default()
                    });
                }
            }
        }
    }

    Ok(result.into_iter().collect())
}

fn append_bytes<W: Write>(
    tar: &mut Builder<W>,
    path: &str,
    data: &[u8],
) -> Result<(), String> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, path, data)
        .map_err(|e| format!("Cannot write {path} to archive: {e}"))
}

/// Build a [`NativeFbRegistry`] from device profiles found in the project's
/// `profiles/` directory. Used by `create_bundle()` so native FB projects
/// compile correctly during bundling.
///
/// Returns `None` if no profiles directory exists or no profiles are found.
fn build_native_fb_registry_for_bundle(
    project_root: &Path,
) -> Option<st_comm_api::NativeFbRegistry> {
    let profiles_dir = project_root.join("profiles");
    if !profiles_dir.is_dir() {
        // Also try parent directories (workspace root pattern)
        let mut cur = project_root.to_path_buf();
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                let candidate = parent.join("profiles");
                if candidate.is_dir() {
                    return build_registry_from_dir(&candidate);
                }
                cur = parent.to_path_buf();
            } else {
                break;
            }
        }
        return None;
    }
    build_registry_from_dir(&profiles_dir)
}

fn build_registry_from_dir(dir: &Path) -> Option<st_comm_api::NativeFbRegistry> {
    let mut registry = st_comm_api::NativeFbRegistry::new();
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        if let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) {
            // Create a stub NativeFb that just provides the layout for compilation.
            // At runtime, the actual NativeFb implementation will be provided by the engine.
            registry.register(Box::new(StubNativeFb {
                layout: profile.to_native_fb_layout(),
            }));
        }
    }
    if registry.is_empty() {
        None
    } else {
        Some(registry)
    }
}

/// A stub NativeFb used only during bundle compilation. Provides the layout
/// for type checking and code generation but execute() is a no-op.
struct StubNativeFb {
    layout: st_comm_api::NativeFbLayout,
}

impl st_comm_api::NativeFb for StubNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }
    fn layout(&self) -> &st_comm_api::NativeFbLayout {
        &self.layout
    }
    fn execute(&self, _fields: &mut [st_ir::Value]) {
        // Stub — no-op during bundling.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a minimal ST project in a temp directory.
    fn create_test_project(extra_yaml: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        let yaml = format!(
            "name: TestBundle\nversion: '1.2.3'\nentryPoint: Main\n{extra_yaml}"
        );
        fs::write(root.join("plc-project.yaml"), &yaml).unwrap();

        fs::write(
            root.join("main.st"),
            "PROGRAM Main\nVAR\n    counter : INT := 0;\nEND_VAR\n    counter := counter + 1;\nEND_PROGRAM\n",
        )
        .unwrap();

        (dir, root)
    }

    #[test]
    fn create_and_extract_development_bundle() {
        let (_dir, root) = create_test_project("");
        let options = BundleOptions::default();

        let bundle = create_bundle(&root, &options).unwrap();
        assert_eq!(bundle.manifest.name, "TestBundle");
        assert_eq!(bundle.manifest.version, "1.2.3");
        assert_eq!(bundle.manifest.mode, BundleMode::Development);
        assert!(!bundle.bytecode.is_empty());
        assert_eq!(bundle.sources.len(), 1);
        assert!(bundle.sources[0].0.contains("main.st"));
        assert!(bundle.project_yaml.is_some());

        // Write to file
        let bundle_path = root.join("test.st-bundle");
        let size = write_bundle(&bundle, &bundle_path).unwrap();
        assert!(size > 0);
        assert!(bundle_path.exists());

        // Extract
        let extracted = extract_bundle(&bundle_path).unwrap();
        assert_eq!(extracted.manifest.name, "TestBundle");
        assert_eq!(extracted.manifest.version, "1.2.3");
        assert_eq!(extracted.manifest.mode, BundleMode::Development);
        assert_eq!(extracted.bytecode, bundle.bytecode);
        assert_eq!(extracted.sources.len(), 1);
        assert!(extracted.project_yaml.is_some());
    }

    #[test]
    fn release_bundle_excludes_source() {
        let (_dir, root) = create_test_project("");
        let options = BundleOptions {
            mode: BundleMode::Release,
            ..Default::default()
        };

        let bundle = create_bundle(&root, &options).unwrap();
        assert_eq!(bundle.manifest.mode, BundleMode::Release);
        assert!(bundle.sources.is_empty(), "Release bundle must not contain source files");
        assert!(bundle.manifest.source_files.is_empty());

        // Write and inspect
        let bundle_path = root.join("release.st-bundle");
        write_bundle(&bundle, &bundle_path).unwrap();

        let info = inspect_bundle(&bundle_path).unwrap();
        assert!(info.checksum_valid);
        let has_source = info.files.iter().any(|(p, _)| p.starts_with("source/"));
        assert!(!has_source, "Release bundle file listing must not contain source/");
    }

    #[test]
    fn bundle_checksum_verification() {
        let (_dir, root) = create_test_project("");
        let options = BundleOptions::default();

        let bundle = create_bundle(&root, &options).unwrap();
        let bundle_path = root.join("test.st-bundle");
        write_bundle(&bundle, &bundle_path).unwrap();

        let info = inspect_bundle(&bundle_path).unwrap();
        assert!(info.checksum_valid);
        assert!(!info.manifest.bytecode_checksum.is_empty());
    }

    #[test]
    fn bundle_contains_profiles() {
        let (_dir, root) = create_test_project("");

        // Create a profiles directory with a test profile
        let profiles_dir = root.join("profiles");
        fs::create_dir_all(&profiles_dir).unwrap();
        fs::write(
            profiles_dir.join("test_device.yaml"),
            "name: TestDevice\nvendor: Test\nfields: []\n",
        )
        .unwrap();

        let options = BundleOptions::default();
        let bundle = create_bundle(&root, &options).unwrap();
        assert_eq!(bundle.profiles.len(), 1);
        assert_eq!(bundle.profiles[0].0, "test_device.yaml");
        assert_eq!(bundle.manifest.profile_files, vec!["test_device.yaml"]);

        // Round-trip through file
        let bundle_path = root.join("test.st-bundle");
        write_bundle(&bundle, &bundle_path).unwrap();
        let extracted = extract_bundle(&bundle_path).unwrap();
        assert_eq!(extracted.profiles.len(), 1);
        assert_eq!(extracted.profiles[0].0, "test_device.yaml");
    }

    #[test]
    fn inspect_shows_all_files() {
        let (_dir, root) = create_test_project("");
        let options = BundleOptions::default();

        let bundle = create_bundle(&root, &options).unwrap();
        let bundle_path = root.join("test.st-bundle");
        write_bundle(&bundle, &bundle_path).unwrap();

        let info = inspect_bundle(&bundle_path).unwrap();
        let file_names: Vec<&str> = info.files.iter().map(|(p, _)| p.as_str()).collect();
        assert!(file_names.contains(&"manifest.yaml"));
        assert!(file_names.contains(&"program.stc"));
        assert!(file_names.contains(&"plc-project.yaml"));
        assert!(info.archive_size > 0);
    }

    #[test]
    fn multi_file_project_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();

        fs::write(
            root.join("plc-project.yaml"),
            "name: MultiFile\nversion: '2.0.0'\nentryPoint: Main\n",
        )
        .unwrap();
        fs::write(
            root.join("main.st"),
            "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := Add_One(IN1 := x);\nEND_PROGRAM\n",
        )
        .unwrap();
        fs::write(
            root.join("helper.st"),
            "FUNCTION Add_One : INT\nVAR_INPUT\n    IN1 : INT;\nEND_VAR\n    Add_One := IN1 + 1;\nEND_FUNCTION\n",
        )
        .unwrap();

        let options = BundleOptions::default();
        let bundle = create_bundle(&root, &options).unwrap();
        assert_eq!(bundle.manifest.name, "MultiFile");
        assert_eq!(bundle.sources.len(), 2);

        // Release mode excludes both files
        let release_bundle = create_bundle(
            &root,
            &BundleOptions {
                mode: BundleMode::Release,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(release_bundle.sources.is_empty());
    }

    #[test]
    fn manifest_roundtrip() {
        let manifest = BundleManifest {
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            mode: BundleMode::ReleaseDebug,
            compiled_at: "2026-04-10T14:30:00Z".to_string(),
            compiler_version: "0.1.0".to_string(),
            bytecode_checksum: "abc123".to_string(),
            has_debug_map: true,
            entry_point: Some("Main".to_string()),
            source_files: vec![],
            profile_files: vec!["dev.yaml".to_string()],
        };

        let yaml = serde_yaml::to_string(&manifest).unwrap();
        let parsed: BundleManifest = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.mode, BundleMode::ReleaseDebug);
        assert_eq!(parsed.entry_point, Some("Main".to_string()));
        assert_eq!(parsed.profile_files, vec!["dev.yaml"]);
    }

    #[test]
    fn sha256_hex_deterministic() {
        let data = b"hello world";
        let hash1 = sha256_hex(data);
        let hash2 = sha256_hex(data);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn empty_project_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("plc-project.yaml"), "name: Empty\n").unwrap();
        // No .st files — should error during compilation (no PROGRAM found)
        let result = create_bundle(&root, &BundleOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn bundle_mode_display() {
        assert_eq!(BundleMode::Development.to_string(), "development");
        assert_eq!(BundleMode::Release.to_string(), "release");
        assert_eq!(BundleMode::ReleaseDebug.to_string(), "release-debug");
    }
}
