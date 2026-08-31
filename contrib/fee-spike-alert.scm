;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; DESIGN SKETCH -- does not run. None of the primitives below exist in
;;; Dossel yet:
;;;
;;;   (on (new-block block) ...)   an event-subscription form; nothing in
;;;                                Dossel currently pushes events into a
;;;                                session -- everything today is pull
;;;                                (rpc-call, get-block-height) or a
;;;                                hand-rolled poll loop (see
;;;                                contrib/block-watcher.scm).
;;;   block                        an event payload carrying a full block,
;;;                                as opposed to just a height.
;;;   (block-median-fee block)     no fee data is exposed at all today.
;;;   (remember! key value #:limit n)
;;;   (recall key)                 a per-session or per-node rolling-history
;;;                                store, keyed by symbol, capped at a size.
;;;   (average list)
;;;   (alert fmt . args)           a notification sink distinct from plain
;;;                                stdout output.
;;;
;;; Kept here as a concrete target for what an event-handling API in Dossel
;;; could look like, in case it's built later. Per this project's rule of
;;; growing the primitive surface one real capability at a time, none of
;;; this belongs in contrib/ (which holds only working scripts) until each
;;; primitive it depends on has been implemented and has a real caller.

(on (new-block block)
  (remember! 'fees (block-median-fee block) #:limit 100)

  (let ((avg (average (recall 'fees))))
    (when (> (block-median-fee block) (* 2 avg))
      (alert "fee spike: ~ax"
             (/ (block-median-fee block) avg)))))
