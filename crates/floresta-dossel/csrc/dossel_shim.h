/* SPDX-License-Identifier: MIT OR Apache-2.0 */

/*
 * Dossel's C shim over libguile.
 *
 * Almost everything Dossel needs from Guile is already a real, linkable
 * function that bindgen can bind directly. Three categories are not, and this
 * shim exists solely to turn those into ordinary functions:
 *
 *   1. Constants. SCM_BOOL_T, SCM_EOL, SCM_UNSPECIFIED and friends are
 *      preprocessor macros expanding to bit-pattern arithmetic (see
 *      libguile/scm.h). bindgen cannot evaluate them.
 *   2. Predicates. scm_is_true / scm_is_false / scm_is_pair are macros.
 *   3. Variadics. scm_c_export, scm_list_n and scm_error take varargs or
 *      sentinel-terminated argument lists, which are not safely callable from
 *      Rust.
 *
 * Everything here is a thin, allocation-free wrapper. None of these functions
 * take ownership of anything.
 */

#ifndef DOSSEL_SHIM_H
#define DOSSEL_SHIM_H

#include <libguile.h>

/* --- Constants ---------------------------------------------------------- */

SCM dossel_bool_t(void);
SCM dossel_bool_f(void);
SCM dossel_eol(void);
SCM dossel_unspecified(void);
SCM dossel_undefined(void);

/* --- Predicates --------------------------------------------------------- */

int dossel_is_true(SCM x);
int dossel_is_false(SCM x);
int dossel_is_pair(SCM x);
int dossel_is_string(SCM x);
int dossel_is_symbol(SCM x);
int dossel_is_exact_integer(SCM x);
int dossel_is_real(SCM x);
int dossel_is_undefined(SCM x);
int dossel_is_null(SCM x);

/* --- Construction ------------------------------------------------------- */

SCM dossel_cons(SCM car, SCM cdr);
SCM dossel_car(SCM pair);
SCM dossel_cdr(SCM pair);
SCM dossel_from_bool(int b);
SCM dossel_acons(SCM key, SCM value, SCM alist);
SCM dossel_list_1(SCM a);

/* --- Errors ------------------------------------------------------------- */

/*
 * Raise a Scheme error attributed to SUBR, carrying MESSAGE.
 *
 * This performs a non-local exit (longjmp) and never returns to its caller.
 * Rust callers must ensure no live destructors remain on the stack.
 */
void dossel_throw_error(SCM subr, SCM message) __attribute__((noreturn));

/* --- Module export ------------------------------------------------------ */

/* scm_c_export is variadic and NULL-terminated; export one name at a time. */
void dossel_export_1(const char *name);

#endif /* DOSSEL_SHIM_H */
