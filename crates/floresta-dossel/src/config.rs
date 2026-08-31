// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runtime configuration surface exposed as `(get-config)` / `(set-config!)`.
//!
//! # Design
//!
//! The set of keys is fixed and closed. A key that is not in [`ConfigKey`]
//! cannot be read or written, and there is deliberately no escape hatch. In
//! particular **no consensus parameter appears here** — not as a writable key,
//! not as a read-only key, not at all.
//!
//! Each key carries two independent properties:
//!
//! * [`Access`] — whether writing is meaningful at all. `network`, `datadir`
//!   and `version` are `ReadOnly` by definition; rewriting them at runtime is
//!   not a thing a node can do.
//! * A *binding* — whether this build of Floresta actually has something behind
//!   the key. Bound keys delegate to a [`ConfigBackend`] supplied by the
//!   embedder. Unbound keys report why they cannot answer.
//!
//! Keeping these separate matters. `max-peers` is conceptually writable but is
//! unbound in Floresta today, because the peer limit lives in
//! `NodeContext::MAX_OUTGOING_PEERS`, an associated *const* resolved at compile
//! time. Reporting that honestly is better than accepting the write and
//! silently doing nothing, which is what a plain in-memory config cell would
//! do.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::RwLock;

use crate::error::ApiError;
use crate::error::ApiResult;

/// The largest value accepted for `max-peers`.
pub const MAX_PEERS_LIMIT: u32 = 1024;

/// A configuration key. This list is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConfigKey {
    MaxPeers,
    MempoolMaxSizeMb,
    FeeFilterRate,
    LogLevel,
    BanThreshold,
    Network,
    Datadir,
    Version,
}

impl ConfigKey {
    /// Every key, in a stable order, for `(list-config)`.
    pub const ALL: [Self; 8] = [
        Self::MaxPeers,
        Self::MempoolMaxSizeMb,
        Self::FeeFilterRate,
        Self::LogLevel,
        Self::BanThreshold,
        Self::Network,
        Self::Datadir,
        Self::Version,
    ];

    /// The Scheme symbol naming this key.
    pub const fn as_symbol(self) -> &'static str {
        match self {
            Self::MaxPeers => "max-peers",
            Self::MempoolMaxSizeMb => "mempool-max-size-mb",
            Self::FeeFilterRate => "fee-filter-rate",
            Self::LogLevel => "log-level",
            Self::BanThreshold => "ban-threshold",
            Self::Network => "network",
            Self::Datadir => "datadir",
            Self::Version => "version",
        }
    }

    /// Parse a Scheme symbol into a key.
    pub fn from_symbol(symbol: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_symbol() == symbol)
    }

    /// Whether writing this key is meaningful, independent of whether this
    /// build has anything behind it.
    pub const fn access(self) -> Access {
        match self {
            Self::Network | Self::Datadir | Self::Version => Access::ReadOnly,
            Self::MaxPeers
            | Self::MempoolMaxSizeMb
            | Self::FeeFilterRate
            | Self::LogLevel
            | Self::BanThreshold => Access::ReadWrite,
        }
    }

    /// Validate and normalize a value being written to this key.
    fn validate(self, value: ConfigValue) -> ApiResult<ConfigValue> {
        let type_error = |expected: &str| {
            Err(ApiError::InvalidArgument(format!(
                "{} expects {expected}, got {}",
                self.as_symbol(),
                value.type_name(),
            )))
        };

        match self {
            Self::MaxPeers => {
                let n = match value {
                    ConfigValue::Integer(n) => n,
                    _ => return type_error("an exact non-negative integer"),
                };
                let n = u32::try_from(n).map_err(|_| {
                    ApiError::InvalidArgument(format!("max-peers must fit in a u32, got {n}"))
                })?;
                if n == 0 {
                    return Err(ApiError::InvalidArgument(
                        "max-peers must be at least 1".to_owned(),
                    ));
                }
                if n > MAX_PEERS_LIMIT {
                    return Err(ApiError::InvalidArgument(format!(
                        "max-peers must be at most {MAX_PEERS_LIMIT}, got {n}"
                    )));
                }
                Ok(ConfigValue::Integer(i128::from(n)))
            }
            Self::MempoolMaxSizeMb | Self::BanThreshold => match value {
                ConfigValue::Integer(n) if n >= 0 => Ok(ConfigValue::Integer(n)),
                ConfigValue::Integer(n) => Err(ApiError::InvalidArgument(format!(
                    "{} must be non-negative, got {n}",
                    self.as_symbol()
                ))),
                _ => type_error("an exact non-negative integer"),
            },
            Self::FeeFilterRate => {
                let r = match value {
                    ConfigValue::Real(r) => r,
                    // Accept an exact integer where a rate is wanted; `1` for
                    // `1.0` is the natural thing to type at a REPL.
                    ConfigValue::Integer(n) => n as f64,
                    _ => return type_error("a real number"),
                };
                if !r.is_finite() || r < 0.0 {
                    return Err(ApiError::InvalidArgument(format!(
                        "fee-filter-rate must be a finite, non-negative number, got {r}"
                    )));
                }
                Ok(ConfigValue::Real(r))
            }
            Self::LogLevel => {
                match &value {
                    // A bare level symbol: validated here against the
                    // well-known levels.
                    ConfigValue::Symbol(s) => {
                        if !LOG_LEVELS.contains(&s.as_str()) {
                            return Err(ApiError::InvalidArgument(format!(
                                "log-level must be one of {}, got '{s}",
                                LOG_LEVELS.join(", ")
                            )));
                        }
                        Ok(value)
                    }
                    // A full filter directive such as "info,wire=debug":
                    // parsed by the backend (EnvFilter), not here.
                    ConfigValue::Str(_) => Ok(value),
                    _ => type_error("a level symbol or a filter string"),
                }
            }
            Self::Network | Self::Datadir | Self::Version => Err(ApiError::InvalidArgument(
                format!("{} is read-only", self.as_symbol()),
            )),
        }
    }
}

/// The log levels `(set-config! 'log-level ...)` accepts.
const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

impl fmt::Display for ConfigKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_symbol())
    }
}

/// Whether a key may be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// A configuration value, in the shapes Scheme can produce and consume.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    /// An exact integer. Held as `i128` so every u32/u64/i64 key fits without
    /// a lossy narrowing at the FFI boundary.
    Integer(i128),
    Real(f64),
    Boolean(bool),
    Str(String),
    Symbol(String),
}

impl ConfigValue {
    /// The name used in type-mismatch messages.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "an exact integer",
            Self::Real(_) => "a real number",
            Self::Boolean(_) => "a boolean",
            Self::Str(_) => "a string",
            Self::Symbol(_) => "a symbol",
        }
    }
}

impl fmt::Display for ConfigValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(n) => write!(f, "{n}"),
            Self::Real(r) => write!(f, "{r}"),
            Self::Boolean(true) => f.write_str("#t"),
            Self::Boolean(false) => f.write_str("#f"),
            Self::Str(s) => write!(f, "{s:?}"),
            Self::Symbol(s) => write!(f, "'{s}"),
        }
    }
}

/// Something that can actually read, and possibly write, a configuration key
/// in the running node.
///
/// Implementations are called from Guile threads and must be `Send + Sync`.
/// They must not block for long: a REPL client is waiting.
pub trait ConfigBackend: Send + Sync + 'static {
    /// Read the current value from the node.
    fn get(&self) -> ApiResult<ConfigValue>;

    /// Write a new value into the node.
    ///
    /// The value has already been validated against the key's type and range,
    /// and the key has already been checked to be [`Access::ReadWrite`]. The
    /// default refuses, which is the right answer for a backend that can read a
    /// live value but has no way to change it.
    fn set(&self, _value: ConfigValue) -> ApiResult<()> {
        Err(ApiError::Node(
            "this key is readable but cannot be changed at runtime".to_owned(),
        ))
    }
}

/// A [`ConfigBackend`] over a value that is fixed for the lifetime of the node.
///
/// Used for `network`, `datadir` and `version`.
pub struct StaticBackend(ConfigValue);

impl StaticBackend {
    pub const fn new(value: ConfigValue) -> Self {
        Self(value)
    }
}

impl ConfigBackend for StaticBackend {
    fn get(&self) -> ApiResult<ConfigValue> {
        Ok(self.0.clone())
    }
}

/// A [`ConfigBackend`] built from a pair of closures.
pub struct FnBackend<G, S> {
    get: G,
    set: Option<S>,
}

impl<G> FnBackend<G, fn(ConfigValue) -> ApiResult<()>>
where
    G: Fn() -> ApiResult<ConfigValue> + Send + Sync + 'static,
{
    /// A backend that can read the live value but not change it.
    pub const fn read_only(get: G) -> Self {
        Self { get, set: None }
    }
}

impl<G, S> FnBackend<G, S>
where
    G: Fn() -> ApiResult<ConfigValue> + Send + Sync + 'static,
    S: Fn(ConfigValue) -> ApiResult<()> + Send + Sync + 'static,
{
    /// A backend that can both read and write the live value.
    pub const fn read_write(get: G, set: S) -> Self {
        Self {
            get,
            set: Some(set),
        }
    }
}

impl<G, S> ConfigBackend for FnBackend<G, S>
where
    G: Fn() -> ApiResult<ConfigValue> + Send + Sync + 'static,
    S: Fn(ConfigValue) -> ApiResult<()> + Send + Sync + 'static,
{
    fn get(&self) -> ApiResult<ConfigValue> {
        (self.get)()
    }

    fn set(&self, value: ConfigValue) -> ApiResult<()> {
        match &self.set {
            Some(set) => set(value),
            None => Err(ApiError::Node(
                "this key is readable but cannot be changed at runtime".to_owned(),
            )),
        }
    }
}

/// Why a key has no backend in this build.
struct Unbound {
    reason: &'static str,
}

enum Binding {
    Bound(Arc<dyn ConfigBackend>),
    Unbound(Unbound),
}

/// The live configuration surface.
///
/// Cheap to clone; all clones share one table.
#[derive(Clone)]
pub struct RuntimeConfig {
    bindings: Arc<RwLock<HashMap<ConfigKey, Binding>>>,
}

impl RuntimeConfig {
    /// Start from a table in which every key is unbound, with a default
    /// explanation. The embedder then binds the ones it can support.
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(ConfigKey::ALL.len());
        for key in ConfigKey::ALL {
            bindings.insert(key, Binding::Unbound(Unbound {
                reason: default_unbound_reason(key),
            }));
        }

        Self {
            bindings: Arc::new(RwLock::new(bindings)),
        }
    }

    /// Attach a backend to `key`, replacing any previous binding.
    pub fn bind(&self, key: ConfigKey, backend: Arc<dyn ConfigBackend>) {
        self.write_table()
            .insert(key, Binding::Bound(backend));
    }

    /// Read a key.
    pub fn get(&self, key: ConfigKey) -> ApiResult<ConfigValue> {
        match self.read_table().get(&key) {
            Some(Binding::Bound(backend)) => backend.get(),
            Some(Binding::Unbound(u)) => Err(ApiError::Unsupported {
                capability: key.as_symbol(),
                reason: u.reason,
            }),
            // Unreachable in practice: `new` populates every key and `bind`
            // only ever replaces. Handled rather than panicking because a
            // panic here would cross the FFI boundary.
            None => Err(ApiError::NotFound(format!("unknown config key {key}"))),
        }
    }

    /// Write a key.
    ///
    /// Checks are ordered so the user gets the most fundamental objection
    /// first: read-only before unbound, and both before validation. Telling
    /// someone their value is out of range for a key they were never allowed to
    /// write would be actively misleading.
    pub fn set(&self, key: ConfigKey, value: ConfigValue) -> ApiResult<()> {
        if key.access() == Access::ReadOnly {
            return Err(ApiError::InvalidArgument(format!("{key} is read-only")));
        }

        let backend = match self.read_table().get(&key) {
            Some(Binding::Bound(backend)) => Arc::clone(backend),
            Some(Binding::Unbound(u)) => {
                return Err(ApiError::Unsupported {
                    capability: key.as_symbol(),
                    reason: u.reason,
                });
            }
            None => return Err(ApiError::NotFound(format!("unknown config key {key}"))),
        };

        // The guard is dropped before calling into the backend: a backend may
        // block on the node, and holding the table lock across that would stall
        // every other REPL session.
        backend.set(key.validate(value)?)
    }

    /// Whether `key` has a backend in this build.
    pub fn is_bound(&self, key: ConfigKey) -> bool {
        matches!(self.read_table().get(&key), Some(Binding::Bound(_)))
    }

    fn read_table(&self) -> std::sync::RwLockReadGuard<'_, HashMap<ConfigKey, Binding>> {
        // A poisoned lock means some other thread panicked mid-update. The
        // table only ever holds `Arc`s and static strings, so no invariant can
        // be half-broken; recovering is strictly better than propagating a
        // panic across the FFI boundary.
        self.bindings.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write_table(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<ConfigKey, Binding>> {
        self.bindings.write().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Why each key is unbound before the embedder binds anything.
///
/// These strings are what a REPL user sees, so they name the concrete reason
/// rather than saying "unsupported".
const fn default_unbound_reason(key: ConfigKey) -> &'static str {
    match key {
        ConfigKey::MaxPeers => {
            "Floresta's outbound peer limit is NodeContext::MAX_OUTGOING_PEERS, an associated \
             const fixed at compile time, so it cannot be changed in a running node"
        }
        ConfigKey::MempoolMaxSizeMb => {
            "Floresta fixes the mempool size limit when the node task builds its mempool and \
             exposes no accessor for it afterwards"
        }
        ConfigKey::FeeFilterRate => "Floresta does not implement a fee filter rate",
        ConfigKey::LogLevel => "no log-level backend was installed by the embedder",
        ConfigKey::BanThreshold => "no ban-threshold backend was installed by the embedder",
        ConfigKey::Network => "no network backend was installed by the embedder",
        ConfigKey::Datadir => "no datadir backend was installed by the embedder",
        ConfigKey::Version => "no version backend was installed by the embedder",
    }
}
