//! PLC project discovery and configuration.
//!
//! Supports three modes:
//! 1. **Single file**: `st-cli run file.st` — compile one file + stdlib
//! 2. **Autodiscovery**: `st-cli run` or `st-cli run dir/` — walk directory tree for .st/.scl files
//! 3. **Project file**: `plc-project.yaml` in the root — explicit configuration

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A resolved PLC project: all source files ready to compile.
#[derive(Debug, Clone)]
pub struct Project {
    /// Root directory of the project.
    pub root: PathBuf,
    /// All source file paths, in compilation order.
    pub source_files: Vec<PathBuf>,
    /// Entry point PROGRAM name (None = first PROGRAM found).
    pub entry_point: Option<String>,
    /// Project name (from yaml or directory name).
    pub name: String,
}

/// Project configuration from `plc-project.yaml`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Explicit source file list (supports globs). If omitted, autodiscover.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    /// Entry point PROGRAM name.
    #[serde(default, rename = "entryPoint")]
    pub entry_point: Option<String>,
    /// Additional library directories to include.
    #[serde(default)]
    pub libraries: Option<Vec<String>>,
    /// Files/directories to exclude from autodiscovery.
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

/// Discover a project from a path.
///
/// - If `path` is a `.st`/`.scl` file → single-file mode
/// - If `path` is a directory → look for `plc-project.yaml`, else autodiscover
/// - If `path` is None → use current directory
pub fn discover_project(path: Option<&Path>) -> Result<Project, String> {
    let path = path.unwrap_or_else(|| Path::new("."));
    let path = std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve path: {e}"))?;

    if path.is_file() {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "st" || ext == "scl" {
            return Ok(Project {
                root: path.parent().unwrap_or(&path).to_path_buf(),
                source_files: vec![path.clone()],
                entry_point: None,
                name: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            });
        }
        if ext == "yaml" || ext == "yml" {
            return load_project_yaml(&path);
        }
        return Err(format!("Not a .st, .scl, or .yaml file: {}", path.display()));
    }

    if path.is_dir() {
        // Check for plc-project.yaml
        let yaml_path = path.join("plc-project.yaml");
        let yml_path = path.join("plc-project.yml");
        if yaml_path.exists() {
            return load_project_yaml(&yaml_path);
        }
        if yml_path.exists() {
            return load_project_yaml(&yml_path);
        }
        // Autodiscover
        return autodiscover(&path, &[], &[]);
    }

    Err(format!("Path does not exist: {}", path.display()))
}

/// Load and resolve a `plc-project.yaml` file.
fn load_project_yaml(yaml_path: &Path) -> Result<Project, String> {
    let content = std::fs::read_to_string(yaml_path)
        .map_err(|e| format!("Cannot read {}: {e}", yaml_path.display()))?;
    let config: ProjectConfig = serde_yaml::from_str(&content)
        .map_err(|e| format!("Invalid YAML in {}: {e}", yaml_path.display()))?;

    let root = yaml_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let project_name = config.name.clone()
        .unwrap_or_else(|| root.file_name().unwrap_or_default().to_string_lossy().to_string());

    let exclude_patterns: Vec<String> = config.exclude.clone().unwrap_or_default();

    let mut source_files = if let Some(ref sources) = config.sources {
        // Explicit source list (supports globs)
        resolve_source_globs(&root, sources)?
    } else {
        // Autodiscover from root
        discover_st_files(&root, &exclude_patterns)?
    };

    // Add library directories
    if let Some(ref libs) = config.libraries {
        for lib_dir in libs {
            let lib_path = root.join(lib_dir);
            if lib_path.is_dir() {
                let lib_files = discover_st_files(&lib_path, &[])?;
                source_files.extend(lib_files);
            }
        }
    }

    Ok(Project {
        root,
        source_files,
        entry_point: config.entry_point,
        name: project_name,
    })
}

/// Autodiscover all .st/.scl files in a directory tree.
pub fn autodiscover(
    root: &Path,
    exclude: &[String],
    extra_lib_dirs: &[PathBuf],
) -> Result<Project, String> {
    let mut source_files = discover_st_files(root, exclude)?;

    for lib_dir in extra_lib_dirs {
        if lib_dir.is_dir() {
            let lib_files = discover_st_files(lib_dir, &[])?;
            source_files.extend(lib_files);
        }
    }

    let name = root.file_name().unwrap_or_default().to_string_lossy().to_string();

    Ok(Project {
        root: root.to_path_buf(),
        source_files,
        entry_point: None,
        name,
    })
}

/// Recursively find all .st/.scl files in a directory.
fn discover_st_files(dir: &Path, exclude: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    walk_dir(dir, &mut files, exclude)?;
    files.sort(); // deterministic order
    Ok(files)
}

fn walk_dir(dir: &Path, files: &mut Vec<PathBuf>, exclude: &[String]) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read directory {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Directory entry error: {e}"))?;
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        // Check exclude patterns
        if is_excluded(&name, &path, exclude) {
            continue;
        }

        if path.is_dir() {
            // Skip hidden directories and common non-source dirs
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk_dir(&path, files, exclude)?;
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext == "st" || ext == "scl" {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn is_excluded(name: &str, path: &Path, exclude: &[String]) -> bool {
    for pattern in exclude {
        // Simple glob matching: * at start/end
        if pattern.ends_with('/') {
            // Directory pattern
            let dir_name = pattern.trim_end_matches('/');
            if name == dir_name {
                return true;
            }
        } else if let Some(ext) = pattern.strip_prefix("*.") {
            // Extension pattern
            if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                return true;
            }
        } else if pattern.contains('*') {
            // Simple wildcard
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let (prefix, suffix) = (parts[0], parts[1]);
                if name.starts_with(prefix) && name.ends_with(suffix) {
                    return true;
                }
            }
        } else if name == pattern.as_str() {
            return true;
        }
    }
    false
}

/// Resolve a list of source patterns (may include globs) relative to root.
fn resolve_source_globs(root: &Path, patterns: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for pattern in patterns {
        let full_pattern = root.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();

        if pattern_str.contains('*') {
            // Glob pattern
            let matches = glob::glob(&pattern_str)
                .map_err(|e| format!("Invalid glob pattern '{pattern}': {e}"))?;
            for entry in matches {
                let path = entry.map_err(|e| format!("Glob error: {e}"))?;
                if path.is_file() {
                    files.push(path);
                }
            }
        } else {
            // Direct file path
            let path = root.join(pattern);
            if path.exists() {
                files.push(path);
            } else {
                return Err(format!("Source file not found: {}", path.display()));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Load all source files from a project and return their contents.
pub fn load_project_sources(project: &Project) -> Result<Vec<(PathBuf, String)>, String> {
    let mut sources = Vec::new();
    for path in &project.source_files {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        sources.push((path.clone(), content));
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, content).unwrap();
        }
        let root = dir.path().to_path_buf();
        (dir, root)
    }

    #[test]
    fn discover_single_file() {
        let (_dir, root) = create_temp_project(&[("main.st", "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n")]);
        let project = discover_project(Some(&root.join("main.st"))).unwrap();
        assert_eq!(project.source_files.len(), 1);
        assert_eq!(project.name, "main");
    }

    #[test]
    fn autodiscover_flat_directory() {
        let (_dir, root) = create_temp_project(&[
            ("types.st", "(* types *)"),
            ("main.st", "(* main *)"),
            ("utils.st", "(* utils *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 3);
        // Files should be sorted
        let names: Vec<_> = project.source_files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["main.st", "types.st", "utils.st"]);
    }

    #[test]
    fn autodiscover_nested_directories() {
        let (_dir, root) = create_temp_project(&[
            ("main.st", "(* main *)"),
            ("lib/helpers.st", "(* helpers *)"),
            ("lib/math/trig.st", "(* trig *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 3);
    }

    #[test]
    fn autodiscover_skips_hidden_and_target() {
        let (_dir, root) = create_temp_project(&[
            ("main.st", "(* main *)"),
            (".hidden/secret.st", "(* hidden *)"),
            ("target/build.st", "(* build artifact *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 1);
    }

    #[test]
    fn autodiscover_includes_scl_files() {
        let (_dir, root) = create_temp_project(&[
            ("main.st", "(* main *)"),
            ("siemens.scl", "(* scl *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 2);
    }

    #[test]
    fn project_yaml_with_sources() {
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "name: TestProject\nsources:\n  - main.st\n  - lib.st\nentryPoint: Main\n"),
            ("main.st", "(* main *)"),
            ("lib.st", "(* lib *)"),
            ("unused.st", "(* not included *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.name, "TestProject");
        assert_eq!(project.source_files.len(), 2);
        assert_eq!(project.entry_point, Some("Main".to_string()));
    }

    #[test]
    fn project_yaml_autodiscover() {
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "name: AutoProject\nentryPoint: Main\n"),
            ("main.st", "(* main *)"),
            ("helpers.st", "(* helpers *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.name, "AutoProject");
        assert_eq!(project.source_files.len(), 2);
    }

    #[test]
    fn project_yaml_with_exclude() {
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "name: ExcludeTest\nexclude:\n  - test_*.st\n  - deprecated/\n"),
            ("main.st", "(* main *)"),
            ("test_something.st", "(* test *)"),
            ("deprecated/old.st", "(* old *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 1);
        assert!(project.source_files[0].ends_with("main.st"));
    }

    #[test]
    fn project_yaml_with_libraries() {
        // Use explicit sources + libraries to avoid double-counting
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "name: LibTest\nsources:\n  - main.st\nlibraries:\n  - custom_libs/\n"),
            ("main.st", "(* main *)"),
            ("custom_libs/mylib.st", "(* custom lib *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 2);
    }

    #[test]
    fn project_yaml_with_globs() {
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "name: GlobTest\nsources:\n  - \"*.st\"\n"),
            ("main.st", "(* main *)"),
            ("helper.st", "(* helper *)"),
            ("sub/nested.st", "(* nested — not matched by *.st *)"),
        ]);
        let project = discover_project(Some(&root)).unwrap();
        assert_eq!(project.source_files.len(), 2); // only root-level .st files
    }

    #[test]
    fn exclude_patterns() {
        assert!(is_excluded("test_main.st", Path::new("test_main.st"), &["test_*.st".to_string()]));
        assert!(!is_excluded("main.st", Path::new("main.st"), &["test_*.st".to_string()]));
        assert!(is_excluded("deprecated", Path::new("deprecated"), &["deprecated/".to_string()]));
        assert!(!is_excluded("main.st", Path::new("main.st"), &["deprecated/".to_string()]));
    }

    #[test]
    fn nonexistent_path_errors() {
        let result = discover_project(Some(Path::new("/nonexistent/path")));
        assert!(result.is_err());
    }

    #[test]
    fn yaml_parse_error() {
        let (_dir, root) = create_temp_project(&[
            ("plc-project.yaml", "invalid: yaml: [broken"),
        ]);
        let result = discover_project(Some(&root));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid YAML"));
    }
}
