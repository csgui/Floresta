/* SPDX-License-Identifier: MIT OR Apache-2.0 */

#include "dossel_shim.h"

/* --- Constants ---------------------------------------------------------- */

SCM dossel_bool_t(void) { return SCM_BOOL_T; }
SCM dossel_bool_f(void) { return SCM_BOOL_F; }
SCM dossel_eol(void) { return SCM_EOL; }
SCM dossel_unspecified(void) { return SCM_UNSPECIFIED; }
SCM dossel_undefined(void) { return SCM_UNDEFINED; }

/* --- Predicates --------------------------------------------------------- */

int dossel_is_true(SCM x) { return scm_is_true(x); }
int dossel_is_false(SCM x) { return scm_is_false(x); }
int dossel_is_pair(SCM x) { return scm_is_pair(x); }
int dossel_is_string(SCM x) { return scm_is_string(x); }
int dossel_is_symbol(SCM x) { return scm_is_symbol(x); }
int dossel_is_exact_integer(SCM x) { return scm_is_exact_integer(x); }
int dossel_is_real(SCM x) { return scm_is_true(scm_real_p(x)); }
int dossel_is_undefined(SCM x) { return SCM_UNBNDP(x); }
int dossel_is_null(SCM x) { return scm_is_null(x); }

/* --- Construction ------------------------------------------------------- */

SCM dossel_cons(SCM car, SCM cdr) { return scm_cons(car, cdr); }
SCM dossel_car(SCM pair) { return SCM_CAR(pair); }
SCM dossel_cdr(SCM pair) { return SCM_CDR(pair); }
SCM dossel_from_bool(int b) { return scm_from_bool(b); }
SCM dossel_acons(SCM key, SCM value, SCM alist) { return scm_acons(key, value, alist); }
SCM dossel_list_1(SCM a) { return scm_list_1(a); }

/* --- Errors ------------------------------------------------------------- */

void dossel_throw_error(SCM subr, SCM message)
{
  /*
   * The key is `misc-error` rather than something Dossel-specific on purpose.
   * Guile's REPL renders a recognised error key as
   *
   *     In procedure get-config: max-peers is not available: ...
   *
   * whereas an unrecognised key falls back to dumping the raw throw form:
   *
   *     Throw to key `dossel-error' with args `(#f "~A" ("...") #f)'.
   *
   * The first is what an operator should see. `misc-error` is the idiomatic
   * key for exactly this case.
   *
   * "~A" formats the single argument in `args` into the message, so the
   * message text itself is never treated as a format string. That matters
   * because the text can contain node-supplied data (peer addresses, error
   * strings) that may legitimately contain a tilde.
   */
  scm_error_scm(scm_from_utf8_symbol("misc-error"),
                subr,
                scm_from_utf8_string("~A"),
                scm_list_1(message),
                SCM_BOOL_F);

  /* scm_error_scm does not return; this is unreachable and only silences
     -Wreturn-type style warnings on compilers that cannot see that. */
  __builtin_unreachable();
}

/* --- Module export ------------------------------------------------------ */

void dossel_export_1(const char *name) { scm_c_export(name, NULL); }
