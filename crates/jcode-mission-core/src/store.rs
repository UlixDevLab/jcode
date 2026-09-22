use crate::{
    MissionArtifact, MissionCas, MissionPathError, MissionPaths, MissionProjection,
    MissionTransition, MissionTransitionError, render_projection,
};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct MissionStore {
    paths: MissionPaths,
}

impl MissionStore {
    pub fn open(project_root: impl AsRef<Path>) -> Result<Self, MissionStoreError> {
        Ok(Self {
            paths: MissionPaths::resolve(project_root).map_err(MissionStoreError::Path)?,
        })
    }

    pub fn paths(&self) -> &MissionPaths {
        &self.paths
    }

    pub fn create(&self, artifact: &MissionArtifact) -> Result<(), MissionStoreError> {
        self.with_lock(|| {
            self.recover_sidecar_temp()?;
            if self.paths.sidecar().exists() {
                return Err(MissionStoreError::AlreadyExists);
            }
            self.persist_locked(artifact)
        })
    }

    pub fn load(&self) -> Result<MissionArtifact, MissionStoreError> {
        self.with_lock(|| {
            self.recover_sidecar_temp()?;
            self.load_locked()
        })
    }

    pub fn compare_and_swap(
        &self,
        expected: &MissionCas,
        transition: MissionTransition,
    ) -> Result<MissionArtifact, MissionStoreError> {
        self.with_lock(|| {
            self.recover_sidecar_temp()?;
            let current = self.load_locked()?;
            expected
                .verify(&current)
                .map_err(MissionStoreError::Transition)?;
            let mut next = current.clone();
            transition
                .apply(&mut next)
                .map_err(MissionStoreError::Transition)?;
            self.persist_locked(&next)?;
            Ok(next)
        })
    }

    pub fn persist_atomic(&self, artifact: &MissionArtifact) -> Result<(), MissionStoreError> {
        self.with_lock(|| self.persist_locked(artifact))
    }

    fn with_lock<T>(
        &self,
        action: impl FnOnce() -> Result<T, MissionStoreError>,
    ) -> Result<T, MissionStoreError> {
        self.paths
            .ensure_mission_dir()
            .map_err(MissionStoreError::Path)?;
        self.paths
            .reject_symlink_if_exists(&self.paths.lock())
            .map_err(MissionStoreError::Path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(self.paths.lock())
            .map_err(MissionStoreError::Io)?;
        file.lock().map_err(MissionStoreError::Io)?;
        let _guard = MissionLock { _file: file };
        action()
    }

    fn load_locked(&self) -> Result<MissionArtifact, MissionStoreError> {
        let sidecar = self.paths.sidecar();
        self.paths
            .reject_symlink_if_exists(&sidecar)
            .map_err(MissionStoreError::Path)?;
        let bytes = fs::read(&sidecar).map_err(MissionStoreError::Io)?;
        let artifact = serde_yaml::from_slice::<MissionArtifact>(&bytes)
            .map_err(MissionStoreError::InvalidSidecar)?;
        self.validate_for_store(&artifact)?;
        Ok(artifact)
    }

    fn persist_locked(&self, artifact: &MissionArtifact) -> Result<(), MissionStoreError> {
        self.validate_for_store(artifact)?;
        let projection = render_projection(artifact);
        self.write_projection(&projection)?;
        let encoded = serde_yaml::to_string(artifact).map_err(MissionStoreError::Encode)?;
        self.atomic_write(
            &self.paths.sidecar(),
            &self.paths.temp(),
            encoded.as_bytes(),
        )
    }

    fn write_projection(&self, projection: &MissionProjection) -> Result<(), MissionStoreError> {
        self.atomic_write(
            &self.paths.markdown(),
            &self.paths.mission_dir().join(".mission.md.tmp"),
            &projection.markdown,
        )?;
        self.atomic_write(
            &self.paths.html(),
            &self.paths.mission_dir().join(".mission.html.tmp"),
            &projection.html,
        )
    }

    fn validate_for_store(&self, artifact: &MissionArtifact) -> Result<(), MissionStoreError> {
        artifact
            .validate()
            .map_err(MissionStoreError::InvalidArtifact)?;
        if artifact.project_root_id() != self.paths.project_root_id() {
            return Err(MissionStoreError::ForeignProjectRoot);
        }
        for entry in artifact.scope_entries() {
            let normalized = self
                .paths
                .normalize_relative(&entry.relative_path)
                .map_err(MissionStoreError::Path)?;
            if normalized != entry.relative_path {
                return Err(MissionStoreError::PathNotNormalized);
            }
        }
        Ok(())
    }

    fn recover_sidecar_temp(&self) -> Result<(), MissionStoreError> {
        self.remove_known_temp(&self.paths.temp())
    }

    fn remove_known_temp(&self, temp: &Path) -> Result<(), MissionStoreError> {
        self.paths
            .reject_symlink_if_exists(temp)
            .map_err(MissionStoreError::Path)?;
        match fs::remove_file(temp) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(MissionStoreError::Io(error)),
        }
    }

    fn atomic_write(
        &self,
        destination: &Path,
        temporary: &Path,
        bytes: &[u8],
    ) -> Result<(), MissionStoreError> {
        self.paths
            .reject_symlink_if_exists(destination)
            .map_err(MissionStoreError::Path)?;
        self.remove_known_temp(temporary)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)
            .map_err(MissionStoreError::Io)?;
        file.write_all(bytes).map_err(MissionStoreError::Io)?;
        file.sync_all().map_err(MissionStoreError::Io)?;
        fs::rename(temporary, destination).map_err(MissionStoreError::Io)?;
        sync_directory(self.paths.mission_dir()).map_err(MissionStoreError::Io)
    }
}

struct MissionLock {
    _file: File,
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    match File::open(path)?.sync_all() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub enum MissionStoreError {
    Path(MissionPathError),
    Io(std::io::Error),
    Encode(serde_yaml::Error),
    InvalidSidecar(serde_yaml::Error),
    InvalidArtifact(crate::MissionArtifactError),
    AlreadyExists,
    ForeignProjectRoot,
    PathNotNormalized,
    Transition(MissionTransitionError),
}

impl fmt::Display for MissionStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Path(error) => return error.fmt(f),
            Self::Io(error) => return error.fmt(f),
            Self::Encode(error) => return error.fmt(f),
            Self::InvalidSidecar(error) => return write!(f, "invalid mission sidecar: {error}"),
            Self::InvalidArtifact(error) => return error.fmt(f),
            Self::AlreadyExists => "mission sidecar already exists",
            Self::ForeignProjectRoot => "mission artifact belongs to another project root",
            Self::PathNotNormalized => "mission artifact contains a non-normalized scope path",
            Self::Transition(error) => return error.fmt(f),
        })
    }
}
impl std::error::Error for MissionStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Path(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Encode(error) | Self::InvalidSidecar(error) => Some(error),
            Self::InvalidArtifact(error) => Some(error),
            Self::AlreadyExists
            | Self::ForeignProjectRoot
            | Self::PathNotNormalized
            | Self::Transition(_) => None,
        }
    }
}
