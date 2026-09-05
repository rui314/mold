//! Small helpers shared across the linker.

// Utility functions

pub mod compress;
pub mod concurrent_map;
pub mod demangle;
pub mod glob;
pub mod hyperloglog;
pub mod perf;
pub(crate) mod siphash;
pub mod tar;

/// Requests transparent huge pages for a mapped byte range on Linux-based
/// targets.
///
/// # Safety
///
/// `data..data + size` must describe a live mapping.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) unsafe fn madvise_hugepage(data: *mut u8, size: usize) {
    // SAFETY: guaranteed by the caller; MADV_HUGEPAGE is only a kernel hint.
    let _ = unsafe { libc::madvise(data.cast(), size, libc::MADV_HUGEPAGE) };
}

/// No-op on targets that do not support Linux's `MADV_HUGEPAGE` advice.
///
/// # Safety
///
/// Kept identical to the supported-target signature.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) unsafe fn madvise_hugepage(_data: *mut u8, _size: usize) {}

/// Requests transparent huge pages for the whole pages strictly inside an
/// allocation, leaving possible allocator metadata in its boundary pages
/// untouched.
///
/// # Safety
///
/// `data..data + size` must describe a live allocation.
#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) unsafe fn madvise_hugepage_interior(data: *const u8, size: usize) {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size == 0 || page_size <= 0 {
        return;
    }

    let page_size = page_size as usize;
    let start = data.addr();
    let Some(end) = start.checked_add(size) else {
        return;
    };
    let Some(rounded) = start.checked_add(page_size - 1) else {
        return;
    };
    let first_page = rounded / page_size * page_size;
    let last_page = end / page_size * page_size;
    if first_page < last_page {
        // SAFETY: these complete pages lie strictly inside the caller's
        // allocation.
        unsafe { madvise_hugepage(first_page as *mut u8, last_page - first_page) };
    }
}

/// No-op on targets that do not support Linux's `MADV_HUGEPAGE` advice.
///
/// # Safety
///
/// Kept identical to the supported-target signature.
#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) unsafe fn madvise_hugepage_interior(_data: *const u8, _size: usize) {}

/// Rounds `value` up to a multiple of `align`, which must be zero or a power
/// of two. Zero means "no alignment".
#[inline]
pub fn align_to(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Rounds `value` down to a multiple of `align`, which must be a power of two.
pub fn align_down(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

/// Returns bit `pos` of `value`.
pub fn bit(value: u64, pos: u32) -> u64 {
    (value >> pos) & 1
}

// Returns [hi:lo] bits of val.
#[inline]
pub fn bits(value: u64, hi: u32, lo: u32) -> u64 {
    (value >> lo) & ((1u64 << (hi - lo + 1)) - 1)
}

// Cast val to a signed N bit integer.
// For example, sign_extend(x, 32) == (i32)x for any integer x.
pub fn sign_extend(value: u64, n: u32) -> i64 {
    ((value << (64 - n)) as i64) >> (64 - n)
}

/// Whether `value` is representable as a signed `n`-bit integer.
pub fn is_int(value: i64, n: u32) -> bool {
    sign_extend(value as u64, n) == value
}

/// Writes a NUL-terminated string and returns the number of bytes written.
pub fn write_cstr(buf: &mut [u8], s: &[u8]) -> usize {
    buf[..s.len()].copy_from_slice(s);
    buf[s.len()] = 0;
    s.len() + 1
}

/// Returns the NUL-terminated string starting at `offset` in a string table.
/// The result excludes the terminator. A missing terminator yields the rest
/// of the table.
#[inline]
pub fn cstr_at(table: &[u8], offset: usize) -> &[u8] {
    let rest = table.get(offset..).unwrap_or(&[]);
    if rest.is_empty() {
        return rest;
    }

    // ELF string tables normally end in NUL, so strlen cannot read past the
    // table. Keep the bounded path for malformed tables without a terminator.
    let end = unsafe {
        if table.last() == Some(&0) {
            libc::strlen(rest.as_ptr().cast())
        } else {
            libc::strnlen(rest.as_ptr().cast(), rest.len())
        }
    };
    &rest[..end]
}

/// Returns the position of `byte` in `data`.
#[inline]
pub fn find_byte(byte: u8, data: &[u8]) -> Option<usize> {
    // SAFETY: memchr reads at most data.len() bytes from the slice. A
    // non-null result therefore points into the same allocation.
    let ptr = unsafe { libc::memchr(data.as_ptr().cast(), byte.into(), data.len()) }.cast::<u8>();
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { ptr.offset_from(data.as_ptr()) as usize })
    }
}

/// Appends `value` in unsigned LEB128 encoding.
pub fn encode_uleb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Appends `value` in signed LEB128 encoding.
pub fn encode_sleb(out: &mut Vec<u8>, mut value: i64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let negative = byte & 0x40 != 0;
        if (value == 0 && !negative) || (value == -1 && negative) {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Overwrites an existing unsigned LEB128 value in place, keeping its length.
pub fn overwrite_uleb(buf: &mut [u8], mut value: u64) {
    let mut i = 0;
    while buf[i] & 0x80 != 0 {
        buf[i] = 0x80 | (value & 0x7f) as u8;
        value >>= 7;
        i += 1;
    }
    buf[i] = (value & 0x7f) as u8;
}

/// Reads an unsigned LEB128 value, advancing `bytes` past it.
#[inline]
pub fn read_uleb(bytes: &mut &[u8]) -> u64 {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let (&byte, rest) = bytes.split_first().expect("truncated LEB128");
        *bytes = rest;
        if shift < 64 {
            value |= ((byte & 0x7f) as u64) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

/// Reads a signed LEB128 value, advancing `bytes` past it.
#[inline]
pub fn read_sleb(bytes: &mut &[u8]) -> i64 {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let (&byte, rest) = bytes.split_first().expect("truncated LEB128");
        *bytes = rest;
        if shift < 64 {
            value |= ((byte & 0x7f) as u64) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return if shift < 64 {
                sign_extend(value, shift)
            } else {
                value as i64
            };
        }
    }
}

/// Fills `buf` with random bytes from the operating system.
pub fn random_bytes(buf: &mut [u8]) {
    use std::io::Read;
    let mut file = std::fs::File::open("/dev/urandom").expect("cannot open /dev/urandom");
    file.read_exact(buf).expect("cannot read /dev/urandom");
}

/// Leaks a value for the rest of the process's lifetime.
///
/// Input files, symbol names and a few other objects must outlive every
/// data structure of a link, and the process exits as soon as the link is
/// done, so never freeing them is both simplest and cheapest.
pub fn leak<T>(value: T) -> &'static T {
    Box::leak(Box::new(value))
}

/// Leaks a byte string for the rest of the process's lifetime.
pub fn leak_bytes(bytes: Vec<u8>) -> &'static [u8] {
    Vec::leak(bytes)
}

// Returns the path of the mold executable itself
#[cfg(target_os = "freebsd")]
pub fn self_path() -> std::path::PathBuf {
    // /proc may not be mounted on FreeBSD. The proper way to get the
    // current executable's path is to use sysctl(2).
    let mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PATHNAME,
        -1,
    ];
    let mut size = 0;
    // SAFETY: the MIB and output-size pointers are valid.
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null(),
            0,
        )
    } == -1
    {
        panic!(
            "cannot get current executable path: {}",
            std::io::Error::last_os_error()
        );
    }

    let mut path = vec![0u8; size];
    // SAFETY: sysctl writes at most `size` bytes to the allocated buffer.
    if unsafe {
        libc::sysctl(
            mib.as_ptr(),
            mib.len() as u32,
            path.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null(),
            0,
        )
    } == -1
    {
        panic!(
            "cannot get current executable path: {}",
            std::io::Error::last_os_error()
        );
    }
    path.truncate(size);
    if path.last() == Some(&0) {
        path.pop();
    }

    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(path).into()
}

#[cfg(not(target_os = "freebsd"))]
pub fn self_path() -> std::path::PathBuf {
    std::fs::read_link("/proc/self/exe").expect("cannot read /proc/self/exe")
}

/// Normalizes a path lexically, resolving `.` and `..` components without
/// consulting the file system.
pub fn path_clean(path: &str) -> String {
    use std::path::{Component, Path, PathBuf};
    let mut out = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::RootDir) => {}
                None | Some(Component::ParentDir) => out.push(".."),
                _ => {
                    out.pop();
                }
            },
            other => out.push(other),
        }
    }
    let s = out.to_string_lossy().into_owned();
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

/// Returns the directory part of a path, as `dirname(1)` would.
pub fn path_dirname(path: &str) -> String {
    match std::path::Path::new(path).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_string_lossy().into_owned(),
        _ => ".".to_string(),
    }
}

/// Returns the file name part of a path.
pub fn path_filename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Formats a byte string for diagnostics, replacing invalid UTF-8.
pub fn display(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_roundtrip() {
        for &value in &[0u64, 1, 127, 128, 300, u64::MAX] {
            let mut buf = Vec::new();
            encode_uleb(&mut buf, value);
            let mut slice = buf.as_slice();
            assert_eq!(read_uleb(&mut slice), value);
            assert!(slice.is_empty());
        }
        for &value in &[0i64, -1, 63, 64, -64, -65, i64::MIN, i64::MAX] {
            let mut buf = Vec::new();
            encode_sleb(&mut buf, value);
            let mut slice = buf.as_slice();
            assert_eq!(read_sleb(&mut slice), value);
        }
    }

    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0xff, 8), -1);
        assert_eq!(sign_extend(0x7f, 8), 127);
        assert!(is_int(-128, 8));
        assert!(!is_int(128, 8));
    }

    #[test]
    fn clean_paths() {
        assert_eq!(path_clean("a/./b/../c"), "a/c");
        assert_eq!(path_clean("/a/../.."), "/");
        assert_eq!(path_clean("../a"), "../a");
        assert_eq!(path_clean("../../a/b"), "../../a/b");
        assert_eq!(path_clean("a/../../b"), "../b");
        assert_eq!(path_clean("a/b/../../../c"), "../c");
        assert_eq!(path_clean(".."), "..");
        assert_eq!(path_clean("/.."), "/");
    }
}
