use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

const SIDECAR: &str = "mission.yaml";
const LOCK: &str = "mission.yaml.lock";
const TEMP: &str = ".mission.yaml.tmp";
const MARKDOWN: &str = "mission.md";
const HTML: &str = "mission.html";

#[derive(Debug, Clone)]
pub struct MissionPaths {
    root: PathBuf,
    mission_dir: PathBuf,
}

impl MissionPaths {
    pub fn resolve(project_root: impl AsRef<Path>) -> Result<Self, MissionPathError> {
        let root = fs::canonicalize(project_root).map_err(MissionPathError::Io)?;
        if !root.is_dir() {
            return Err(MissionPathError::RootIsNotDirectory);
        }
        Ok(Self {
            mission_dir: root.join(".jcode"),
            root,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.root
    }
    pub fn project_root_id(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    pub fn mission_dir(&self) -> &Path {
        &self.mission_dir
    }
    pub fn sidecar(&self) -> PathBuf {
        self.mission_dir.join(SIDECAR)
    }
    pub fn lock(&self) -> PathBuf {
        self.mission_dir.join(LOCK)
    }
    pub fn temp(&self) -> PathBuf {
        self.mission_dir.join(TEMP)
    }
    pub fn markdown(&self) -> PathBuf {
        self.mission_dir.join(MARKDOWN)
    }
    pub fn html(&self) -> PathBuf {
        self.mission_dir.join(HTML)
    }

    pub fn ensure_mission_dir(&self) -> Result<(), MissionPathError> {
        match fs::symlink_metadata(&self.mission_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(MissionPathError::SymlinkedMissionDirectory);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&self.mission_dir).map_err(MissionPathError::Io)?
            }
            Err(error) => return Err(MissionPathError::Io(error)),
        }
        if !self.mission_dir.is_dir() {
            return Err(MissionPathError::MissionDirectoryIsNotDirectory);
        }
        Ok(())
    }

    pub fn normalize_relative(&self, value: &str) -> Result<String, MissionPathError> {
        if value.is_empty() || value.contains('\\') {
            return Err(MissionPathError::InvalidRelativePath);
        }
        let path = Path::new(value);
        if path.is_absolute() {
            return Err(MissionPathError::AbsolutePath);
        }
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => normalized.push(part),
                Component::CurDir => {}
                Component::ParentDir => return Err(MissionPathError::Traversal),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(MissionPathError::AbsolutePath);
                }
            }
        }
        if normalized.as_os_str().is_empty() {
            return Err(MissionPathError::InvalidRelativePath);
        }
        self.ensure_existing_prefix_confined(&normalized)?;
        normalized
            .to_str()
            .map(str::to_owned)
            .ok_or(MissionPathError::NonUnicodePath)
    }

    fn ensure_existing_prefix_confined(&self, normalized: &Path) -> Result<(), MissionPathError> {
        let mut current = self.root.clone();
        for component in normalized.components() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(_) => {
                    let resolved = fs::canonicalize(&current).map_err(MissionPathError::Io)?;
                    if !resolved.starts_with(&self.root) {
                        return Err(MissionPathError::SymlinkEscape);
                    }
                    current = resolved;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(MissionPathError::Io(error)),
            }
        }
        Ok(())
    }

    pub(crate) fn reject_symlink_if_exists(&self, path: &Path) -> Result<(), MissionPathError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(MissionPathError::SymlinkedStorage)
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(MissionPathError::Io(error)),
        }
    }
}

#[derive(Debug)]
pub enum MissionPathError {
    Io(std::io::Error),
    RootIsNotDirectory,
    MissionDirectoryIsNotDirectory,
    SymlinkedMissionDirectory,
    SymlinkedStorage,
    InvalidRelativePath,
    AbsolutePath,
    Traversal,
    SymlinkEscape,
    NonUnicodePath,
}

impl fmt::Display for MissionPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "mission path I/O error",
            Self::RootIsNotDirectory => "mission project root is not a directory",
            Self::MissionDirectoryIsNotDirectory => "mission directory is not a directory",
            Self::SymlinkedMissionDirectory => "mission directory must not be a symlink",
            Self::SymlinkedStorage => "mission storage path must not be a symlink",
            Self::InvalidRelativePath => "mission path must be a normalized relative path",
            Self::AbsolutePath => "mission path must not be absolute",
            Self::Traversal => "mission path must not traverse parent directories",
            Self::SymlinkEscape => "mission path escapes the project root through a symlink",
            Self::NonUnicodePath => "mission path must be UTF-8",
        })
    }
}
impl std::error::Error for MissionPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}
