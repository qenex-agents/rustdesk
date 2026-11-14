//! QENEX seat binding.
//!
//! Every QENEX bot owns a private desktop and a private working tree on its seat
//! host. When the client runs inside a seat, all of its writable state -- the
//! configuration, the data and cache directories, the logs, the autostart entries
//! and the temporary files -- has to live inside that seat's workspace, so that
//! one bot never reads or clobbers another bot's (or the host operator's) client
//! state.
//!
//! The rules implemented here come from the shared QENEX seat binding contract:
//!
//! * R1: the process is seat-bound iff `QENEX_SEAT` is set and non-empty.
//! * R2: while seat-bound every writable root resolves under `QENEX_SEAT_WORKSPACE`
//!   and a write outside of it is refused with an error that names the offending
//!   path and mentions `QENEX_SEAT_ALLOW_GLOBAL=1`.
//! * R3: reads are never restricted, only writes are confined.
//! * R4: the seat identity is observable through [`SeatBinding::info_line`].

use std::path::{Component, Path, PathBuf};

/// Seat identifier. Set and non-empty means the process is seat-bound.
pub const ENV_SEAT: &str = "QENEX_SEAT";
/// Absolute path of the seat working tree, the only writable root while seat-bound.
pub const ENV_WORKSPACE: &str = "QENEX_SEAT_WORKSPACE";
/// Set to `1` to disable the confinement, for operator and maintenance use.
pub const ENV_ALLOW_GLOBAL: &str = "QENEX_SEAT_ALLOW_GLOBAL";

/// Name of the application directory below the seat workspace.
///
/// It matches the application name `hbb_common` uses, so the roots declared here
/// are exactly the directories the client writes to.
pub const APP_DIR_NAME: &str = "RustDesk";

/// The writable roots of the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// Configuration files.
    Config,
    /// Application data.
    Data,
    /// Caches.
    Cache,
    /// State that is neither configuration nor application data.
    State,
    /// Scratch space.
    Tmp,
    /// Home directory, used for downloads, logs and autostart entries.
    Home,
}

impl Root {
    /// The directory of the seat workspace this root lives in.
    fn base_dir(self) -> &'static str {
        match self {
            Root::Config => ".config",
            Root::Data => ".data",
            Root::Cache => ".cache",
            Root::State => ".state",
            Root::Tmp => ".tmp",
            Root::Home => "home",
        }
    }

    /// Whether the root holds the application directory below its base directory.
    fn is_scoped(self) -> bool {
        !matches!(self, Root::Tmp | Root::Home)
    }
}

/// A resolved seat binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatBinding {
    /// Seat identifier, for example `coder`.
    pub seat: String,
    /// Absolute path of the seat working tree.
    pub workspace: PathBuf,
}

impl SeatBinding {
    /// The root of `kind` inside the seat workspace.
    pub fn root(&self, kind: Root) -> PathBuf {
        let base = self.workspace.join(kind.base_dir());
        if kind.is_scoped() {
            base.join(APP_DIR_NAME)
        } else {
            base
        }
    }

    /// Whether `path` is inside the seat workspace.
    pub fn contains(&self, path: &Path) -> bool {
        normalize(path).starts_with(normalize(&self.workspace))
    }

    /// Refuse a write that escapes the seat workspace.
    pub fn ensure_writable(&self, path: &Path) -> Result<(), SeatError> {
        if self.contains(path) {
            Ok(())
        } else {
            Err(SeatError::outside(self, path))
        }
    }

    /// The seat identity reported by the client info output (R4).
    pub fn info_line(&self) -> String {
        format!("seat={} workspace={}", self.seat, self.workspace.display())
    }
}

/// A write that was refused because it escapes the seat workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatError {
    /// The offending path.
    pub path: PathBuf,
    /// Human readable description of the refusal.
    pub message: String,
}

impl SeatError {
    fn outside(binding: &SeatBinding, path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            message: format!(
                "seat '{}' refuses to write '{}': the path is outside the seat workspace '{}'; set {}=1 to disable seat confinement",
                binding.seat,
                path.display(),
                binding.workspace.display(),
                ENV_ALLOW_GLOBAL
            ),
        }
    }
}

impl std::fmt::Display for SeatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SeatError {}

/// Resolve a seat binding from the given environment values (R1).
///
/// Returns `Ok(None)` when the process is not seat-bound and an error when the
/// seat is announced but its workspace is missing, because the seat scoped roots
/// cannot be resolved then.
pub fn detect(seat: Option<&str>, workspace: Option<&str>) -> Result<Option<SeatBinding>, String> {
    let seat = match seat.map(str::trim) {
        Some(seat) if !seat.is_empty() => seat,
        _ => return Ok(None),
    };
    let workspace = match workspace.map(str::trim) {
        Some(workspace) if !workspace.is_empty() => workspace,
        _ => {
            return Err(format!(
                "{} is set to '{}' but {} is not set: the seat scoped roots cannot be resolved",
                ENV_SEAT, seat, ENV_WORKSPACE
            ))
        }
    };
    Ok(Some(SeatBinding {
        seat: seat.to_owned(),
        workspace: PathBuf::from(workspace),
    }))
}

/// The seat binding of the current process, if it is seat-bound.
pub fn current() -> Result<Option<SeatBinding>, String> {
    detect(
        std::env::var(ENV_SEAT).ok().as_deref(),
        std::env::var(ENV_WORKSPACE).ok().as_deref(),
    )
}

/// Whether the confinement escape hatch is enabled.
pub fn allow_global() -> bool {
    match std::env::var(ENV_ALLOW_GLOBAL) {
        Ok(value) => value.trim() == "1",
        Err(_) => false,
    }
}

/// The seat identity of `binding`, `None` when the process is not seat-bound (R4).
pub fn format_info(binding: Option<&SeatBinding>) -> Option<String> {
    binding.map(|binding| binding.info_line())
}

/// The seat identity of the current process, `None` when it is not seat-bound (R4).
pub fn info_line() -> Option<String> {
    match current() {
        Ok(binding) => format_info(binding.as_ref()),
        Err(_) => None,
    }
}

/// Bind the writable roots of the current process to its seat (R2).
///
/// Returns the binding when the process is seat-bound and `None` when it is not.
/// A process that is not seat-bound is left completely untouched, so the legacy
/// global behaviour is kept.
///
/// # Errors
///
/// Returns an error when the seat is announced but cannot be bound, or when a
/// root the client uses still resolves outside of the seat workspace after the
/// redirection.
pub fn apply() -> Result<Option<SeatBinding>, String> {
    let binding = match current()? {
        Some(binding) => binding,
        None => return Ok(None),
    };
    if allow_global() {
        // Escape hatch: keep the legacy global roots. The seat identity is still
        // reported, so that it stays visible that the confinement is disabled.
        return Ok(Some(binding));
    }
    redirect(&binding)?;
    verify(&binding)?;
    Ok(Some(binding))
}

/// Redirect every writable root of the process into the seat workspace.
fn redirect(binding: &SeatBinding) -> Result<(), String> {
    for kind in [
        Root::Config,
        Root::Data,
        Root::Cache,
        Root::State,
        Root::Tmp,
        Root::Home,
    ] {
        let root = binding.root(kind);
        std::fs::create_dir_all(&root).map_err(|e| {
            format!(
                "failed to create the seat scoped root '{}': {}",
                root.display(),
                e
            )
        })?;
    }
    // `hbb_common` resolves the configuration directory through
    // `directories_next::ProjectDirs` and the home directory through
    // `dirs_next::home_dir`. Both read the environment on every call, so the
    // redirection below moves every root the client writes to into the seat
    // workspace.
    set_var("XDG_CONFIG_HOME", binding.root(Root::Config).parent());
    set_var("XDG_DATA_HOME", binding.root(Root::Data).parent());
    set_var("XDG_CACHE_HOME", binding.root(Root::Cache).parent());
    set_var("XDG_STATE_HOME", binding.root(Root::State).parent());
    set_var("TMPDIR", Some(&binding.root(Root::Tmp)));
    set_var("TMP", Some(&binding.root(Root::Tmp)));
    set_var("TEMP", Some(&binding.root(Root::Tmp)));
    set_var("HOME", Some(&binding.root(Root::Home)));
    set_var("USERPROFILE", Some(&binding.root(Root::Home)));
    set_var("APPDATA", binding.root(Root::Config).parent());
    set_var("LOCALAPPDATA", binding.root(Root::Data).parent());
    Ok(())
}

/// Check that the roots the client uses really are inside the seat workspace.
fn verify(binding: &SeatBinding) -> Result<(), String> {
    // The configuration directory holds most of the client state.
    let config_root = hbb_common::config::Config::path("");
    binding
        .ensure_writable(&config_root)
        .map_err(|e| e.to_string())?;
    // The home directory is used for downloads, logs and autostart entries.
    let home = hbb_common::config::Config::get_home();
    binding.ensure_writable(&home).map_err(|e| e.to_string())?;
    Ok(())
}

fn set_var(key: &str, value: Option<&Path>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    }
}

/// Resolve `.` and `..` in a path without touching the file system.
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "/var/lib/qenex/seats/coder/workspace";

    fn binding() -> SeatBinding {
        SeatBinding {
            seat: "coder".to_owned(),
            workspace: PathBuf::from(WORKSPACE),
        }
    }

    /// R7.1: an unbound process keeps the legacy, global behaviour.
    #[test]
    fn unbound_keeps_the_legacy_behaviour() {
        assert!(detect(None, None).unwrap().is_none());
        assert!(detect(Some(""), None).unwrap().is_none());
        assert!(detect(Some("   "), Some(WORKSPACE)).unwrap().is_none());
        // No seat is claimed when the process is not seat-bound.
        assert!(format_info(None).is_none());
    }

    /// R7.2: a bound process resolves its roots inside the seat workspace.
    #[test]
    fn bound_resolves_its_roots_inside_the_workspace() {
        let binding = binding();
        assert_eq!(
            binding.root(Root::Config),
            PathBuf::from(WORKSPACE).join(".config").join(APP_DIR_NAME)
        );
        assert_eq!(
            binding.root(Root::Data),
            PathBuf::from(WORKSPACE).join(".data").join(APP_DIR_NAME)
        );
        assert!(binding.contains(&binding.root(Root::Config)));
        assert!(binding.contains(&binding.root(Root::Home)));
        // A write inside the workspace is allowed.
        assert!(binding
            .ensure_writable(&binding.root(Root::Config).join("RustDesk.toml"))
            .is_ok());
        // The seat identity is observable.
        let info = binding.info_line();
        assert!(info.contains("seat=coder"));
        assert!(info.contains(&format!("workspace={}", WORKSPACE)));
    }

    /// R7.3: a bound process refuses writes outside of the seat workspace.
    #[test]
    fn bound_refuses_writes_outside_the_workspace() {
        let binding = binding();
        let error = binding
            .ensure_writable(Path::new("/etc/rustdesk/config.toml"))
            .unwrap_err();
        assert_eq!(error.path, PathBuf::from("/etc/rustdesk/config.toml"));
        let message = error.to_string();
        assert!(message.contains("/etc/rustdesk/config.toml"));
        assert!(message.contains("QENEX_SEAT_ALLOW_GLOBAL=1"));
        // A traversal out of the workspace is refused as well.
        assert!(binding
            .ensure_writable(
                &binding
                    .root(Root::Config)
                    .join("..")
                    .join("..")
                    .join("..")
                    .join("etc")
            )
            .is_err());
    }

    /// A seat without a workspace cannot be bound.
    #[test]
    fn a_seat_without_a_workspace_is_an_error() {
        assert!(detect(Some("coder"), None).is_err());
        assert!(detect(Some("coder"), Some("")).is_err());
        assert!(detect(Some("coder"), Some(WORKSPACE)).unwrap().is_some());
    }
}
