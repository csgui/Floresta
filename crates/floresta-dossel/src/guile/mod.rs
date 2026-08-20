// SPDX-License-Identifier: MIT OR Apache-2.0

//! The embedded Guile interpreter: bindings, safe wrappers, module
//! registration, the async bridge and the REPL server.

pub(crate) mod bindings;
pub(crate) mod bridge;
pub(crate) mod module;
pub(crate) mod repl;
pub(crate) mod runtime;
pub(crate) mod safe;
