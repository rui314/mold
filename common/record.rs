//! Records of object files, read, written and viewed in their file
//! representation. A record's fields are bytes, or integers backed by
//! bytes that read and write in the file's byte order, so a record has
//! alignment one: one in a possibly unaligned archive member is read in
//! place, without depending on the host's byte order.

/// A record stored in its file representation.
///
/// # Safety
///
/// Implementations must have alignment one, contain no padding or references,
/// and accept every bit pattern. These requirements let records in possibly
/// unaligned archive members be read and written without host dependencies.
pub unsafe trait FileRecord: Clone + Copy + Default + Send + Sync + 'static {
    fn parse(bytes: &[u8]) -> Self {
        const { assert!(align_of::<Self>() == 1) };
        assert!(bytes.len() >= size_of::<Self>());
        // SAFETY: the trait guarantees that every bit pattern is valid, and
        // the length check proves that a complete record is available.
        unsafe { bytes.as_ptr().cast::<Self>().read_unaligned() }
    }

    fn write(&self, buf: &mut [u8]) {
        assert!(buf.len() >= size_of::<Self>());
        // SAFETY: `buf` has room for the complete record, and copying bytes
        // does not depend on its alignment.
        unsafe {
            std::ptr::copy_nonoverlapping(
                std::ptr::from_ref(self).cast::<u8>(),
                buf.as_mut_ptr(),
                size_of::<Self>(),
            );
        }
    }

    /// The record's bytes, as a file holds them.
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: a record has no padding, so all of its bytes are
        // initialized.
        unsafe {
            std::slice::from_raw_parts(std::ptr::from_ref(self).cast::<u8>(), size_of::<Self>())
        }
    }

    fn write_all(records: &[Self], buf: &mut [u8]) {
        for (record, slot) in records.iter().zip(buf.chunks_exact_mut(size_of::<Self>())) {
            record.write(slot);
        }
    }
}

/// Views one record directly in its file representation.
pub fn record_from_bytes<R: FileRecord>(data: &[u8]) -> &R {
    const { assert!(align_of::<R>() == 1) };
    assert!(data.len() >= size_of::<R>());
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. The length check proves that one complete record is present.
    unsafe { &*data.as_ptr().cast() }
}

/// Views records directly in their file representation.
pub fn records_from_bytes<R: FileRecord>(data: &[u8]) -> &[R] {
    const { assert!(align_of::<R>() == 1 && size_of::<R>() != 0) };
    let size = size_of::<R>();
    assert!(data.len().is_multiple_of(size));
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. The resulting slice covers exactly `data`.
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), data.len() / size) }
}

/// Mutably views records directly in their file representation.
pub fn records_from_bytes_mut<R: FileRecord>(data: &mut [u8]) -> &mut [R] {
    const { assert!(align_of::<R>() == 1 && size_of::<R>() != 0) };
    let size = size_of::<R>();
    assert!(data.len().is_multiple_of(size));
    // SAFETY: FileRecord requires alignment one and every bit pattern to be
    // valid. `data` is exclusively borrowed for the returned slice.
    unsafe { std::slice::from_raw_parts_mut(data.as_mut_ptr().cast(), data.len() / size) }
}
