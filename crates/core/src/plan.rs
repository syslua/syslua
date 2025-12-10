//! Plan computation and application

use crate::error::CoreError;
use crate::manifest::Manifest;
use std::fs;
use std::path::{Path, PathBuf};
use sys_lua::FileDecl;

/// The kind of change to make to a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChangeKind {
    /// Create a new symlink
    CreateSymlink { target: PathBuf },
    /// Create a new file with content
    CreateContent { content: String },
    /// Copy a file from source
    CopyFile { source: PathBuf },
    /// Update an existing symlink to point to a new target
    UpdateSymlink {
        old_target: PathBuf,
        new_target: PathBuf,
    },
    /// Update file content
    UpdateContent {
        old_content: String,
        new_content: String,
    },
    /// File is already in desired state
    Unchanged,
}

/// A planned change for a single file
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path to the file
    pub path: PathBuf,
    /// Kind of change
    pub kind: FileChangeKind,
    /// File mode to set (if any)
    pub mode: Option<u32>,
}

impl FileChange {
    /// Check if this change requires any action
    pub fn needs_action(&self) -> bool {
        !matches!(self.kind, FileChangeKind::Unchanged)
    }

    /// Get a human-readable description of the change
    pub fn description(&self) -> String {
        match &self.kind {
            FileChangeKind::CreateSymlink { target } => {
                format!("create symlink -> {}", target.display())
            }
            FileChangeKind::CreateContent { content } => {
                let lines = content.lines().count();
                format!("create file ({} lines)", lines)
            }
            FileChangeKind::CopyFile { source } => {
                format!("copy from {}", source.display())
            }
            FileChangeKind::UpdateSymlink {
                old_target,
                new_target,
            } => {
                format!(
                    "update symlink {} -> {}",
                    old_target.display(),
                    new_target.display()
                )
            }
            FileChangeKind::UpdateContent { .. } => "update content".to_string(),
            FileChangeKind::Unchanged => "unchanged".to_string(),
        }
    }
}

/// A plan of changes to apply
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// File changes
    pub files: Vec<FileChange>,
}

impl Plan {
    /// Create an empty plan
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    /// Check if the plan has any changes
    pub fn has_changes(&self) -> bool {
        self.files.iter().any(|f| f.needs_action())
    }

    /// Get the count of changes that need action
    pub fn change_count(&self) -> usize {
        self.files.iter().filter(|f| f.needs_action()).count()
    }

    /// Get only the changes that need action
    pub fn changes(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.needs_action())
    }
}

/// Compute a plan by comparing manifest to current system state
pub fn compute_plan(manifest: &Manifest) -> Result<Plan, CoreError> {
    let mut plan = Plan::new();

    for file_decl in &manifest.files {
        let change = compute_file_change(file_decl)?;
        plan.files.push(change);
    }

    Ok(plan)
}

/// Compute the change needed for a single file declaration
fn compute_file_change(decl: &FileDecl) -> Result<FileChange, CoreError> {
    let path = &decl.path;

    // Check current state
    let exists = path.exists() || path.symlink_metadata().is_ok();

    if let Some(target) = &decl.symlink {
        // Symlink case
        if exists {
            // Check if it's already a symlink to the same target
            if let Ok(metadata) = path.symlink_metadata() {
                if metadata.file_type().is_symlink() {
                    if let Ok(current_target) = fs::read_link(path) {
                        if current_target == *target {
                            return Ok(FileChange {
                                path: path.clone(),
                                kind: FileChangeKind::Unchanged,
                                mode: decl.mode,
                            });
                        } else {
                            return Ok(FileChange {
                                path: path.clone(),
                                kind: FileChangeKind::UpdateSymlink {
                                    old_target: current_target,
                                    new_target: target.clone(),
                                },
                                mode: decl.mode,
                            });
                        }
                    }
                }
            }
            // Exists but not a symlink, or symlink to different target
            return Ok(FileChange {
                path: path.clone(),
                kind: FileChangeKind::UpdateSymlink {
                    old_target: PathBuf::from("(not a symlink)"),
                    new_target: target.clone(),
                },
                mode: decl.mode,
            });
        } else {
            return Ok(FileChange {
                path: path.clone(),
                kind: FileChangeKind::CreateSymlink {
                    target: target.clone(),
                },
                mode: decl.mode,
            });
        }
    }

    if let Some(content) = &decl.content {
        // Content case
        if exists {
            // Check if content matches
            if let Ok(current_content) = fs::read_to_string(path) {
                if current_content == *content {
                    return Ok(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::Unchanged,
                        mode: decl.mode,
                    });
                } else {
                    return Ok(FileChange {
                        path: path.clone(),
                        kind: FileChangeKind::UpdateContent {
                            old_content: current_content,
                            new_content: content.clone(),
                        },
                        mode: decl.mode,
                    });
                }
            }
        }
        return Ok(FileChange {
            path: path.clone(),
            kind: FileChangeKind::CreateContent {
                content: content.clone(),
            },
            mode: decl.mode,
        });
    }

    if let Some(source) = &decl.copy {
        // Copy case
        return Ok(FileChange {
            path: path.clone(),
            kind: FileChangeKind::CopyFile {
                source: source.clone(),
            },
            mode: decl.mode,
        });
    }

    // Should not happen if FileDecl::validate() was called
    Err(CoreError::FileOperation {
        path: path.display().to_string(),
        message: "No symlink, content, or copy specified".to_string(),
    })
}

/// Apply options for the apply function
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// Force overwrite of existing files
    pub force: bool,
    /// Dry run - don't actually make changes
    pub dry_run: bool,
}

/// Apply a plan to the system
pub fn apply(plan: &Plan, options: &ApplyOptions) -> Result<(), CoreError> {
    for change in plan.changes() {
        apply_file_change(change, options)?;
    }
    Ok(())
}

/// Apply a single file change
fn apply_file_change(change: &FileChange, options: &ApplyOptions) -> Result<(), CoreError> {
    if options.dry_run {
        return Ok(());
    }

    let path = &change.path;

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    match &change.kind {
        FileChangeKind::CreateSymlink { target } => {
            create_symlink(target, path)?;
        }
        FileChangeKind::UpdateSymlink { new_target, .. } => {
            // Remove existing file/symlink
            if path.symlink_metadata().is_ok() {
                if path.is_dir() && !path.symlink_metadata()?.file_type().is_symlink() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
            create_symlink(new_target, path)?;
        }
        FileChangeKind::CreateContent { content } => {
            fs::write(path, content)?;
            if let Some(mode) = change.mode {
                set_permissions(path, mode)?;
            }
        }
        FileChangeKind::UpdateContent { new_content, .. } => {
            fs::write(path, new_content)?;
            if let Some(mode) = change.mode {
                set_permissions(path, mode)?;
            }
        }
        FileChangeKind::CopyFile { source } => {
            fs::copy(source, path)?;
            if let Some(mode) = change.mode {
                set_permissions(path, mode)?;
            }
        }
        FileChangeKind::Unchanged => {}
    }

    Ok(())
}

/// Create a symbolic link
fn create_symlink(target: &Path, link: &Path) -> Result<(), CoreError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)?;
    }

    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)?;
        } else {
            std::os::windows::fs::symlink_file(target, link)?;
        }
    }

    Ok(())
}

/// Set file permissions
#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> Result<(), CoreError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(windows)]
fn set_permissions(_path: &Path, _mode: u32) -> Result<(), CoreError> {
    // Windows doesn't use Unix permissions
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_compute_plan_symlink_create() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("target.txt");
        fs::write(&target, "content").unwrap();

        let manifest = Manifest {
            files: vec![FileDecl {
                path: temp_dir.path().join("link.txt"),
                symlink: Some(target.clone()),
                content: None,
                copy: None,
                mode: None,
            }],
            envs: vec![],
        };

        let plan = compute_plan(&manifest).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert!(matches!(
            plan.files[0].kind,
            FileChangeKind::CreateSymlink { .. }
        ));
    }

    #[test]
    fn test_compute_plan_symlink_unchanged() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("target.txt");
        let link = temp_dir.path().join("link.txt");

        fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let manifest = Manifest {
            files: vec![FileDecl {
                path: link.clone(),
                symlink: Some(target.clone()),
                content: None,
                copy: None,
                mode: None,
            }],
            envs: vec![],
        };

        let plan = compute_plan(&manifest).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert!(matches!(plan.files[0].kind, FileChangeKind::Unchanged));
        assert!(!plan.has_changes());
    }

    #[test]
    fn test_compute_plan_content_create() {
        let temp_dir = TempDir::new().unwrap();

        let manifest = Manifest {
            files: vec![FileDecl {
                path: temp_dir.path().join("new.txt"),
                symlink: None,
                content: Some("Hello, World!".to_string()),
                copy: None,
                mode: None,
            }],
            envs: vec![],
        };

        let plan = compute_plan(&manifest).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert!(matches!(
            plan.files[0].kind,
            FileChangeKind::CreateContent { .. }
        ));
    }

    #[test]
    fn test_apply_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("target.txt");
        let link = temp_dir.path().join("link.txt");

        fs::write(&target, "content").unwrap();

        let plan = Plan {
            files: vec![FileChange {
                path: link.clone(),
                kind: FileChangeKind::CreateSymlink {
                    target: target.clone(),
                },
                mode: None,
            }],
        };

        apply(&plan, &ApplyOptions::default()).unwrap();

        assert!(link.exists());
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn test_apply_content() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("test.txt");

        let plan = Plan {
            files: vec![FileChange {
                path: file.clone(),
                kind: FileChangeKind::CreateContent {
                    content: "Hello, World!".to_string(),
                },
                mode: None,
            }],
        };

        apply(&plan, &ApplyOptions::default()).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_apply_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("a/b/c/test.txt");

        let plan = Plan {
            files: vec![FileChange {
                path: file.clone(),
                kind: FileChangeKind::CreateContent {
                    content: "nested".to_string(),
                },
                mode: None,
            }],
        };

        apply(&plan, &ApplyOptions::default()).unwrap();

        assert!(file.exists());
        assert_eq!(fs::read_to_string(&file).unwrap(), "nested");
    }
}
