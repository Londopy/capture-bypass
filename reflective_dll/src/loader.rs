//! Position-independent PE loader for reflective DLL injection.
//!
//! `load_self(raw_dll_base)` is called by the generated `ReflectiveLoader`
//! export via `CreateRemoteThread`.  At that point the injector has already
//! written the raw DLL bytes to `raw_dll_base` in the target process.
//!
//! This function:
//!   1. Walks the PEB to find `kernel32.dll` and resolves four APIs.
//!   2. Allocates a fresh region and copies PE headers + sections.
//!   3. Applies base relocations (if allocation missed the preferred base).
//!   4. Resolves the import table.
//!   5. Flushes the instruction cache, then calls the image entry point.
//!
//! No runtime support whatsoever — `#![no_std]`, no allocator, all Windows
//! calls resolved via PEB walking at runtime.  Rust 64-bit codegen is PIC by
//! default, so position-independence is automatic.
//!
//! Based on Stephen Fewer's ReflectiveDLLInjection technique (2008).

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use core::ffi::c_void;

// ─── Windows type shims ────────────────────────────────────────────────────
type LPVOID   = *mut c_void;
type HMODULE  = *mut c_void;
type FARPROC  = *const c_void;
type LPCSTR   = *const u8;
type DWORD    = u32;
type BOOL     = i32;
type SIZE_T   = usize;

type FnVirtualAlloc          = unsafe extern "system" fn(LPVOID, SIZE_T, DWORD, DWORD) -> LPVOID;
type FnLoadLibraryA          = unsafe extern "system" fn(LPCSTR) -> HMODULE;
type FnGetProcAddress        = unsafe extern "system" fn(HMODULE, LPCSTR) -> FARPROC;
type FnFlushInstructionCache = unsafe extern "system" fn(LPVOID, LPVOID, SIZE_T) -> BOOL;
type DllEntry                = unsafe extern "system" fn(HMODULE, DWORD, LPVOID) -> BOOL;

const MEM_COMMIT:             DWORD = 0x1000;
const MEM_RESERVE:            DWORD = 0x2000;
const PAGE_EXECUTE_READWRITE: DWORD = 0x40;
const DLL_PROCESS_ATTACH:     DWORD = 1;
const IMAGE_REL_BASED_DIR64:  u16   = 10;

// ─── In-process structures (PEB / LDR), x64 layout ────────────────────────
// Offsets verified against Windows 10/11 x64 ntdll.

#[repr(C)]
struct LIST_ENTRY {
    Flink: *mut LIST_ENTRY,
    Blink: *mut LIST_ENTRY,
}

// UNICODE_STRING on x64: Length(2) + MaxLength(2) + pad(4) + Buffer(8) = 16 bytes
#[repr(C)]
struct UNICODE_STRING {
    Length:        u16,
    MaximumLength: u16,
    _pad:          u32,
    Buffer:        *const u16,
}

// Abbreviated LDR_DATA_TABLE_ENTRY — only the fields we read.
//   InLoadOrderLinks  @ +0x00  (16 B)
//   InMemoryOrderLinks @ +0x10 (16 B)
//   InInitOrderLinks  @ +0x20  (16 B)
//   DllBase           @ +0x30  ( 8 B)
//   EntryPoint        @ +0x38  ( 8 B)
//   SizeOfImage       @ +0x40  ( 4 B) + 4 pad
//   FullDllName       @ +0x48  (16 B)
//   BaseDllName       @ +0x58  (16 B)
#[repr(C)]
struct LdrEntry {
    InLoadOrderLinks:       LIST_ENTRY,
    InMemoryOrderLinks:     LIST_ENTRY,
    InInitializationLinks:  LIST_ENTRY,
    DllBase:                *const u8,
    EntryPoint:             *const c_void,
    SizeOfImage:            u32,
    _pad:                   u32,
    FullDllName:            UNICODE_STRING,
    BaseDllName:            UNICODE_STRING,
}

// PEB_LDR_DATA: Length(4) + Initialized(1) + pad(3) + SsHandle(8) + InLoadOrderList(16) = …
#[repr(C)]
struct PEB_LDR_DATA {
    Length:           u32,
    Initialized:      u8,
    _pad:             [u8; 3],
    SsHandle:         *const c_void,
    InLoadOrderList:  LIST_ENTRY,   // +0x10
}

// PEB x64: bytes[4] + pad[4] + Mutant(8) + ImageBase(8) + Ldr(8)
#[repr(C)]
struct PEB {
    _bytes: [u8; 4],
    _pad:   [u8; 4],
    Mutant:    *const c_void,    // +0x08
    ImageBase: *const c_void,    // +0x10
    Ldr:       *const PEB_LDR_DATA, // +0x18
}

// ─── Raw PE read helpers (potentially unaligned file bytes) ───────────────

#[inline(always)]
unsafe fn r16(p: *const u8, off: usize) -> u16 {
    core::ptr::read_unaligned(p.add(off) as *const u16)
}
#[inline(always)]
unsafe fn r32(p: *const u8, off: usize) -> u32 {
    core::ptr::read_unaligned(p.add(off) as *const u32)
}
#[inline(always)]
unsafe fn r64(p: *const u8, off: usize) -> u64 {
    core::ptr::read_unaligned(p.add(off) as *const u64)
}
#[inline(always)]
unsafe fn rusize(p: *const u8, off: usize) -> usize {
    core::ptr::read_unaligned(p.add(off) as *const usize)
}

// ─── ASCII / UTF-16 helpers ────────────────────────────────────────────────

/// `true` iff the null-terminated ASCII string at `ptr` matches `expected` (case-insensitive).
unsafe fn ascii_icase_eq(ptr: *const u8, expected: &[u8]) -> bool {
    for (i, &e) in expected.iter().enumerate() {
        let b = *ptr.add(i);
        if b == 0 { return false; }
        if b.to_ascii_uppercase() != e.to_ascii_uppercase() { return false; }
    }
    *ptr.add(expected.len()) == 0
}

/// `true` iff the UTF-16 string (`buf`, `chars` code units) matches the ASCII slice (case-insensitive).
unsafe fn utf16_icase_eq_ascii(buf: *const u16, chars: usize, ascii: &[u8]) -> bool {
    if chars != ascii.len() { return false; }
    for i in 0..chars {
        let wc = *buf.add(i);
        if wc > 0x7F { return false; }
        if (wc as u8).to_ascii_uppercase() != ascii[i].to_ascii_uppercase() {
            return false;
        }
    }
    true
}

// ─── PEB walking ──────────────────────────────────────────────────────────

/// Walk PEB's InLoadOrderModuleList to find the base address of a loaded module.
/// `name_upper_ascii` should be uppercase ASCII, e.g. `b"KERNEL32.DLL"`.
unsafe fn find_module(name_upper_ascii: &[u8]) -> *const u8 {
    let peb: *const PEB;
    core::arch::asm!(
        "mov {peb}, qword ptr gs:[0x60]",
        peb = out(reg) peb,
        options(nostack, pure, nomem),
    );
    if peb.is_null() { return core::ptr::null(); }

    let ldr = (*peb).Ldr;
    if ldr.is_null() { return core::ptr::null(); }

    let list_head = &(*ldr).InLoadOrderList as *const LIST_ENTRY;
    let mut cur   = (*list_head).Flink;

    while cur != list_head as *mut LIST_ENTRY {
        // InLoadOrderLinks is at offset 0 of LdrEntry, so the cast is valid.
        let entry  = cur as *const LdrEntry;
        let name   = &(*entry).BaseDllName;
        let chars  = name.Length as usize / 2; // UTF-16 Length is in bytes
        if !name.Buffer.is_null()
            && utf16_icase_eq_ascii(name.Buffer, chars, name_upper_ascii)
        {
            return (*entry).DllBase;
        }
        cur = (*cur).Flink;
    }
    core::ptr::null()
}

/// Scan the export table of an already-mapped PE image for a named export.
/// Returns the absolute VA of the function, or null.
unsafe fn find_export(module_base: *const u8, name: &[u8]) -> *const c_void {
    if module_base.is_null() { return core::ptr::null(); }

    if r16(module_base, 0) != 0x5A4D { return core::ptr::null(); } // MZ
    let nt_off = r32(module_base, 60) as usize;
    if r32(module_base, nt_off) != 0x0000_4550 { return core::ptr::null(); } // PE

    // Export dir RVA: optional header at nt_off+24; DataDirectory[0] at +112 (PE32+) or +96 (PE32)
    let exp_rva_off = match r16(module_base, nt_off + 24) {
        0x020B => nt_off + 24 + 112, // PE32+
        0x010B => nt_off + 24 + 96,  // PE32
        _ => return core::ptr::null(),
    };
    let exp_rva = r32(module_base, exp_rva_off) as usize;
    if exp_rva == 0 { return core::ptr::null(); }

    let exp          = module_base.add(exp_rva);
    let num_names    = r32(exp, 24) as usize; // NumberOfNames
    let names_rva    = r32(exp, 32) as usize; // AddressOfNames
    let ordinals_rva = r32(exp, 36) as usize; // AddressOfNameOrdinals
    let funcs_rva    = r32(exp, 28) as usize; // AddressOfFunctions

    for i in 0..num_names {
        let name_rva = r32(module_base, names_rva + i * 4) as usize;
        if ascii_icase_eq(module_base.add(name_rva), name) {
            let ord    = r16(module_base, ordinals_rva + i * 2) as usize;
            let fn_rva = r32(module_base, funcs_rva + ord * 4) as usize;
            return module_base.add(fn_rva) as *const c_void;
        }
    }
    core::ptr::null()
}

// ─── Main reflective loader ────────────────────────────────────────────────

/// Map the raw DLL bytes at `raw_dll_base` into a new executable allocation
/// and invoke the image entry point.  Returns the new image base on success,
/// or 0 on any failure.
///
/// Called by the `ReflectiveLoader` export via `CreateRemoteThread`; the
/// injector passes the allocation base (= address of raw bytes) as the
/// thread parameter.
pub unsafe fn load_self(raw_dll_base: usize) -> usize {
    if raw_dll_base == 0 { return 0; }
    let raw = raw_dll_base as *const u8;

    // ── 1. Resolve kernel32 exports via PEB ──────────────────────────────
    let k32 = find_module(b"KERNEL32.DLL");
    if k32.is_null() { return 0; }

    macro_rules! resolve {
        ($name:expr, $ty:ty) => {{
            let p = find_export(k32, $name);
            if p.is_null() { return 0; }
            core::mem::transmute::<_, $ty>(p)
        }};
    }

    let VirtualAlloc:          FnVirtualAlloc          = resolve!(b"VirtualAlloc",          FnVirtualAlloc);
    let LoadLibraryA:          FnLoadLibraryA          = resolve!(b"LoadLibraryA",          FnLoadLibraryA);
    let GetProcAddress:        FnGetProcAddress        = resolve!(b"GetProcAddress",        FnGetProcAddress);
    let FlushInstructionCache: FnFlushInstructionCache = resolve!(b"FlushInstructionCache", FnFlushInstructionCache);

    // ── 2. Parse raw PE headers ──────────────────────────────────────────
    if r16(raw, 0) != 0x5A4D { return 0; }          // MZ
    let nt_off = r32(raw, 60) as usize;
    if r32(raw, nt_off) != 0x0000_4550 { return 0; } // PE
    if r16(raw, nt_off + 24) != 0x020B { return 0; } // PE32+

    // nt_off + 24 = start of PE32+ optional header.
    let opt           = nt_off + 24;
    let entry_rva     = r32(raw, opt + 16)  as usize; // AddressOfEntryPoint
    let image_base    = r64(raw, opt + 24)  as usize; // ImageBase (preferred)
    let size_of_image = r32(raw, opt + 56)  as usize; // SizeOfImage
    let size_of_hdrs  = r32(raw, opt + 60)  as usize; // SizeOfHeaders
    let num_sections  = r16(raw, nt_off + 6) as usize; // FileHeader.NumberOfSections
    let opt_hdr_size  = r16(raw, nt_off + 20) as usize; // FileHeader.SizeOfOptionalHeader

    // DataDirectory entries (each 8 bytes: VA u32 + Size u32)
    let import_rva  = r32(raw, opt + 120) as usize; // dir[1] Import
    let import_sz   = r32(raw, opt + 124) as usize;
    let reloc_rva   = r32(raw, opt + 152) as usize; // dir[5] BaseReloc
    let reloc_sz    = r32(raw, opt + 156) as usize;

    // ── 3. Allocate mapped image ─────────────────────────────────────────
    // Try preferred base first, then any available address.
    let mut new_base = VirtualAlloc(
        image_base as LPVOID,
        size_of_image,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    );
    if new_base.is_null() {
        new_base = VirtualAlloc(
            core::ptr::null_mut(),
            size_of_image,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        );
    }
    if new_base.is_null() { return 0; }
    let nb = new_base as *mut u8;

    // ── 4. Copy PE headers ───────────────────────────────────────────────
    core::ptr::copy_nonoverlapping(raw, nb, size_of_hdrs);

    // ── 5. Copy sections ─────────────────────────────────────────────────
    // Section table starts at: nt_off + 4 (sig) + 20 (FileHdr) + SizeOfOptionalHeader
    let sec_table = raw.add(nt_off + 4 + 20 + opt_hdr_size);
    for i in 0..num_sections {
        let s        = sec_table.add(i * 40); // IMAGE_SECTION_HEADER = 40 bytes
        let vaddr    = r32(s, 12) as usize;   // VirtualAddress
        let raw_sz   = r32(s, 16) as usize;   // SizeOfRawData
        let raw_ptr  = r32(s, 20) as usize;   // PointerToRawData
        // VirtualAlloc zeroes the region, so BSS (raw_sz==0) sections are already handled.
        if raw_sz == 0 || raw_ptr == 0 { continue; }
        core::ptr::copy_nonoverlapping(raw.add(raw_ptr), nb.add(vaddr), raw_sz);
    }

    // ── 6. Base relocations ──────────────────────────────────────────────
    let delta = (new_base as isize).wrapping_sub(image_base as isize);
    if delta != 0 && reloc_rva != 0 && reloc_sz != 0 {
        let reloc_start = nb.add(reloc_rva);
        let reloc_end   = reloc_start.add(reloc_sz);
        let mut block   = reloc_start;

        while block < reloc_end {
            let block_va = r32(block, 0) as usize; // block's page RVA
            let block_sz = r32(block, 4) as usize; // block size in bytes
            if block_sz < 8 { break; }

            let count   = (block_sz - 8) / 2;
            let entries = block.add(8) as *const u16;
            for j in 0..count {
                let e     = *entries.add(j);
                let rtype = e >> 12;
                let off   = (e & 0x0FFF) as usize;
                if rtype == IMAGE_REL_BASED_DIR64 {
                    let patch = nb.add(block_va + off) as *mut isize;
                    *patch = (*patch).wrapping_add(delta);
                }
            }
            block = block.add(block_sz);
        }
    }

    // ── 7. Import table ──────────────────────────────────────────────────
    if import_rva != 0 && import_sz != 0 {
        let import_end = nb.add(import_rva + import_sz);
        let mut desc   = nb.add(import_rva);

        while desc.add(20) <= import_end {
            let name_rva = r32(desc, 12) as usize; // ImportDescriptor.Name RVA
            if name_rva == 0 { break; }            // null-terminator entry

            let dll_handle = LoadLibraryA(nb.add(name_rva) as LPCSTR);
            if !dll_handle.is_null() {
                let oft_rva = r32(desc, 0) as usize; // OriginalFirstThunk
                let iat_rva = r32(desc, 16) as usize; // FirstThunk (IAT)

                // Use OFT if present, otherwise fall back to IAT
                let thunk_rva = if oft_rva != 0 { oft_rva } else { iat_rva };
                let mut thunk = nb.add(thunk_rva) as *mut usize;
                let mut iat   = nb.add(iat_rva)   as *mut usize;

                loop {
                    let tv = *thunk;
                    if tv == 0 { break; }

                    let func = if tv & (1usize << 63) != 0 {
                        // Import by ordinal: low 16 bits
                        GetProcAddress(dll_handle, (tv & 0xFFFF) as LPCSTR)
                    } else {
                        // Import by name: IMAGE_IMPORT_BY_NAME.Name starts 2 bytes in
                        GetProcAddress(dll_handle, nb.add(tv + 2) as LPCSTR)
                    };

                    if !func.is_null() {
                        *iat = func as usize;
                    }
                    thunk = thunk.add(1);
                    iat   = iat.add(1);
                }
            }
            desc = desc.add(20); // sizeof(IMAGE_IMPORT_DESCRIPTOR)
        }
    }

    // ── 8. Flush instruction cache ───────────────────────────────────────
    // Pseudohandle -1 = current process (no GetCurrentProcess call needed).
    FlushInstructionCache((-1isize) as LPVOID, new_base, size_of_image);

    // ── 9. Call entry point (DllMain / _DllMainCRTStartup) ───────────────
    if entry_rva != 0 {
        let entry: DllEntry = core::mem::transmute(nb.add(entry_rva));
        // Pass raw_dll_base as lpReserved so the DLL knows it was loaded reflectively.
        entry(new_base as HMODULE, DLL_PROCESS_ATTACH, raw_dll_base as LPVOID);
    }

    new_base as usize
}
