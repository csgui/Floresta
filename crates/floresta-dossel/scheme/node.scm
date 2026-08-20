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

(define (get-block-height)
  "Return the height of the current best chain tip, as an exact integer."
  (%get-block-height))
