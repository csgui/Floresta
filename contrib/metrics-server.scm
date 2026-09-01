;;; SPDX-License-Identifier: MIT OR Apache-2.0
;;;
;;; Serves a small HTML metrics page over plain HTTP, computed fresh from
;;; live rpc-call/get-block-height on every request -- no polling loop, no
;;; cached state, so there's nothing to keep in sync or protect against
;;; concurrent access. Starts automatically as soon as this file is loaded.
;;;
;;; Bound to 127.0.0.1 only. This opens a second, separate surface from the
;;; Dossel socket itself -- unlike the socket, it's read-only (the handler
;;; below only ever calls rpc-call with read-only methods), but it's still a
;;; new listener on the machine, so it stays loopback-only rather than
;;; reachable from the network. If you want it reachable from elsewhere,
;;; that's a job for a reverse proxy you control (nginx, an SSH tunnel, an
;;; authenticated ngrok-style service), not for widening the bind address
;;; here.
;;;
;;; Load at node startup:
;;;   florestad --dossel --load contrib/metrics-server.scm ...
;;;
;;; Or into an already-running node, from a connected REPL session:
;;;   (load "/path/to/contrib/metrics-server.scm")
;;;
;;; Then, from the same machine (or through a tunnel):
;;;   curl http://127.0.0.1:8090/

(use-modules (web server)
             (ice-9 format)
             (ice-9 threads))   ; call-with-new-thread

;;; Peer subver strings come from the remote peer, not from us, so they are
;;; escaped before landing in the page -- a hostile peer could otherwise set
;;; its subver to include a literal `<script>` and have it interpreted by
;;; whatever browser later loads this page. Everything else on the page
;;; (heights, hashes, chain name) is locally-controlled, but escaping is
;;; applied uniformly rather than trying to track which fields need it.
(define (html-escape s)
  (string-concatenate
   (map (lambda (c)
          (case c
            ((#\<) "&lt;")
            ((#\>) "&gt;")
            ((#\&) "&amp;")
            ((#\") "&quot;")
            (else (string c))))
        (string->list s))))

;;; Phosphor palette, the same greens repl.scm uses for the REPL banner --
;;; kept as one small style block rather than inline styles on every
;;; element, so the look stays in one place if it needs adjusting later.
(define page-style
  "body {
  background: #0b1210;
  color: #50b96e;
  font-family: ui-monospace, Menlo, Consolas, 'Liberation Mono', monospace;
  padding: 2rem;
}
h1, h2 {
  color: #82eb96;
  font-weight: bold;
  border-bottom: 1px solid #34784e;
  padding-bottom: 0.3rem;
}
ul { list-style: none; padding-left: 0; }
li {
  padding: 0.2rem 0 0.2rem 0.6rem;
  border-left: 2px solid #34784e;
  margin-bottom: 0.15rem;
}
.label { color: #8c9691; }
.value { color: #82eb96; font-weight: bold; }
.ibd-true { color: #e0a030; font-weight: bold; }
.ibd-false { color: #82eb96; font-weight: bold; }
footer {
  margin-top: 2rem;
  padding-top: 1rem;
  border-top: 1px solid #34784e;
  color: #8c9691;
  font-size: 0.85rem;
}")

(define (ibd-badge in-progress?)
  (if in-progress?
      "<span class=\"ibd-true\">true</span>"
      "<span class=\"ibd-false\">false</span>"))

(define (field label value)
  (format #f "<li><span class=\"label\">~a:</span> <span class=\"value\">~a</span></li>~%"
          label value))

(define (status-html)
  (let* ((info (rpc-call "getblockchaininfo"))
         (peers (rpc-call "getpeerinfo")))
    (format #f
            "<!DOCTYPE html>
<html>
<head>
<meta charset=\"utf-8\">
<title>Floresta Status Page</title>
<style>~a</style>
</head>
<body>
<h1>Floresta node status</h1>

<h2>Chain</h2>
<ul>
~a~a~a~a~a
</ul>

<h2>Network</h2>
<ul>
~a
</ul>

<h2>Peers</h2>
<ul>
~a
</ul>

<footer>Powered by Dossel - Floresta's programmable environment for the Utreexo network.</footer>
</body>
</html>
"
            page-style
            (field "Network" (html-escape (assq-ref info 'chain)))
            (field "Block Height" (get-block-height))
            (field "Header Height" (assq-ref info 'headers))
            (field "Difficulty" (assq-ref info 'difficulty))
            (field "IBD" (ibd-badge (assq-ref info 'initialblockdownload)))
            (field "Peers" (length peers))
            (string-concatenate
             (map (lambda (p)
                    (field (html-escape (or (assq-ref p 'address) "?"))
                           (html-escape (or (assq-ref p 'user_agent) "?"))))
                  peers)))))

(define (handler request body)
  (values '((content-type . (text/html)))
          (status-html)))

(define* (start-metrics-server! #:optional (port 8090))
  (call-with-new-thread
   (lambda ()
     ;; Qualified reference: (system repl server), used internally by
     ;; Dossel's own REPL machinery in this same shared (guile-user)
     ;; namespace, exports a `run-server' of its own -- a plain
     ;; `use-modules (web server)' produces a duplicate-binding warning and
     ;; relies on import order to resolve to the right one. Naming it
     ;; explicitly removes the ambiguity instead of depending on that order.
     ((@ (web server) run-server)
      handler 'http (list #:host "127.0.0.1" #:port port))))
  'metrics-server-running)

;; Start immediately -- loading this file is what activates the server.
(start-metrics-server! 8090)
