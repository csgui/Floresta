// SPDX-License-Identifier: MIT OR Apache-2.0

//! Raw, generated FFI declarations for libguile and Dossel's C shim.
//!
//! Everything in here is `unsafe` and unchecked. Nothing outside
//! [`crate::guile`] should use it directly; go through
//! [`crate::guile::safe`] instead.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/guile_bindings.rs"));
