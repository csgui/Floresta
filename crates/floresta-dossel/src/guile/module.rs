// SPDX-License-Identifier: MIT OR Apache-2.0

//! Registration of the `(floresta node)` Scheme module.
//!
//! # Shape
//!
//! Rust registers *primitives* under names prefixed with `%`. The user-facing
//! procedures — currently just `get-block-height` — are defined in Scheme, in
//! `scheme/node.scm`, on top of those primitives.
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
