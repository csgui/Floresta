;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; Polls the chain tip every 3 seconds and reports on each tick: "No new
;;; block" if the height hasn't moved, or "New block <height>" if it has.
;;; Starts automatically as soon as this file is loaded.
;;;
;;; Load at node startup:
;;;   florestad --dossel --load contrib/block-watcher.scm ...
;;;
;;; Or into an already-running node, from a connected REPL session:
;;;   (load "/path/to/contrib/block-watcher.scm")

(use-modules (ice-9 format)   ; format
             (ice-9 threads)) ; call-with-new-thread, sleep

(define (block-watcher-loop period known-height)
  (sleep period)
  (let ((height (get-block-height)))
    (if (> height known-height)
        (format #t "New block ~a~%" height)
        (format #t "No new block~%"))
    (force-output)
    (block-watcher-loop period height)))

(define* (start-block-watcher! #:optional (period 3))
  (call-with-new-thread
   (lambda ()
     (catch #t
       (lambda () (block-watcher-loop period (get-block-height)))
       (lambda (key . args)
         (format #t "block-watcher died: ~a ~a~%" key args)))))
  'block-watcher-running)

;; Start immediately — loading this file is what activates the watcher.
(start-block-watcher! 3)
