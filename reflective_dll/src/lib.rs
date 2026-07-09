//! Runtime companion to the `reflective_dll_macro` proc-macro crate.
//!
//! # What this crate provides
//!
//! 1. **`reflective_dll` attribute macro** (re-exported from `reflective_dll_macro`)
//!    — annotate your `dll_main` function to auto-generate `DllMain` and
//!    `ReflectiveLoader` exports.
//!
//! 2. **`loader` module** — contains `load_self(raw_dll_base: usize) -> usize`,
//!    which the generated `ReflectiveLoader` export calls when the injector
//!    starts it via `CreateRemoteThread`.
//!
//! # no_std
//!
//! This crate is `#![no_std]` because it is linked into `cdylib` DLL crates
//! that are also `#![no_std]`.  Only `core` is available here.

#![no_std]

pub use reflective_dll_macro::reflective_dll;

pub mod loader;
