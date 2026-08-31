// SPDX-License-Identifier: MIT OR Apache-2.0

//! Safe(r) wrappers over the raw libguile bindings.
//!
//! # The one invariant
//!
//! **Every function in this module must be called from a thread that is in
//! Guile mode** — that is, from inside [`with_guile`], or from a thread Guile
//! itself created (such as the ones `spawn-server` makes for REPL clients).
//! Calling any of this from an arbitrary thread is undefined behaviour, because
//! libguile will not have registered that thread with the garbage collector.
//!
//! This is stated once here rather than repeated on every function. The module
//! is `pub(crate)` and every path into it originates in [`super::runtime`],
//! which establishes the invariant before doing anything else.
//!
//! # The two hazards
//!
//! *Guile errors are `longjmp`s.* When Scheme code raises, libguile performs a
//! non-local exit. If that jump crosses a Rust frame, every destructor on that
//! frame is skipped — leaks at best, and unsound for anything holding a lock.
//! Two rules follow. First, any call that can evaluate user code goes through
//! [`catch_raw`], which puts an `scm_c_catch` between the Scheme and the Rust.
//! Second, [`throw`] is the only place Dossel raises a Scheme error, and it is
//! written so that no Rust destructor is live at the moment it jumps.
//!
//! *The collector moves and frees.* Any `SCM` that Rust holds across a call
//! that might allocate must be reachable by the collector. BDW-GC scans thread
//! stacks conservatively, so an `SCM` in a local is fine; one stored behind a
//! heap pointer is not, and needs [`Protected`].
//!
//! # Minimal by design
//!
//! `Scm` exposes only the value shapes this crate's current primitives
//! (`get-block-height`, `rpc-call`) and its own catch/throw machinery
//! actually need — see `guile/module.rs`. Association lists (`alist`) and
//! bignum-to-decimal fallback (`from_i128`) are not among them, since nothing
//! currently produces those shapes; add them back when a primitive needs
//! them, so every method here stays backed by a real caller.

use std::ffi::CString;
use std::os::raw::c_char;
use std::os::raw::c_void;

use super::bindings;

/// A Scheme value.
///
/// `Copy` because the underlying `SCM` is just a tagged word; ownership is the
/// collector's business, not ours. Deliberately **not** `Send` or `Sync`: an
/// `SCM` is only meaningful on a thread in Guile mode, and the raw pointer it
/// wraps makes that fall out of the auto-trait rules for free.
#[derive(Clone, Copy)]
pub(crate) struct Scm(bindings::SCM);

impl Scm {
    /// Wrap a raw value coming back from libguile.
    pub(crate) const fn from_raw(raw: bindings::SCM) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> bindings::SCM {
        self.0
    }

    // --- Constants -------------------------------------------------------

    pub(crate) fn bool_t() -> Self {
        // SAFETY: reads a compile-time constant bit pattern out of the shim.
        // Allocates nothing and cannot raise.
        Self(unsafe { bindings::dossel_bool_t() })
    }

    pub(crate) fn bool_f() -> Self {
        // SAFETY: as `bool_t`.
        Self(unsafe { bindings::dossel_bool_f() })
    }

    pub(crate) fn unspecified() -> Self {
        // SAFETY: as `bool_t`.
        Self(unsafe { bindings::dossel_unspecified() })
    }

    pub(crate) fn eol() -> Self {
        // SAFETY: as `bool_t`.
        Self(unsafe { bindings::dossel_eol() })
    }

    // --- From Rust -------------------------------------------------------

    pub(crate) fn from_bool(b: bool) -> Self {
        // SAFETY: `dossel_from_bool` maps any int to one of two constants.
        Self(unsafe { bindings::dossel_from_bool(i32::from(b)) })
    }

    pub(crate) fn from_u32(n: u32) -> Self {
        // SAFETY: `scm_from_uint32` accepts the whole u32 range. It may
        // allocate a bignum, which is fine on a Guile-mode thread.
        Self(unsafe { bindings::scm_from_uint32(n) })
    }

    pub(crate) fn from_u64(n: u64) -> Self {
        // SAFETY: as `from_u32`.
        Self(unsafe { bindings::scm_from_uint64(n) })
    }

    pub(crate) fn from_i64(n: i64) -> Self {
        // SAFETY: as `from_u32`.
        Self(unsafe { bindings::scm_from_int64(n) })
    }

    pub(crate) fn from_f64(x: f64) -> Self {
        // SAFETY: every f64 is representable as a Scheme real.
        Self(unsafe { bindings::scm_from_double(x) })
    }

    /// Build a Scheme string.
    ///
    /// A Rust `String` may contain interior NUL bytes, which cannot survive the
    /// C boundary. Rather than fail — these strings are error messages on their
    /// way to a human — NULs are replaced with U+FFFD.
    pub(crate) fn from_str_lossy(s: &str) -> Self {
        let c = match CString::new(s) {
            Ok(c) => c,
            Err(_) => {
                let cleaned: String = s.replace('\0', "\u{fffd}");
                CString::new(cleaned).unwrap_or_else(|_| {
                    // Unreachable: the replacement removed every NUL.
                    CString::default()
                })
            }
        };

        // SAFETY: `c` is a valid NUL-terminated C string that outlives the
        // call, and `scm_from_utf8_string` copies its contents into a
        // GC-managed Scheme string.
        Self(unsafe { bindings::scm_from_utf8_string(c.as_ptr()) })
    }

    /// Build a Scheme symbol. NUL handling as [`Scm::from_str_lossy`].
    pub(crate) fn symbol(s: &str) -> Self {
        let c = CString::new(s).unwrap_or_else(|_| {
            CString::new(s.replace('\0', "\u{fffd}")).unwrap_or_default()
        });

        // SAFETY: as `from_str_lossy`; `scm_from_utf8_symbol` interns a copy.
        Self(unsafe { bindings::scm_from_utf8_symbol(c.as_ptr()) })
    }

    // --- Predicates ------------------------------------------------------

    pub(crate) fn is_true(self) -> bool {
        // SAFETY: the predicate shims only inspect the tag bits of the value.
        unsafe { bindings::dossel_is_true(self.0) != 0 }
    }

    fn is_string(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_string(self.0) != 0 }
    }

    fn is_symbol(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_symbol(self.0) != 0 }
    }

    fn is_exact_integer(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_exact_integer(self.0) != 0 }
    }

    pub(crate) fn is_real(self) -> bool {
        // SAFETY: `scm_real_p` is a predicate over any value.
        unsafe { bindings::dossel_is_real(self.0) != 0 }
    }

    pub(crate) fn is_pair(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_pair(self.0) != 0 }
    }

    /// Whether this is the empty list, `'()`.
    ///
    /// Distinct from `is_pair`: the empty list is not a pair, but it *is* a
    /// list, and it is how a caller writes "no arguments".
    pub(crate) fn is_null(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_null(self.0) != 0 }
    }

    /// Whether this is the "argument was not supplied" marker Guile passes for
    /// optional parameters a caller left out.
    pub(crate) fn is_undefined(self) -> bool {
        // SAFETY: as `is_true`.
        unsafe { bindings::dossel_is_undefined(self.0) != 0 }
    }

    // --- To Rust ---------------------------------------------------------

    /// Copy a Scheme string out into Rust. Returns `None` if this is not a
    /// string.
    pub(crate) fn as_string(self) -> Option<String> {
        if !self.is_string() {
            return None;
        }

        // SAFETY: `self` is a string, so `scm_to_utf8_string` returns a
        // freshly `malloc`ed, NUL-terminated buffer that becomes ours to free.
        let raw = unsafe { bindings::scm_to_utf8_string(self.0) };
        if raw.is_null() {
            return None;
        }

        // SAFETY: `raw` is non-null and NUL-terminated, as documented for
        // `scm_to_utf8_string`.
        let owned = unsafe { std::ffi::CStr::from_ptr(raw) }
            .to_string_lossy()
            .into_owned();

        // SAFETY: `raw` came from libguile's `malloc` and has not been freed;
        // `owned` above holds an independent copy, so freeing here is correct.
        unsafe { libc::free(raw.cast::<c_void>()) };

        Some(owned)
    }

    /// The name of a symbol, without its quote.
    pub(crate) fn as_symbol_name(self) -> Option<String> {
        if !self.is_symbol() {
            return None;
        }

        // SAFETY: `self` is a symbol, which is exactly `scm_symbol_to_string`'s
        // domain; it cannot raise here.
        let s = unsafe { bindings::scm_symbol_to_string(self.0) };
        Self(s).as_string()
    }

    /// Read an exact integer as an `i64`.
    ///
    /// Returns `None` for non-integers and for bignums outside `i64`, both of
    /// which would otherwise make `scm_to_int64` raise.
    pub(crate) fn as_i64(self) -> Option<i64> {
        if !self.is_exact_integer() {
            return None;
        }

        // A bignum outside i64 would make `scm_to_int64` raise, so bound it in
        // Scheme first, where a comparison cannot fail on an exact integer.
        if !self.fits_in_i64() {
            return None;
        }

        // SAFETY: `self` is an exact integer known to be within i64, so the
        // conversion is total and cannot raise.
        Some(unsafe { bindings::scm_to_int64(self.0) })
    }

    /// Whether an exact integer is within `i64`, tested without risking a raise.
    fn fits_in_i64(self) -> bool {
        // SAFETY: `scm_num_eq_p` and friends are not needed here; comparing via
        // `scm_less_p` on two exact integers cannot raise. The bounds are built
        // from `i64` extremes.
        unsafe {
            let min = bindings::scm_from_int64(i64::MIN);
            let max = bindings::scm_from_int64(i64::MAX);
            let too_small = bindings::dossel_is_true(bindings::scm_less_p(self.0, min)) != 0;
            let too_large = bindings::dossel_is_true(bindings::scm_less_p(max, self.0)) != 0;
            !too_small && !too_large
        }
    }

    /// Read any real number as an `f64`.
    pub(crate) fn as_f64(self) -> Option<f64> {
        if !self.is_real() {
            return None;
        }

        // SAFETY: `self` is a real, which is `scm_to_double`'s domain.
        Some(unsafe { bindings::scm_to_double(self.0) })
    }

    // --- Pairs and lists -------------------------------------------------

    pub(crate) fn cons(car: Self, cdr: Self) -> Self {
        // SAFETY: `scm_cons` accepts any two values and allocates a pair.
        Self(unsafe { bindings::dossel_cons(car.0, cdr.0) })
    }

    pub(crate) fn car(self) -> Option<Self> {
        self.is_pair().then(|| {
            // SAFETY: guarded by `is_pair`, so `SCM_CAR` is in bounds.
            Self(unsafe { bindings::dossel_car(self.0) })
        })
    }

    pub(crate) fn cdr(self) -> Option<Self> {
        self.is_pair().then(|| {
            // SAFETY: guarded by `is_pair`, so `SCM_CDR` is in bounds.
            Self(unsafe { bindings::dossel_cdr(self.0) })
        })
    }

    /// Prepend `(key . value)` to an association list.
    pub(crate) fn acons(key: Self, value: Self, alist: Self) -> Self {
        // SAFETY: `scm_acons` accepts any three values.
        Self(unsafe { bindings::dossel_acons(key.0, value.0, alist.0) })
    }

    /// Build a proper list from `items`, preserving order.
    pub(crate) fn list<I>(items: I) -> Self
    where
        I: IntoIterator<Item = Self>,
        I::IntoIter: DoubleEndedIterator,
    {
        items
            .into_iter()
            .rev()
            .fold(Self::eol(), |tail, head| Self::cons(head, tail))
    }

    /// Walk a proper list into a `Vec`.
    ///
    /// Stops at the first non-pair tail, so an improper list yields its proper
    /// prefix rather than looping or raising.
    pub(crate) fn list_to_vec(self) -> Vec<Self> {
        let mut out = Vec::new();
        let mut cursor = self;
        while let (Some(head), Some(tail)) = (cursor.car(), cursor.cdr()) {
            out.push(head);
            cursor = tail;
        }
        out
    }
}

/// An `SCM` pinned against collection for as long as this guard lives.
///
/// Needed only for values Rust holds somewhere the collector does not scan.
/// A plain local does not need this: BDW-GC scans thread stacks conservatively.
pub(crate) struct Protected(bindings::SCM);

impl Protected {
    pub(crate) fn new(value: Scm) -> Self {
        // SAFETY: registers `value` as a GC root. Balanced by `Drop`.
        Self(unsafe { bindings::scm_gc_protect_object(value.raw()) })
    }

    pub(crate) const fn get(&self) -> Scm {
        Scm(self.0)
    }
}

impl Drop for Protected {
    fn drop(&mut self) {
        // SAFETY: `self.0` was registered by `Protected::new` and each guard
        // unprotects exactly once. Note this will not run if a Guile `longjmp`
        // skips the frame, which leaks a root rather than corrupting anything.
        unsafe {
            bindings::scm_gc_unprotect_object(self.0);
        }
    }
}

/// A Scheme exception that crossed back into Rust.
#[derive(Debug, Clone)]
pub(crate) struct GuileException {
    /// The throw key, e.g. `unbound-variable`.
    pub(crate) key: String,
    /// The throw arguments, written out.
    pub(crate) args: String,
}

impl std::fmt::Display for GuileException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.args)
    }
}

/// Shared state between [`catch_raw`] and its C trampolines.
struct CatchCtx<'a> {
    body: &'a mut dyn FnMut() -> Scm,
    /// Set by the handler when the body raised: the throw key and arguments,
    /// pinned so they survive until the caller has read them.
    raised: Option<(Protected, Protected)>,
}

/// The body trampoline handed to `scm_c_catch`.
///
/// # Panics
///
/// Must not panic: this is an `extern "C"` frame, so an unwind would abort the
/// process. Callers only ever pass closures that make libguile calls.
unsafe extern "C" fn catch_body(data: *mut c_void) -> bindings::SCM {
    // SAFETY: `data` is the `&mut CatchCtx` that `catch_raw` passed to
    // `scm_c_catch`, which is still borrowed and alive for the whole call.
    let ctx = unsafe { &mut *data.cast::<CatchCtx<'_>>() };
    (ctx.body)().raw()
}

/// The post-unwind handler handed to `scm_c_catch`.
///
/// Runs after the stack between the throw and the catch has been unwound, on
/// the frame that called `scm_c_catch` — so writing through `data` is sound.
unsafe extern "C" fn catch_handler(
    data: *mut c_void,
    tag: bindings::SCM,
    throw_args: bindings::SCM,
) -> bindings::SCM {
    // SAFETY: as `catch_body`.
    let ctx = unsafe { &mut *data.cast::<CatchCtx<'_>>() };

    // Pin both values. They are about to be stored behind a Rust pointer, where
    // the conservative stack scan would not find them.
    ctx.raised = Some((
        Protected::new(Scm::from_raw(tag)),
        Protected::new(Scm::from_raw(throw_args)),
    ));

    Scm::bool_f().raw()
}

/// Run `body`, catching any Scheme error it raises.
///
/// This is the only sanctioned way to evaluate Scheme from Rust. Everything
/// that can raise — evaluating user input, loading a file, calling a procedure —
/// must go through it, so that a `longjmp` lands in `scm_c_catch` rather than
/// tearing through Rust frames.
///
/// Returns the raw throw key and arguments on failure; use [`catch`] for a
/// rendered message.
pub(crate) fn catch_raw<F>(mut body: F) -> Result<Scm, (Protected, Protected)>
where
    F: FnMut() -> Scm,
{
    let mut ctx = CatchCtx {
        body: &mut body,
        raised: None,
    };
    let ctx_ptr = std::ptr::from_mut(&mut ctx).cast::<c_void>();

    // SAFETY: `catch_body` and `catch_handler` match the signatures
    // `scm_c_catch` expects, and `ctx_ptr` points at `ctx`, which outlives the
    // call because it is a local of this frame. `#t` as the tag catches every
    // key. The `pre_unwind_handler` is null, which `scm_c_catch` documents as
    // "no pre-unwind handler".
    let value = unsafe {
        bindings::scm_c_catch(
            Scm::bool_t().raw(),
            Some(catch_body),
            ctx_ptr,
            Some(catch_handler),
            ctx_ptr,
            None,
            std::ptr::null_mut(),
        )
    };

    match ctx.raised.take() {
        Some(raised) => Err(raised),
        None => Ok(Scm::from_raw(value)),
    }
}

/// Run `body`, rendering any Scheme error into a [`GuileException`].
pub(crate) fn catch<F>(body: F) -> Result<Scm, GuileException>
where
    F: FnMut() -> Scm,
{
    catch_raw(body).map_err(|(key, args)| GuileException {
        key: write_to_string(key.get()),
        args: write_to_string(args.get()),
    })
}

/// Render a value the way `write` would.
///
/// Printing is itself Scheme evaluation and can raise — a custom record printer
/// may be broken, a port may be closed. It therefore runs inside its own
/// [`catch_raw`], whose failure path does no printing, so the recursion is at
/// most one level deep.
pub(crate) fn write_to_string(value: Scm) -> String {
    let rendered = catch_raw(|| {
        // SAFETY: any error raised is caught by the enclosing `catch_raw`.
        //
        // The printer argument must be `SCM_UNDEFINED`, not `SCM_UNSPECIFIED`.
        // Guile signals "this optional C argument was omitted" with the former;
        // passing the latter makes `object->string` treat the unspecified value
        // as the printer procedure and raise, which turned every diagnostic
        // into "<unprintable>".
        Scm::from_raw(unsafe {
            bindings::scm_object_to_string(value.raw(), bindings::dossel_undefined())
        })
    });

    match rendered {
        Ok(s) => s.as_string().unwrap_or_else(|| "<unprintable>".to_owned()),
        Err(_) => "<unprintable>".to_owned(),
    }
}

/// Raise a Scheme error attributed to `subr`, carrying `message`.
///
/// # The careful part
///
/// This performs a `longjmp`, so any Rust value with a destructor still live on
/// the stack at that moment is leaked. That is why this takes already-built
/// [`Scm`] values rather than `&str`s: a borrowed string implies an owner one
/// frame up that would not be dropped. Callers convert their message with
/// [`Scm::from_str_lossy`], drop the owned `String`, and only then call here —
/// see `module::guard` for the canonical sequence.
pub(crate) fn throw(subr: Scm, message: Scm) -> ! {
    // SAFETY: `dossel_throw_error` calls `scm_error_scm`, which does not
    // return. The only live locals are plain `SCM` words owned by the
    // collector, so the `longjmp` skips no destructor. The message is passed as
    // a format *argument* rather than as the format string, so tildes in
    // node-supplied text cannot be interpreted as directives.
    unsafe { bindings::dossel_throw_error(subr.raw(), message.raw()) }
}

/// Enter Guile mode on the current thread and run `f`.
///
/// This is the boundary that establishes the module invariant: inside `f`, the
/// thread is registered with the collector and the rest of this module is
/// legal to call.
///
/// # Panics
///
/// `f` must not panic. It runs inside an `extern "C"` frame, so an unwind would
/// abort the process; [`super::runtime`] catches panics inside `f` before they
/// reach that frame.
pub(crate) fn with_guile<F>(mut f: F)
where
    F: FnMut(),
{
    unsafe extern "C" fn trampoline(data: *mut c_void) -> *mut c_void {
        // SAFETY: `data` is the `&mut dyn FnMut()` set up below, alive for the
        // duration of the `scm_with_guile` call.
        let f = unsafe { &mut *data.cast::<&mut dyn FnMut()>() };
        f();
        std::ptr::null_mut()
    }

    let mut erased: &mut dyn FnMut() = &mut f;
    let data = std::ptr::from_mut(&mut erased).cast::<c_void>();

    // SAFETY: `trampoline` matches the signature `scm_with_guile` expects and
    // `data` points at `erased`, a local that outlives the call.
    unsafe {
        bindings::scm_with_guile(Some(trampoline), data);
    }
}

/// Evaluate a Scheme expression, catching any error it raises.
pub(crate) fn eval_string(code: &str) -> Result<Scm, GuileException> {
    // Built outside the closure so no allocation happens inside the `catch`
    // body, where a `longjmp` would skip its destructor.
    let c_code = CString::new(code).unwrap_or_else(|_| {
        CString::new(code.replace('\0', " ")).unwrap_or_default()
    });

    catch(|| {
        // SAFETY: `c_code` is a valid NUL-terminated string that outlives the
        // call, and any Scheme error is caught by the enclosing `catch`.
        Scm::from_raw(unsafe { bindings::scm_c_eval_string(c_code.as_ptr()) })
    })
}

/// Load and evaluate a Scheme source file, catching any error it raises.
pub(crate) fn load_file(path: &str) -> Result<Scm, GuileException> {
    let c_path = CString::new(path).unwrap_or_default();

    catch(|| {
        // SAFETY: as `eval_string`.
        Scm::from_raw(unsafe { bindings::scm_c_primitive_load(c_path.as_ptr()) })
    })
}

/// Define a primitive procedure in the current module.
///
/// `req`, `opt` and `rest` describe the arity exactly as `scm_c_define_gsubr`
/// does. `func` must be an `extern "C"` function taking that many `SCM`
/// arguments and returning `SCM`.
///
/// # Safety
///
/// `func` must have the C signature implied by `req`/`opt`/`rest`. Getting this
/// wrong is a type-confused call through a function pointer. Callers in
/// [`super::module`] pair each registration with its handler directly so the two
/// cannot drift apart.
pub(crate) unsafe fn define_gsubr(
    name: &str,
    req: i32,
    opt: i32,
    rest: i32,
    func: *mut c_void,
) {
    let c_name = CString::new(name).unwrap_or_default();

    // SAFETY: `c_name` outlives the call; libguile copies the name. The arity
    // contract on `func` is the caller's obligation, documented above.
    unsafe {
        bindings::scm_c_define_gsubr(
            c_name.as_ptr().cast::<c_char>(),
            req,
            opt,
            rest,
            func,
        );
    }
}

/// Export a name from the module currently being defined.
pub(crate) fn export(name: &str) {
    let c_name = CString::new(name).unwrap_or_default();

    // SAFETY: `c_name` outlives the call. `scm_c_export` is only meaningful
    // while a module definition is in progress, which is where `super::module`
    // calls it from.
    unsafe {
        bindings::dossel_export_1(c_name.as_ptr().cast::<c_char>());
    }
}
