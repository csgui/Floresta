// SPDX-License-Identifier: MIT OR Apache-2.0

//! Registration of the `(floresta node)` Scheme module.
//!
//! # Shape
//!
//! Rust registers *primitives* under names prefixed with `%`. The user-facing
//! procedures — `get-block-height`, `rpc-call`, `get-config`, `set-config!` —
//! are defined in Scheme, in `scheme/node.scm`, on top of those primitives.
//!
//! That split is deliberate. Argument checking and docstrings are far cleaner
//! to express in Scheme than in `extern "C"` functions, and every line
//! written there is a line that cannot cause undefined behaviour. Rust
//! handles only what it must: crossing into the node and converting values.
//!
//! `scheme/repl.scm` loads first: REPL presentation (colors, banner, prompt)
//! with no dependency on the `%` primitives. `scheme/node.scm` loads after
//! primitive registration, since its procedures call them. There is no
//! prelude — a reusable standard library, generic to Dossel — yet.
//!
//! Both are embedded in the binary with `include_str!`, so there is no load
//! path to configure and no files to install alongside `florestad`.
//!
//! # What is deliberately absent
//!
//! No consensus parameter is reachable from this module. Not as a writable
//! value, not as a read-only one, not indirectly through `rpc-call`. Block
//! validation, difficulty adjustment and signature verification have no
//! representation here at all.

use std::os::raw::c_void;
use std::panic::AssertUnwindSafe;

use super::bindings;
use super::bridge;
use super::safe;
use super::safe::Scm;
use crate::config::ConfigKey;
use crate::config::ConfigValue;
use crate::error::ApiError;
use crate::error::ApiResult;

/// REPL presentation: colors, banner, prompt. No dependency on the `%`
/// primitives, so it is safe to evaluate before they exist.
const REPL: &str = include_str!("../../scheme/repl.scm");

/// The Scheme source layered on top of the primitives registered here.
const NODE: &str = include_str!("../../scheme/node.scm");

/// The module's name, as `(floresta node)`.
const MODULE_NAME: &str = "floresta node";

// ---------------------------------------------------------------------------
// The guard every primitive runs inside
// ---------------------------------------------------------------------------

/// Run a primitive's body, converting both failures and panics into Scheme
/// errors.
///
/// Two things have to be true on the way out of an `extern "C"` frame:
///
/// * **No Rust panic may escape.** Unwinding out of `extern "C"` aborts the
///   process, which would let a typo in a REPL session kill the node. The
///   `catch_unwind` here is what makes acceptance criterion 7 hold.
/// * **No Rust destructor may be live when we `longjmp`.** The error message is
///   converted into a Scheme string and the owning `String` dropped *before*
///   [`safe::throw_scm`] is called.
fn guard<F>(subr: &'static str, body: F) -> bindings::SCM
where
    F: FnOnce() -> ApiResult<Scm>,
{
    // `AssertUnwindSafe` is sound here because the only state shared with the
    // rest of the process is behind the node's own locks, and a panic in a
    // conversion routine cannot leave those observably broken — Dossel never
    // holds a node lock across a panic-capable section.
    let outcome = std::panic::catch_unwind(AssertUnwindSafe(body));

    let message = match outcome {
        Ok(Ok(value)) => return value.raw(),
        Ok(Err(err)) => err.to_string(),
        Err(payload) => format!("internal error in Dossel: {}", panic_text(&payload)),
    };

    // Report the name the operator typed, not the primitive behind it: they
    // called `(get-config ...)`, and `%get-config` would be a puzzle.
    let scm_subr = Scm::from_str_lossy(subr.trim_start_matches('%'));

    // Convert, then drop, then jump. Order matters; see `safe::throw`.
    let scm_message = Scm::from_str_lossy(&message);
    drop(message);
    safe::throw(scm_subr, scm_message)
}

/// Best-effort rendering of a panic payload.
fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_owned())
}

// ---------------------------------------------------------------------------
// Argument extraction
// ---------------------------------------------------------------------------

/// Read a configuration key argument, accepting a symbol or a string.
fn arg_config_key(value: Scm) -> ApiResult<ConfigKey> {
    let name = value
        .as_symbol_name()
        .or_else(|| value.as_string())
        .ok_or_else(|| {
            ApiError::InvalidArgument(format!(
                "config key must be a symbol, got {}",
                safe::write_to_string(value)
            ))
        })?;

    ConfigKey::from_symbol(&name).ok_or_else(|| {
        let known = ConfigKey::ALL
            .iter()
            .map(|k| k.as_symbol())
            .collect::<Vec<_>>()
            .join(", ");
        ApiError::NotFound(format!("unknown config key '{name}; known keys are {known}"))
    })
}

/// Convert an arbitrary Scheme value into a [`ConfigValue`].
fn arg_config_value(value: Scm) -> ApiResult<ConfigValue> {
    if let Some(n) = value.as_i64() {
        return Ok(ConfigValue::Integer(i128::from(n)));
    }
    if let Some(s) = value.as_symbol_name() {
        return Ok(ConfigValue::Symbol(s));
    }
    if let Some(s) = value.as_string() {
        return Ok(ConfigValue::Str(s));
    }
    if value.is_real() {
        if let Some(x) = value.as_f64() {
            return Ok(ConfigValue::Real(x));
        }
    }

    // Booleans are last: every non-`#f` value is "true" in Scheme, so testing
    // this first would swallow strings, symbols and numbers alike.
    Ok(ConfigValue::Boolean(value.is_true()))
}

// ---------------------------------------------------------------------------
// Result conversion
// ---------------------------------------------------------------------------

impl ConfigValue {
    /// Render as the Scheme value `(get-config)` should return.
    fn to_scm(&self) -> Scm {
        match self {
            Self::Integer(n) => Scm::from_i128(*n),
            Self::Real(x) => Scm::from_f64(*x),
            Self::Boolean(b) => Scm::from_bool(*b),
            Self::Str(s) => Scm::from_str_lossy(s),
            Self::Symbol(s) => Scm::symbol(s),
        }
    }
}

/// Convert a JSON value into Scheme.
///
/// Objects become association lists with symbol keys, so `assoc-ref` works on
/// them the same way it would on any other record. `null` becomes the symbol
/// `'null` rather than `#f`, which would be indistinguishable from a JSON
/// `false`.
fn json_to_scm(value: &serde_json::Value) -> Scm {
    match value {
        serde_json::Value::Null => Scm::symbol("null"),
        serde_json::Value::Bool(b) => Scm::from_bool(*b),
        serde_json::Value::Number(n) => n.as_i64().map_or_else(
            || {
                n.as_u64().map_or_else(
                    || Scm::from_f64(n.as_f64().unwrap_or(f64::NAN)),
                    Scm::from_u64,
                )
            },
            Scm::from_i64,
        ),
        serde_json::Value::String(s) => Scm::from_str_lossy(s),
        serde_json::Value::Array(items) => {
            Scm::list(items.iter().map(json_to_scm).collect::<Vec<_>>())
        }
        serde_json::Value::Object(map) => map
            .iter()
            .rev()
            .fold(Scm::eol(), |tail, (key, value)| {
                Scm::acons(Scm::symbol(key), json_to_scm(value), tail)
            }),
    }
}

/// Convert a Scheme value into JSON, for `rpc-call` parameters.
///
/// Symbols become strings, which is what an RPC method expects when a user
/// types `'verbose` rather than `"verbose"`.
fn scm_to_json(value: Scm) -> serde_json::Value {
    if let Some(n) = value.as_i64() {
        return serde_json::Value::from(n);
    }
    if let Some(s) = value.as_string() {
        return serde_json::Value::String(s);
    }
    if let Some(s) = value.as_symbol_name() {
        return serde_json::Value::String(s);
    }
    if value.is_real() {
        if let Some(x) = value.as_f64() {
            return serde_json::Number::from_f64(x)
                .map_or(serde_json::Value::Null, serde_json::Value::Number);
        }
    }
    if value.is_pair() || value.is_null() {
        return serde_json::Value::Array(
            value.list_to_vec().into_iter().map(scm_to_json).collect(),
        );
    }

    // Anything left is treated as a boolean, which is how Scheme itself reads
    // an arbitrary value in a conditional: only `#f` is false.
    serde_json::Value::Bool(value.is_true())
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------
//
// Each is an `extern "C"` function whose arity must match its registration in
// `SUBRS` below. The macro at the bottom keeps the two together.

unsafe extern "C" fn p_get_block_height() -> bindings::SCM {
    guard("%get-block-height", || {
        bridge::call(|api| api.get_block_height()).map(Scm::from_u32)
    })
}

unsafe extern "C" fn p_rpc_call(method: bindings::SCM, params: bindings::SCM) -> bindings::SCM {
    guard("%rpc-call", || {
        let method_scm = Scm::from_raw(method);
        let method = method_scm
            .as_string()
            .or_else(|| method_scm.as_symbol_name())
            .ok_or_else(|| {
                ApiError::InvalidArgument(format!(
                    "method must be a string or symbol, got {}",
                    safe::write_to_string(method_scm)
                ))
            })?;

        let params_scm = Scm::from_raw(params);
        // `'()` is the natural way to write "no parameters", and it is neither
        // undefined nor a pair, so it needs its own arm.
        let params = if params_scm.is_undefined() || params_scm.is_null() {
            Vec::new()
        } else if params_scm.is_pair() {
            params_scm.list_to_vec().into_iter().map(scm_to_json).collect()
        } else {
            return Err(ApiError::InvalidArgument(format!(
                "params must be a list, got {}",
                safe::write_to_string(params_scm)
            )));
        };

        bridge::call(|api| api.rpc_call(&method, params)).map(|v| json_to_scm(&v))
    })
}

unsafe extern "C" fn p_get_config(key: bindings::SCM) -> bindings::SCM {
    guard("%get-config", || {
        let key = arg_config_key(Scm::from_raw(key))?;
        bridge::config()?.get(key).map(|v| v.to_scm())
    })
}

unsafe extern "C" fn p_set_config(key: bindings::SCM, value: bindings::SCM) -> bindings::SCM {
    guard("%set-config!", || {
        let key = arg_config_key(Scm::from_raw(key))?;
        let value = arg_config_value(Scm::from_raw(value))?;
        bridge::config()?.set(key, value)?;
        Ok(Scm::symbol("ok"))
    })
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register every primitive, pairing each name with its arity and handler.
///
/// The macro exists so that a name, its declared arity and the function that
/// implements it appear on one line and cannot drift apart. A mismatch here is
/// a type-confused indirect call, which is why [`safe::define_gsubr`] is
/// `unsafe`.
macro_rules! register_subrs {
    ($( $name:literal => ($req:expr, $opt:expr, $rest:expr, $func:path) ),+ $(,)?) => {
        $(
            // SAFETY: `$func` is an `extern "C" fn` taking exactly
            // `$req + $opt` `SCM` arguments (plus a rest list when `$rest` is
            // 1) and returning `SCM`, matching the arity declared alongside it.
            // Verified by inspection at each call site below.
            unsafe {
                safe::define_gsubr(
                    $name,
                    $req,
                    $opt,
                    $rest,
                    $func as *const () as *mut c_void,
                );
            }
            safe::export($name);
        )+
    };
}

/// Register every primitive into the current module.
///
/// # Panics
///
/// Called from inside a [`safe::catch`] body, so it must not panic. Everything
/// it does is registration, which cannot fail short of allocation failure.
fn register_primitives() {
    register_subrs! {
        "%get-block-height" => (0, 0, 0, p_get_block_height),
        "%rpc-call"         => (1, 1, 0, p_rpc_call),
        "%get-config"       => (1, 0, 0, p_get_config),
        "%set-config!"      => (2, 0, 0, p_set_config),
    }
}

/// Define `(floresta node)`: create the module, evaluate the REPL
/// presentation Scheme, register the primitives, then evaluate the
/// node-specific Scheme on top.
///
/// # Ordering
///
/// The four steps are ordered by dependency and cannot be rearranged.
/// `define-module` both creates the module and makes it the current one, which
/// is what causes `scm_c_define_gsubr` to define into it rather than into
/// `(guile-user)`. `REPL` runs next since it has no dependency on the `%`
/// primitives. `NODE` runs last because its procedures call them, and Guile's
/// compiler warns about references to bindings that do not yet exist.
///
/// The current module is restored at the end so that the Dossel thread is left
/// in `(guile-user)`, the module a fresh REPL client starts in.
///
/// Must be called from a thread in Guile mode; see [`safe`].
pub(crate) fn define() -> Result<(), safe::GuileException> {
    safe::eval_string(&format!("(define-module ({MODULE_NAME}))"))?;

    safe::eval_string(REPL)?;

    // Registration allocates Scheme objects, so it runs inside a catch like
    // any other Guile call, even though it has no expected failure mode.
    safe::catch(|| {
        register_primitives();
        Scm::unspecified()
    })?;

    safe::eval_string(NODE)?;

    safe::eval_string("(set-current-module (resolve-module '(guile-user)))")?;

    Ok(())
}

/// Import `(floresta node)` into `(guile-user)`, so a REPL client can call the
/// procedures without a `use-modules` of their own.
pub(crate) fn import_into_user_module() -> Result<(), safe::GuileException> {
    safe::eval_string(&format!(
        "(module-use! (resolve-module '(guile-user)) (resolve-interface '({MODULE_NAME})))"
    ))
    .map(|_| ())
}
