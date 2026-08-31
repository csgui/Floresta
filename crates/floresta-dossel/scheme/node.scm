;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; The Scheme half of the (floresta node) module.
;;;
;;; The Rust side registers primitives named with a leading `%`. This file
;;; is where the procedures operators actually call belong, defined on top
;;; of those primitives — argument checks, optional arguments, docstrings,
;;; help text.
;;;
;;; REPL presentation (colors, banner, prompt) lives in repl.scm instead,
;;; which loads before this file.
;;;
;;; This file is embedded into florestad with `include_str!`, so there is
;;; nothing to install and no load path to configure.

(use-modules (ice-9 format))

(define (get-block-height)
  "Return the height of the current best chain tip, as an exact integer."
  (%get-block-height))

;;; ------------------------------------------------------------------
;;; Generic RPC passthrough
;;; ------------------------------------------------------------------

(define* (rpc-call method #:optional (params '()))
  "Call the JSON-RPC method METHOD with PARAMS and return the parsed result.

METHOD is a string or symbol. PARAMS is a list, defaulting to the empty list.
JSON objects come back as association lists with symbol keys, arrays as lists,
and null as the symbol 'null.

  (rpc-call \"getblockcount\")
  (rpc-call \"getblockhash\" '(0))"
  (%rpc-call method params))

;;; ------------------------------------------------------------------
;;; Configuration
;;; ------------------------------------------------------------------

(define (get-config key)
  "Return the value of the configuration KEY, a symbol.

Reading a key that has no backing in this build raises an error explaining
why."
  (%get-config key))

(define (set-config! key value)
  "Set the configuration KEY to VALUE, returning 'ok.

Raises an error if KEY is read-only, if VALUE is out of range, or if the key
has no backing in this build. Consensus parameters are not configuration
and do not appear here at all."
  (%set-config! key value))

;;; ------------------------------------------------------------------
;;; Help
;;; ------------------------------------------------------------------

(define (dossel-help)
  "Print the procedures this build currently implements.

This surface is grown one procedure at a time, not delivered up front — see
`(rpc-call \"getdeploymentinfo\")`-style calls for anything not listed here;
`rpc-call` reaches every JSON-RPC method florestad implements, whether or
not there is a dedicated wrapper for it yet."
  (format #t "~%Dossel - the Floresta node REPL~%~%")
  (format #t "Chain~%")
  (format #t "  (get-block-height)                     current tip height~%")
  (format #t "    (get-block-height) => 320169~%~%")
  (format #t "RPC passthrough~%")
  (format #t "  (rpc-call method [params])              call any JSON-RPC method~%")
  (format #t "    (rpc-call \"getblockcount\")~%")
  (format #t "    (rpc-call \"getblockhash\" '(0))~%")
  (format #t "    (rpc-call \"getpeerinfo\")~%~%")
  (format #t "Configuration~%")
  (format #t "  (get-config key)                        read a key~%")
  (format #t "  (set-config! key value)                 write a key~%")
  (format #t "    (get-config 'network) => regtest~%")
  (format #t "    (set-config! 'log-level \"debug\")~%~%")
  (format #t "REPL~%")
  (format #t "  (clear)                                 clear the screen, replay the banner~%")
  (format #t "  (quit)                                  end this session; the node keeps running~%~%")
  (format #t "Any procedure's own documentation is available with ,d, e.g.~%")
  (format #t "  ,d rpc-call~%~%")
  (format #t "Sessions share the (guile-user) module, so a definition made in~%")
  (format #t "one session is visible in every other one, including later ones.~%~%")
  (values))
