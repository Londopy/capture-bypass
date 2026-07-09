//! Proc-macro: `#[reflective_dll]`
//!
//! Annotate a `dll_main` function with this attribute and the macro generates
//! two additional exports in your `cdylib` crate:
//!
//! - **`DllMain`** — the standard Windows entry point.  Called by LoadLibrary
//!   injection and by the reflective loader after it has finished mapping the
//!   DLL.  The macro just forwards all three arguments to your `dll_main`.
//!
//! - **`ReflectiveLoader`** — the reflective injection entry point.  The
//!   injector writes the raw DLL bytes into the target process and calls this
//!   export via `CreateRemoteThread` instead of using `LoadLibraryA`.  A full
//!   position-independent PE loader lives in `reflective_dll::loader::load_self`;
//!   the generated stub calls through to it.
//!
//! # Usage
//!
//! ```rust
//! // In your cdylib crate:
//! #![no_std]
//!
//! use reflective_dll::reflective_dll;
//! use windows::Win32::Foundation::{BOOL, HMODULE, TRUE};
//!
//! #[reflective_dll]
//! unsafe fn dll_main(
//!     module:      HMODULE,
//!     call_reason: u32,
//!     _reserved:   *mut core::ffi::c_void,
//! ) -> BOOL {
//!     // your DLL_PROCESS_ATTACH logic here
//!     TRUE
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn reflective_dll(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let user_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &user_fn.sig.ident;

    let expanded = quote! {
        // ── User's dll_main function (kept under its original name) ──────────
        #user_fn

        // ── Standard DLL entry point ──────────────────────────────────────────
        //
        // Used when the DLL is loaded via LoadLibraryA (classic injection).
        // Also called by ReflectiveLoader after it finishes mapping the image.
        #[no_mangle]
        pub unsafe extern "system" fn DllMain(
            module:      ::windows::Win32::Foundation::HMODULE,
            call_reason: u32,
            reserved:    *mut ::core::ffi::c_void,
        ) -> ::windows::Win32::Foundation::BOOL {
            #fn_name(module, call_reason, reserved)
        }

        // ── Reflective injection entry point ──────────────────────────────────
        //
        // The injector (core::inject_dll_reflective) finds this export's RVA in
        // the raw DLL bytes, then calls it via CreateRemoteThread instead of
        // going through LoadLibraryA.  The function receives the base address of
        // the raw DLL buffer as its parameter.
        //
        // A complete position-independent PE loader is provided by
        // `reflective_dll::loader::load_self`.  See that module's documentation
        // for implementation notes and a reference to the canonical open-source
        // implementation (Stephen Fewer's ReflectiveDLLInjection).
        #[no_mangle]
        #[allow(non_snake_case)]
        pub unsafe extern "system" fn ReflectiveLoader(raw_dll_base: usize) -> usize {
            ::reflective_dll::loader::load_self(raw_dll_base)
        }
    };

    TokenStream::from(expanded)
}
