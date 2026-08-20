// SPDX-License-Identifier: MIT OR Apache-2.0

//! The Unix domain socket REPL server.
//!
//! Guile already ships a REPL server in `(system repl server)`. Dossel does not
//! reimplement it; it prepares the socket path, asks Guile to listen there, and
//! tightens the permissions. Everything about the REPL itself — the reader, the
//! printer, the meta-commands, the per-client threads — is Guile's.
//!
//! `spawn-server` creates one Guile thread per connected client, which is what
//! gives multiple simultaneous sessions (acceptance criterion 5) for free. It
//! is also the reason the node handle cannot live in a thread-local; see
//! [`super::bridge`].

use std::fs;
use std::mem::offset_of;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::safe;
use crate::error::DosselError;

/// The longest path a Unix domain socket can have on this platform.
///
/// `sun_path` is a fixed-size array inside `sockaddr_un`, so an over-long path
/// is silently truncated by `bind(2)` rather than rejected — which would leave
/// the node listening somewhere other than where it logged. Better to refuse up
/// front with a message that says what to do.
const SUN_PATH_MAX: usize = size_of::<libc::sockaddr_un>()
    - offset_of!(libc::sockaddr_un, sun_path)
    - 1; // trailing NUL

/// Permissions for the REPL socket: owner read/write only.
///
/// This is the whole access control story. Anything that can connect to this
/// socket can evaluate arbitrary Scheme in the node process, so the socket must
/// never be group- or world-accessible.
const SOCKET_MODE: u32 = 0o600;

/// Permissions for the directory holding the socket: owner only.
///
/// Belt and braces. There is an unavoidable window between `bind(2)` creating
/// the socket and `chmod(2)` tightening it, during which the socket carries
/// whatever the process umask allows. A `0700` parent directory closes that
/// window: no other user can reach the socket inode regardless of its own mode.
const SOCKET_DIR_MODE: u32 = 0o700;

/// Prepare the filesystem so Guile can bind `path`.
///
/// Creates the parent directory `0700`, removes any stale socket left by a
/// previous run, and checks the path against the platform's `sun_path` limit.
pub(crate) fn prepare_socket_path(path: &Path) -> Result<(), DosselError> {
    let len = path.as_os_str().as_encoded_bytes().len();
    if len > SUN_PATH_MAX {
        return Err(DosselError::SocketPathTooLong {
            path: path.to_path_buf(),
            len,
            max: SUN_PATH_MAX,
        });
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| DosselError::SocketDir {
            path: parent.to_path_buf(),
            source,
        })?;

        fs::set_permissions(parent, fs::Permissions::from_mode(SOCKET_DIR_MODE)).map_err(
            |source| DosselError::SocketDir {
                path: parent.to_path_buf(),
                source,
            },
        )?;
    }

    // `bind(2)` fails with EADDRINUSE if the path exists, even when nothing is
    // listening — an unclean shutdown always leaves one behind.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DosselError::StaleSocket {
                path: path.to_path_buf(),
                source,
            });
        }
    }

    Ok(())
}

/// Start the REPL server listening on `path`.
///
/// Returns once the socket is bound and accepting; the serving itself happens
/// on Guile-managed threads.
///
/// Must be called from a thread in Guile mode; see [`safe`].
pub(crate) fn start(path: &Path) -> Result<(), DosselError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| DosselError::NonUtf8SocketPath(path.to_path_buf()))?;

    let literal = scheme_string_literal(path_str);

    // Two things happen before `spawn-server`, and the order matters.
    //
    // `chmod` runs between `bind` and `spawn-server`, so the socket is already
    // `0600` before the first client can be accepted.
    //
    // `%default-port-encoding` is set because client ports otherwise inherit
    // their encoding from the process locale, and a daemon typically runs under
    // `LC_ALL=C`. That would mangle every non-ASCII byte the REPL prints —
    // including peer user-agent strings, which are remote-controlled text.
    // Forcing UTF-8 here works regardless of the ambient locale.
    let script = format!(
        "(begin\n\
         \x20 (use-modules (system repl server))\n\
         \x20 (fluid-set! %default-port-encoding \"UTF-8\")\n\
         \x20 (let ((sock (make-unix-domain-server-socket #:path {literal})))\n\
         \x20   (chmod {literal} #o{mode:o})\n\
         \x20   (spawn-server sock)))",
        mode = SOCKET_MODE,
    );

    safe::eval_string(&script).map_err(|e| DosselError::Startup {
        context: format!("REPL server startup on {}", path.display()),
        message: e.to_string(),
    })?;

    Ok(())
}

/// Stop the server and disconnect every client.
///
/// Best effort: used on shutdown and before restarting a failed Guile thread,
/// where there may be no server to stop. Errors are returned for logging rather
/// than propagated.
pub(crate) fn stop() -> Result<(), String> {
    safe::eval_string(
        "(begin (use-modules (system repl server)) (stop-server-and-clients!))",
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Render `s` as a Scheme string literal, escaping backslashes and quotes.
///
/// The socket path is operator-supplied, so it is not safe to paste raw into
/// generated Scheme source.
fn scheme_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(scheme_string_literal("/tmp/a.sock"), r#""/tmp/a.sock""#);
        assert_eq!(scheme_string_literal(r#"/tmp/a"b"#), r#""/tmp/a\"b""#);
        assert_eq!(scheme_string_literal(r"/tmp/a\b"), r#""/tmp/a\\b""#);
    }

    #[test]
    fn sun_path_max_is_plausible() {
        // 104 on the BSDs and macOS, 108 on Linux.
        assert!(
            (100..=108).contains(&SUN_PATH_MAX),
            "unexpected sun_path limit: {SUN_PATH_MAX}"
        );
    }

    #[test]
    fn rejects_over_long_socket_path() {
        let long = std::path::PathBuf::from(format!("/tmp/{}", "x".repeat(SUN_PATH_MAX)));
        assert!(matches!(
            prepare_socket_path(&long),
            Err(DosselError::SocketPathTooLong { .. })
        ));
    }
}
