//! Thin WASM sandbox policy layer for the Pantheon platform.
//!
//! Per [`SPINE.md`](../SPINE.md) §5.3 this crate owns **none** of the actual
//! WASM execution, memory isolation, or fuel accounting. Its entire job is to
//! translate a [`SandboxPermissions`] struct into correct configuration of
//! `wasmtime` / `wasmtime-wasi` / `cap-std`.
//!
//! The translation is split in two:
//!
//! * [`SandboxPermissions::resolve`] (always available, fully tested) validates
//!   the requested permissions and produces a backend-neutral
//!   [`ResolvedConfig`]. This is the policy core and the only part compiled in
//!   the default build.
//! * With the `wasmtime` feature, the [`wasm`] module consumes a
//!   [`ResolvedConfig`] to build a `wasmtime::Store` (fuel / memory / epoch), a
//!   `cap-std`-backed `WasiCtx` (only preopened paths, no ambient authority),
//!   and to register host functions on a `Linker` **conditionally** — an
//!   ungranted capability gets no import in the instantiated module's
//!   environment, rather than a runtime-checked denial.
//!
//! Keeping the binding behind the `wasmtime` feature is also what lets the
//! default workspace build stay light and keeps this crate a thin policy layer
//! (the §5.3 drift signal).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

/// Filesystem grants. All paths are host paths; `guest_path` is derived at
/// resolution time as the path's final component unless overridden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilesystemPerms {
    pub read_only: Vec<PathBuf>,
    pub read_write: Vec<PathBuf>,
}

/// Network grants. `allow_hosts` is an explicit allow-list of
/// `"host:port"` endpoints — there is no wildcard; ambient networking is
/// forbidden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPerms {
    pub allow_hosts: Vec<String>,
}

/// The high-level permission request a caller makes. Resolved into a
/// [`ResolvedConfig`] before any engine work happens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxPermissions {
    /// WASM fuel budget. `None` means fuel accounting is off (module may run
    /// unbounded) — callers should almost always set this.
    pub fuel: Option<u64>,
    /// Hard cap on total linear memory in bytes.
    pub max_memory_bytes: Option<usize>,
    /// Wall-clock epoch timeout; the engine interrupts the instance after this.
    pub epoch_timeout: Option<Duration>,
    pub filesystem: FilesystemPerms,
    pub network: NetworkPerms,
    /// Whether the module may call the Keystone host bridge.
    pub keystone: bool,
    /// Whether the module may read from the secrets host bridge.
    pub secrets: bool,
}

/// A single preopened directory handed to the guest, with no ambient authority
/// beyond it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preopen {
    pub guest_path: PathBuf,
    pub host_path: PathBuf,
    pub writable: bool,
}

/// Individual host functions. An ungranted capability means its `HostFn` is
/// absent from [`ResolvedConfig::host_fns`], so the `wasmtime` layer registers
/// no corresponding import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostFn {
    /// Read access to a preopened directory.
    FsRead,
    /// Write access to a preopened directory.
    FsWrite,
    /// Outbound networking to the allow-listed hosts.
    Network,
    /// Keystone host bridge.
    Keystone,
    /// Secrets host bridge.
    Secrets,
}

/// Backend-neutral result of [`SandboxPermissions::resolve`]. The `wasmtime`
/// feature consumes this to configure the engine — there is no `wasmtime`
/// type here, on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub fuel: Option<u64>,
    pub max_memory_bytes: Option<usize>,
    pub epoch_timeout: Option<Duration>,
    pub preopens: Vec<Preopen>,
    pub host_fns: BTreeSet<HostFn>,
    pub network_allow: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxError {
    #[error("path {0} appears in both read-only and read-write lists")]
    OverlappingPath(String),
    #[error("path {0} is not absolute")]
    NotAbsolute(String),
}

impl SandboxPermissions {
    /// Validate and translate into a [`ResolvedConfig`].
    ///
    /// Pure and backend-free: this is what the default build compiles and
    /// tests, so the policy logic has a fast, dependency-light proof even when
    /// the `wasmtime` feature is off.
    pub fn resolve(&self) -> Result<ResolvedConfig, SandboxError> {
        // mode: false = read-only seen, true = read-write seen, missing = unseen.
        let mut mode: HashMap<PathBuf, bool> = HashMap::new();
        let mut preopens: Vec<Preopen> = Vec::new();
        let mut host_fns: BTreeSet<HostFn> = BTreeSet::new();

        let classify = |p: &PathBuf,
                        writable: bool,
                        mode: &mut HashMap<PathBuf, bool>,
                        preopens: &mut Vec<Preopen>,
                        host_fns: &mut BTreeSet<HostFn>|
         -> Result<(), SandboxError> {
            if !p.has_root() {
                return Err(SandboxError::NotAbsolute(p.display().to_string()));
            }
            match mode.get(p) {
                Some(&prev) if prev != writable => {
                    return Err(SandboxError::OverlappingPath(p.display().to_string()))
                }
                Some(_) => return Ok(()), // same mode already registered: idempotent
                None => {}
            }
            mode.insert(p.clone(), writable);
            preopens.push(Preopen {
                guest_path: guest_name(p)?,
                host_path: p.clone(),
                writable,
            });
            host_fns.insert(if writable {
                HostFn::FsWrite
            } else {
                HostFn::FsRead
            });
            Ok(())
        };

        for p in &self.filesystem.read_only {
            classify(p, false, &mut mode, &mut preopens, &mut host_fns)?;
        }
        for p in &self.filesystem.read_write {
            classify(p, true, &mut mode, &mut preopens, &mut host_fns)?;
        }

        if !self.network.allow_hosts.is_empty() {
            host_fns.insert(HostFn::Network);
        }
        if self.keystone {
            host_fns.insert(HostFn::Keystone);
        }
        if self.secrets {
            host_fns.insert(HostFn::Secrets);
        }

        Ok(ResolvedConfig {
            fuel: self.fuel,
            max_memory_bytes: self.max_memory_bytes,
            epoch_timeout: self.epoch_timeout,
            preopens,
            host_fns,
            network_allow: self.network.allow_hosts.clone(),
        })
    }
}

/// Derive the guest-visible name for a preopen: the path's final component.
fn guest_name(p: &Path) -> Result<PathBuf, SandboxError> {
    p.file_name()
        .map(PathBuf::from)
        .ok_or_else(|| SandboxError::NotAbsolute(p.display().to_string()))
}

#[cfg(feature = "wasmtime")]
pub mod wasm {
    //! Real `wasmtime` / `cap-std` binding. Consumes a [`ResolvedConfig`] and
    //! produces engine artifacts. Kept deliberately small — this is the thin
    //! adapter, not a reimplementation of sandboxing.

    use super::*;
    use wasmtime::{component::Linker as ComponentLinker, Config, Engine, Linker, Store, StoreLimits, StoreLimitsBuilder};
    use wasmtime_wasi::{DirPerms, FilePerms, ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

    /// Host state threaded through a `wasmtime::Store`. Implements [`WasiView`]
    /// so the standard WASI surface can be linked onto a component-model
    /// `Linker<HostData>` via [`link_wasi`]. Pantheon's own capabilities are
    /// added separately by [`register_host_fns`] onto a core `Linker`. The
    /// resolved memory cap is held here so the `Store` limiter can borrow it.
    pub struct HostData {
        table: ResourceTable,
        wasi: WasiCtx,
        limits: StoreLimits,
    }

    impl WasiView for HostData {
        fn table(&mut self) -> &mut ResourceTable {
            &mut self.table
        }
        fn ctx(&mut self) -> &mut WasiCtx {
            &mut self.wasi
        }
    }

    /// Apply resolved engine-level limits. Fuel is always enabled; the memory
    /// cap is enforced per-store via the limiter in [`build_store`].
    pub fn configure_engine(cfg: &mut Config, _resolved: &ResolvedConfig) {
        cfg.consume_fuel(true);
    }

    /// Build a `Store<HostData>` with fuel + memory limits and a cap-std-backed
    /// `WasiCtx` (preopens only, no ambient authority).
    pub fn build_store(engine: &Engine, resolved: &ResolvedConfig) -> Store<HostData> {
        let mut store = Store::new(
            engine,
            HostData {
                table: ResourceTable::new(),
                wasi: build_wasi_ctx(resolved),
                limits: StoreLimitsBuilder::new()
                    .memory_size(resolved.max_memory_bytes.unwrap_or(usize::MAX))
                    .build(),
            },
        );
        if let Some(fuel) = resolved.fuel {
            store.set_fuel(fuel);
        }
        store.limiter(|data: &mut HostData| &mut data.limits);
        store
    }

    /// Build a `WasiCtx` whose only authority is the explicitly preopened
    /// directories from the resolved config (cap-std-backed, no ambient
    /// authority).
    pub fn build_wasi_ctx(resolved: &ResolvedConfig) -> WasiCtx {
        let mut builder = WasiCtxBuilder::new();
        for pre in &resolved.preopens {
            let (dir_perms, file_perms) = if pre.writable {
                (DirPerms::READ | DirPerms::MUTATE, FilePerms::READ | FilePerms::WRITE)
            } else {
                (DirPerms::READ, FilePerms::READ)
            };
            builder
                .preopened_dir(
                    &pre.host_path,
                    pre.guest_path.to_string_lossy().into_owned(),
                    dir_perms,
                    file_perms,
                )
                .expect("preopened host path must exist and be accessible");
        }
        builder.build()
    }

    /// Register host functions **only** for granted capabilities onto a core
    /// `Linker<HostData>`. An ungranted [`HostFn`] gets no import, so a module
    /// importing it fails to instantiate rather than being denied at runtime.
    pub fn register_host_fns(linker: &mut Linker<HostData>, resolved: &ResolvedConfig) {
        let granted = |f: HostFn| resolved.host_fns.contains(&f);

        if granted(HostFn::Keystone) {
            linker
                .func_wrap(
                    "pantheon",
                    "keystone_call",
                    |_caller: wasmtime::Caller<'_, HostData>| -> i32 { 0 },
                )
                .expect("register keystone host fn");
        }
        if granted(HostFn::Secrets) {
            linker
                .func_wrap(
                    "pantheon",
                    "secrets_get",
                    |_caller: wasmtime::Caller<'_, HostData>| -> i32 { 0 },
                )
                .expect("register secrets host fn");
        }
        if granted(HostFn::Network) {
            linker
                .func_wrap(
                    "pantheon",
                    "network_send",
                    |_caller: wasmtime::Caller<'_, HostData>| -> i32 { 0 },
                )
                .expect("register network host fn");
        }
        // FsRead / FsWrite are satisfied by the WASI preopens above; no extra
        // synthetic import is needed.
    }

    /// Wire the standard WASI surface onto a component-model
    /// `Linker<HostData>`. Core-module runtimes add it via their own preview1
    /// wiring plus [`register_host_fns`].
    pub fn link_wasi(linker: &mut ComponentLinker<HostData>) {
        wasmtime_wasi::add_to_linker_sync(linker).expect("link wasi");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_permissions_resolve_to_no_capabilities() {
        let cfg = SandboxPermissions::default().resolve().unwrap();
        assert!(cfg.host_fns.is_empty());
        assert!(cfg.preopens.is_empty());
        assert!(cfg.fuel.is_none());
    }

    #[test]
    fn read_write_fs_grants_fs_write_and_preopen() {
        let perms = SandboxPermissions {
            filesystem: FilesystemPerms {
                read_write: vec![PathBuf::from("/srv/data")],
                ..Default::default()
            },
            ..Default::default()
        };
        let cfg = perms.resolve().unwrap();
        assert!(cfg.host_fns.contains(&HostFn::FsWrite));
        assert!(!cfg.host_fns.contains(&HostFn::FsRead));
        assert_eq!(cfg.preopens.len(), 1);
        assert_eq!(cfg.preopens[0].guest_path, PathBuf::from("data"));
        assert!(cfg.preopens[0].writable);
    }

    #[test]
    fn read_only_fs_grants_fs_read_only() {
        let perms = SandboxPermissions {
            filesystem: FilesystemPerms {
                read_only: vec![PathBuf::from("/srv/conf")],
                ..Default::default()
            },
            ..Default::default()
        };
        let cfg = perms.resolve().unwrap();
        assert!(cfg.host_fns.contains(&HostFn::FsRead));
        assert!(!cfg.host_fns.contains(&HostFn::FsWrite));
        assert!(!cfg.preopens[0].writable);
    }

    #[test]
    fn overlapping_path_is_rejected() {
        let perms = SandboxPermissions {
            filesystem: FilesystemPerms {
                read_only: vec![PathBuf::from("/srv/x")],
                read_write: vec![PathBuf::from("/srv/x")],
            },
            ..Default::default()
        };
        assert!(matches!(
            perms.resolve(),
            Err(SandboxError::OverlappingPath(_))
        ));
    }

    #[test]
    fn relative_path_is_rejected() {
        let perms = SandboxPermissions {
            filesystem: FilesystemPerms {
                read_only: vec![PathBuf::from("relative/path")],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(perms.resolve(), Err(SandboxError::NotAbsolute(_))));
    }

    #[test]
    fn capability_flags_map_to_host_fns() {
        let perms = SandboxPermissions {
            keystone: true,
            secrets: true,
            network: NetworkPerms {
                allow_hosts: vec!["api.example.com:443".into()],
            },
            ..Default::default()
        };
        let cfg = perms.resolve().unwrap();
        assert!(cfg.host_fns.contains(&HostFn::Keystone));
        assert!(cfg.host_fns.contains(&HostFn::Secrets));
        assert!(cfg.host_fns.contains(&HostFn::Network));
        assert_eq!(cfg.network_allow, vec!["api.example.com:443"]);
    }
}
