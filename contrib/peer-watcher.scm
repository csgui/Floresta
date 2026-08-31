;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; Polls connected peers every 5 seconds and announces changes: a new
;;; connection or a disconnection. Peers already connected when this loads
;;; are the starting baseline, so nothing is announced on load itself.
;;; Starts automatically as soon as this file is loaded.
;;;
;;; Built entirely on (rpc-call "getpeerinfo") -- there is no dedicated peer
;;; primitive in the current minimal surface, and none is needed for this.
;;;
;;; Load at node startup:
;;;   florestad --dossel --load contrib/peer-watcher.scm ...
;;;
;;; Or into an already-running node, from a connected REPL session:
;;;   (load "/path/to/contrib/peer-watcher.scm")

(use-modules (ice-9 format)
             (ice-9 threads)   ; call-with-new-thread, sleep
             (srfi srfi-1))    ; lset-difference

(define (peer-addresses)
  (map (lambda (p) (assq-ref p 'address)) (rpc-call "getpeerinfo")))

(define (peer-watcher-loop period known)
  (sleep period)
  (let* ((current (peer-addresses))
         (joined (lset-difference string=? current known))
         (left (lset-difference string=? known current)))
    (for-each (lambda (addr) (format #t "Peer connected: ~a~%" addr)) joined)
    (for-each (lambda (addr) (format #t "Peer disconnected: ~a~%" addr)) left)
    (force-output)
    (peer-watcher-loop period current)))

(define* (start-peer-watcher! #:optional (period 5))
  (call-with-new-thread
   (lambda ()
     (catch #t
       (lambda () (peer-watcher-loop period (peer-addresses)))
       (lambda (key . args)
         (format #t "peer-watcher died: ~a ~a~%" key args)))))
  'peer-watcher-running)

;; Start immediately -- loading this file is what activates the watcher.
(start-peer-watcher! 5)
