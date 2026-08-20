;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; Dossel's REPL presentation: colors, the banner, the prompt, (clear).
;;; This is session bootstrap, not a standard library — it sets up how a
;;; connection looks, not reusable functions for other code to call. None
;;; of it depends on the `%` primitives, so it is evaluated before those
;;; are registered; see guile/module.rs.
;;;
;;; Floresta-specific procedures (get-block-height, list-peers, and the
;;; rest, wrapping those primitives) belong in node.scm instead, loaded
;;; after this file. A genuine prelude.scm — reusable standard-library
;;; functions, generic to Dossel, available to that code and to anyone's
;;; own --load scripts — doesn't exist yet.
;;;
;;; This file is embedded into florestad with `include_str!`, so there is
;;; nothing to install and no load path to configure.
(use-modules (system repl repl)
             (system repl common))
;;; Silence Guile's version/copyright banner. Replacing it via `repl-welcome`
;;; doesn't work: (system repl repl) calls that name as a binding resolved
;;; within its own precompiled module, so runtime `set!` from here never
;;; reaches it — verified empirically, not just assumed. The prompt
;;; procedure below shows our own banner instead, since that IS a plain
;;; first-class procedure value Guile calls directly, with no cross-module
;;; indirection to fight.
(%inhibit-welcome-message #t)
;;; ANSI color helpers. Plain escape bytes embedded in a displayed or
;;; returned string — verified to survive Guile's REPL output path intact,
;;; since the socket is a raw byte stream and nc/rlwrap/socat all pass
;;; escape sequences through untouched. Rendering is up to the client's
;;; terminal, same as any other program that colors its output.
(define (ansi code) (string-append (string (integer->char 27)) "[" code "m"))
(define ansi-reset (ansi "0"))
(define ansi-bold  (ansi "1"))
(define ansi-red   (ansi "31"))   ; (quit) — a plain alarm color is the point
;;; Phosphor palette. 24-bit truecolor SGR sequences, so they go through the
;;; same `ansi` helper as the basic codes above — the "38;2;r;g;b" form still
;;; terminates in the "m" that `ansi` appends, so no separate builder is
;;; needed. Greens evoke the canopy. If a client terminal lacks truecolor
;;; (check $COLORTERM for "truecolor"/"24bit"), swap these for their 256-color
;;; approximations: 38;5;84 / 78 / 65 / 245, in the order below.
(define canopy-hi (ansi "38;2;130;235;150"))   ; bright leaf highlight
(define canopy    (ansi "38;2;80;185;110"))    ; canopy body
(define bark      (ansi "38;2;52;120;78"))     ; frame
(define muted     (ansi "38;2;140;150;145"))   ; secondary text
;;; Clear-screen + cursor-home. Not an SGR color code, so built separately
;;; from `ansi` rather than reusing it (that helper always appends the "m"
;;; that terminates a color sequence, which doesn't apply here).
(define ansi-clear-screen
  (string-append (string (integer->char 27)) "[2J"
                  (string (integer->char 27)) "[H"))
;;; Draw an ASCII box around the title — plain +, -, | so the frame renders
;;; identically on any terminal regardless of Unicode or locale support. The
;;; width padding is applied to the plain text BEFORE the color escapes are
;;; prepended, so the zero-width SGR bytes never enter string-length's count
;;; and the right border stays aligned.
(define (dossel-banner)
  (define w 50)                                   ; interior width
  (define rule (make-string w #\-))
  (define (pad s)
    (string-append s (make-string (max 0 (- w (string-length s))) #\space)))
  (define (row color text)
    (string-append bark "|" ansi-reset
                   color (pad text) ansi-reset
                   bark "|" ansi-reset "\n"))
  (display (string-append bark "+" rule "+" ansi-reset "\n"))
  (display (row canopy ""))
  (display (row (string-append ansi-bold canopy-hi) "   Dossel"))
  (display (row canopy "   Floresta's programmable environment"))
  (display (row canopy ""))
  (display (string-append bark "+" rule "+" ansi-reset "\n\n"))
  (display (string-append
             muted "  " ansi-red "(quit)" muted
             " ends this session. The node keeps running."
             ansi-reset "\n\n")))
;;; Print the Dossel banner on a session's first prompt, then just the
;;; prompt on every one after. *greeted* is keyed by the repl object's
;;; identity (hashq, i.e. eq?), so each connection gets exactly one
;;; automatic banner regardless of how many lines it sends afterward.
;;; (clear), below, can always bring it back on demand.
(define *greeted* (make-hash-table))
(define (dossel-prompt repl)
  (unless (hashq-ref *greeted* repl)
    (hashq-set! *greeted* repl #t)
    (dossel-banner))
  (string-append ansi-bold canopy "dossel> " ansi-reset))
(repl-default-prompt-set! dossel-prompt)
(define (clear)
  "Clear the terminal and show the Dossel greeting again."
  (display ansi-clear-screen)
  (dossel-banner)
  (values))
