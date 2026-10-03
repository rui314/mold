//! The record's bytes, laid out as ld-prime writes them.

use super::{Content, DylibRecord, MergeableRecord, OutFixup};
use crate::context::Context;
use crate::macho::*;
use crate::target::Target;

/// libc++'s std::hash of a string (its CityHash64,
/// __murmur2_or_cityhash<size_t, 64>, in the ABI that hashes 4 to 8
/// bytes with a 32-bit shift), which names are hashed with in the
/// record.
fn cityhash64(s: &[u8]) -> u64 {
    const K0: u64 = 0xc3a5_c85c_97cb_3127;
    const K1: u64 = 0xb492_b66f_be98_f273;
    const K2: u64 = 0x9ae1_6a3b_2f90_404f;
    const K3: u64 = 0xc949_d7c7_509e_6557;
    let w64 = |at: usize| u64::from_le_bytes(s[at..at + 8].try_into().unwrap());
    let w32 = |at: usize| u32::from_le_bytes(s[at..at + 4].try_into().unwrap());
    let rot = |v: u64, shift: u32| if shift == 0 { v } else { v.rotate_right(shift) };
    let shift_mix = |v: u64| v ^ (v >> 47);
    let hash16 = |u: u64, v: u64| {
        const MUL: u64 = 0x9ddf_ea08_eb38_2d69;
        let mut a = (u ^ v).wrapping_mul(MUL);
        a ^= a >> 47;
        let mut b = (v ^ a).wrapping_mul(MUL);
        b ^= b >> 47;
        b.wrapping_mul(MUL)
    };
    let weak32 = |at: usize, a: u64, b: u64| {
        let (w, x, y, z) = (w64(at), w64(at + 8), w64(at + 16), w64(at + 24));
        let mut a = a.wrapping_add(w);
        let mut b = rot(b.wrapping_add(a).wrapping_add(z), 21);
        let c = a;
        a = a.wrapping_add(x).wrapping_add(y);
        b = b.wrapping_add(rot(a, 44));
        (a.wrapping_add(z), b.wrapping_add(c))
    };
    let len = s.len();
    let n = len as u64;
    if len <= 16 {
        if len > 8 {
            let a = w64(0);
            let b = w64(len - 8);
            return hash16(a, rot(b.wrapping_add(n), len as u32)) ^ b;
        }
        if len >= 4 {
            let a = w32(0);
            let b = w32(len - 4);
            return hash16(n.wrapping_add((a << 3) as u64), b as u64);
        }
        if len > 0 {
            let (a, b, c) = (s[0] as u32, s[len >> 1] as u32, s[len - 1] as u32);
            let y = a.wrapping_add(b << 8);
            let z = (len as u32).wrapping_add(c << 2);
            return shift_mix((y as u64).wrapping_mul(K2) ^ (z as u64).wrapping_mul(K3))
                .wrapping_mul(K2);
        }
        return K2;
    }
    if len <= 32 {
        let a = w64(0).wrapping_mul(K1);
        let b = w64(8);
        let c = w64(len - 8).wrapping_mul(K2);
        let d = w64(len - 16).wrapping_mul(K0);
        return hash16(
            rot(a.wrapping_sub(b), 43).wrapping_add(rot(c, 30)).wrapping_add(d),
            a.wrapping_add(rot(b ^ K3, 20)).wrapping_sub(c).wrapping_add(n),
        );
    }
    if len <= 64 {
        let mut z = w64(24);
        let mut a = w64(0).wrapping_add(n.wrapping_add(w64(len - 16)).wrapping_mul(K0));
        let mut b = rot(a.wrapping_add(z), 52);
        let mut c = rot(a, 37);
        a = a.wrapping_add(w64(8));
        c = c.wrapping_add(rot(a, 7));
        a = a.wrapping_add(w64(16));
        let vf = a.wrapping_add(z);
        let vs = b.wrapping_add(rot(a, 31)).wrapping_add(c);
        a = w64(16).wrapping_add(w64(len - 32));
        z = z.wrapping_add(w64(len - 8));
        b = rot(a.wrapping_add(z), 52);
        c = rot(a, 37);
        a = a.wrapping_add(w64(len - 24));
        c = c.wrapping_add(rot(a, 7));
        a = a.wrapping_add(w64(len - 16));
        let wf = a.wrapping_add(z);
        let ws = b.wrapping_add(rot(a, 31)).wrapping_add(c);
        let r = shift_mix(
            vf.wrapping_add(ws).wrapping_mul(K2).wrapping_add(wf.wrapping_add(vs).wrapping_mul(K0)),
        );
        return shift_mix(r.wrapping_mul(K0).wrapping_add(vs)).wrapping_mul(K2);
    }
    let mut x = w64(len - 40);
    let mut y = w64(len - 16).wrapping_add(w64(len - 56));
    let mut z = hash16(w64(len - 48).wrapping_add(n), w64(len - 24));
    let mut v = weak32(len - 64, n, z);
    let mut w = weak32(len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(w64(0));
    let mut pos = 0;
    let mut left = (len - 1) & !63;
    loop {
        x = rot(x.wrapping_add(y).wrapping_add(v.0).wrapping_add(w64(pos + 8)), 37)
            .wrapping_mul(K1);
        y = rot(y.wrapping_add(v.1).wrapping_add(w64(pos + 48)), 42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(w64(pos + 40));
        z = rot(z.wrapping_add(w.0), 33).wrapping_mul(K1);
        v = weak32(pos, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak32(pos + 32, z.wrapping_add(w.1), y.wrapping_add(w64(pos + 16)));
        std::mem::swap(&mut z, &mut x);
        pos += 64;
        left -= 64;
        if left == 0 {
            break;
        }
    }
    hash16(
        hash16(v.0, w.0).wrapping_add(shift_mix(y).wrapping_mul(K1)).wrapping_add(z),
        hash16(v.1, w.1).wrapping_add(x),
    )
}

/// A name's hash as the record keeps it: its length in the top 20
/// bits, the low 44 of its hash below.
fn name_hash(name: &[u8]) -> u64 {
    (name.len() as u64) << 44 | cityhash64(name) & ((1 << 44) - 1)
}

/// The record being written, and the strings its records point at,
/// which go in a pool after them: each with the place of its record.
struct Writer {
    out: Vec<u8>,
    cstrings: Vec<(usize, Vec<u8>)>,
}

impl Writer {
    fn put32(&mut self, at: usize, v: u32) {
        self.out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put64(&mut self, at: usize, v: u64) {
        self.out[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// Pads the record to a multiple of `align`, and returns its size.
    fn align(&mut self, align: usize) -> usize {
        let len = self.out.len().next_multiple_of(align);
        self.out.resize(len, 0);
        len
    }

    /// Adds `size` zero bytes 8-aligned, and returns where they start.
    fn reserve(&mut self, size: usize) -> usize {
        let at = self.align(8);
        self.out.resize(at + size, 0);
        at
    }

    /// A table's place in the header: its offset and its count (or
    /// size) at `field`.
    fn table(&mut self, field: usize, off: usize, count: usize) {
        self.put32(field, off as u32);
        self.put32(field + 4, count as u32);
    }

    /// Appends a NUL-terminated string, and points the 16-byte record at
    /// `at` (CStringRO_1, SymbolString) at it: its offset from the
    /// record, then `tail`.
    fn add_string(&mut self, at: usize, s: &[u8], tail: u64) {
        let pos = self.out.len();
        self.out.extend_from_slice(s);
        self.out.push(0);
        self.put64(at, (pos as i64 - at as i64) as u64);
        self.put64(at + 8, tail);
    }
}

/// What the record leaves to fill once it has its place in the file:
/// the content offsets of the entries whose bytes are the image's (at
/// which offset of the record, of which file offset), and the content
/// pool's offset.
pub(super) struct Serialized {
    pub bytes: Vec<u8>,
    pub image_contents: Vec<(u32, u64)>,
    pub pool_offset: u32,
}

impl MergeableRecord {
    /// The record's bytes, laid out as ld-prime writes them: the
    /// header, the entries, then each 8-aligned after the one before the
    /// fixups, large addends, custom sections, linker options (none),
    /// name records, this dylib's info, its dependencies', the debug
    /// notes, the C string pool, the name pool and the content pool.
    pub(super) fn serialize<E: Target>(&self, ctx: &Context<E>) -> Serialized {
        use crate::mergeable::{DEBUG_INFO_SIZE, ENTRY_SIZE, HEADER_SIZE};
        let mut w = Writer { out: vec![0; HEADER_SIZE], cstrings: Vec::new() };
        self.write_header::<E>(&mut w, ctx);
        let entries_at = w.reserve(self.entries.len() * ENTRY_SIZE);
        w.table(0x60, entries_at, self.entries.len());
        self.write_fixups(&mut w);
        self.write_sections(&mut w);
        let options_at = w.align(8);
        w.table(0x80, options_at, 0);
        let names: Vec<&[u8]> = self.entries.iter().filter_map(|e| e.name).collect();
        let names_at = w.reserve(names.len() * 16);
        w.table(0x88, names_at, names.len());
        self.write_dylib_infos(&mut w);
        let debug_at = w.align(8);
        for d in &self.debug {
            write_debug_record(&mut w, d);
        }
        w.table(0xbc, debug_at, self.debug.len());
        w.put32(0xc4, (self.debug.len() * DEBUG_INFO_SIZE) as u32);

        let pool = w.align(8);
        // (An empty string is none: its record stays zero.)
        for (at, s) in std::mem::take(&mut w.cstrings) {
            if !s.is_empty() {
                w.add_string(at, &s, s.len() as u64);
            }
        }
        w.table(0xa0, pool, w.out.len() - pool);
        let pool = w.align(8);
        for (i, name) in names.iter().enumerate() {
            w.add_string(names_at + i * 16, name, name_hash(name));
        }
        w.table(0x90, pool, w.out.len() - pool);

        let (image_contents, pool_offset) = self.write_entries(&mut w, entries_at);
        let total = w.out.len() as u32;
        w.put32(0x5c, total);
        Serialized { bytes: w.out, image_contents, pool_offset }
    }

    /// The header's identification: the magic, the versions of the
    /// formats, the largest kinds used, the target and the flags.
    fn write_header<E: Target>(&self, w: &mut Writer, ctx: &Context<E>) {
        w.out[..8].copy_from_slice(crate::mergeable::MAGIC);
        w.out[8..10].copy_from_slice(&3u16.to_le_bytes());
        w.out[10] = 2;
        w.out[11] = 2;
        w.out[12] = self.entries.iter().map(|e| e.kind).max().unwrap_or(0);
        w.out[13] = self.entries.iter().map(|e| e.content_type).max().unwrap_or(0);
        let max_kind = |arch: bool| {
            let kinds = self.fixups.iter().map(|(f, _, _)| f.kind);
            kinds.filter(|&k| (k >= 0x80) == arch).max().unwrap_or(0)
        };
        w.out[14..16].copy_from_slice(&max_kind(false).to_le_bytes());
        w.out[16..18].copy_from_slice(&max_kind(true).to_le_bytes());
        w.put32(0x14, E::CPUTYPE);
        w.put32(0x18, E::CPUSUBTYPE);
        w.put32(0x1c, ctx.args.platform);
        w.put32(0x20, ctx.args.platform_minos);
        w.put32(0x24, ctx.args.platform_sdk);
        w.put64(0x28, self.flags);
    }

    /// The fixups, and the addends too large for theirs, each once.
    fn write_fixups(&self, w: &mut Writer) {
        use crate::mergeable::FIXUP_SIZE;
        let at = w.reserve(self.fixups.len() * FIXUP_SIZE);
        w.table(0x68, at, self.fixups.len());
        let mut large: Vec<i64> = Vec::new();
        for (i, &(f, target, from)) in self.fixups.iter().enumerate() {
            let at = at + i * FIXUP_SIZE;
            let (w2, w3) = encode_fixup(&f, from, &mut large);
            w.put32(at, f.offset);
            w.put32(at + 4, target);
            w.put32(at + 8, w2);
            w.put32(at + 12, w3);
        }
        let at = w.align(8);
        for v in &large {
            w.out.extend_from_slice(&v.to_le_bytes());
        }
        w.table(0x70, at, large.len());
    }

    /// The custom sections: the protection of the segment, the flags,
    /// and the names, NUL-terminated.
    fn write_sections(&self, w: &mut Writer) {
        let at = w.align(8);
        for s in &self.sections {
            let mut rec = [0u8; crate::mergeable::SECTION_SIZE];
            let prot: u32 = if s.segname == bytes_to_name(b"__TEXT") { 5 } else { 3 };
            rec[0..4].copy_from_slice(&prot.to_le_bytes());
            rec[4..8].copy_from_slice(&s.flags.to_le_bytes());
            rec[8..24].copy_from_slice(&s.segname);
            rec[26..42].copy_from_slice(&s.sectname);
            w.out.extend_from_slice(&rec);
        }
        w.table(0x78, at, self.sections.len());
    }

    /// This dylib's identity, then each of its dependencies'.
    fn write_dylib_infos(&self, w: &mut Writer) {
        let at = w.align(8);
        write_dylib_record(w, &self.own);
        w.table(0xa8, at, w.out.len() - at);
        let at = w.align(8);
        for d in &self.deps {
            write_dylib_record(w, d);
        }
        let size = w.align(8) - at;
        w.table(0xb0, at, self.deps.len());
        w.put32(0xb8, size as u32);
    }

    /// The entries, and the content pool after everything else: the
    /// bytes the image has none of, each at its alignment. Returns the
    /// entries whose bytes are the image's, and the pool's offset.
    fn write_entries(&self, w: &mut Writer, entries_at: usize) -> (Vec<(u32, u64)>, u32) {
        use crate::mergeable::ENTRY_SIZE;
        let pool_align = (self.entries.iter())
            .filter(|e| matches!(e.content, Content::Pool(_)))
            .map(|a| 1usize << a.p2align)
            .fold(16, usize::max);
        let pool = w.align(pool_align);
        let mut image = Vec::new();
        let mut names = 0u32;
        for (i, entry) in self.entries.iter().enumerate() {
            let at = entries_at + i * ENTRY_SIZE;
            let content: i32 = match entry.content {
                Content::None => -1,
                Content::Image(fileoff) => {
                    image.push(((at + 0x18) as u32, fileoff));
                    0
                }
                Content::Pool(bytes) => {
                    let size = w.out.len() - pool;
                    let align = 1usize << entry.p2align;
                    let mut off = (size & !(align - 1)) + entry.modulus as usize;
                    if off < size {
                        off += align;
                    }
                    w.out.resize(pool + off, 0);
                    w.out.extend_from_slice(bytes);
                    off as i32
                }
            };
            let name = match entry.name {
                Some(_) => {
                    names += 1;
                    names - 1
                }
                None => 0xff_ffff,
            };
            w.put32(at, i as u32);
            w.put32(at + 4, entry.fixups.len() as u32);
            w.put32(at + 8, self.first_fixup[i]);
            w.put32(at + 12, name);
            w.put32(at + 16, entry.flags());
            w.put32(at + 20, entry.size);
            w.put32(at + 24, content as u32);
            w.out[at + 0x1c] = entry.dylib.unwrap_or(0xff);
            w.out[at + 0x1d] = entry.p2align;
            w.out[at + 0x1e..at + 0x20].copy_from_slice(&entry.modulus.to_le_bytes());
            w.out[at + 0x20..at + 0x22].copy_from_slice(&entry.debug.to_le_bytes());
        }
        w.table(0x98, pool, w.out.len() - pool);
        (image, pool as u32)
    }
}

/// A DebugNoteFileInfoRO_2: the object's modification time and CPU
/// subtype (its N_OSO's value and section), then its source directory
/// and name (N_SO), its path (N_OSO), and this dylib's install name.
fn write_debug_record(w: &mut Writer, d: &super::DebugRecord) {
    let at = w.out.len();
    w.out.resize(at + crate::mergeable::DEBUG_INFO_SIZE, 0);
    w.put32(at, d.mtime);
    w.out[at + 4] = d.cpusubtype;
    let strings = [&d.source_dir, &d.source_name, &d.object_path, &d.install_name];
    for (k, s) in strings.into_iter().enumerate() {
        w.cstrings.push((at + 8 + 16 * k, s.clone()));
    }
}

/// A DylibFileInfoRO_2: the versions, the install name, the platforms,
/// and the lists of re-exports and allowed clients (and a third
/// ld-prime leaves empty), each of strings for the pool.
fn write_dylib_record(w: &mut Writer, d: &DylibRecord) {
    let at = w.out.len();
    w.out.resize(at + crate::mergeable::DYLIB_INFO_SIZE, 0);
    w.put32(at, d.current_version);
    w.put32(at + 4, d.compatibility_version);
    w.cstrings.push((at + 8, d.install_name.clone()));
    if !d.platforms.is_empty() {
        let off = w.out.len() - at;
        for &p in &d.platforms {
            w.out.extend_from_slice(&p.to_le_bytes());
        }
        w.align(8);
        w.table(at + 0x28, off, d.platforms.len());
    }
    for (field, list) in [(0x30, &d.reexports), (0x38, &d.clients)] {
        if list.is_empty() {
            continue;
        }
        w.table(at + field, w.out.len() - at, list.len());
        for s in list {
            let pos = w.out.len();
            w.out.resize(pos + 16, 0);
            w.cstrings.push((pos, s.clone()));
        }
    }
}

/// A fixup's third and fourth words: its kind and the addend or the
/// index of a large one, and what its kind keeps besides (see
/// mergeable::extras_usage).
fn encode_fixup(f: &OutFixup, from: u32, large: &mut Vec<i64>) -> (u32, u32) {
    let usage = crate::mergeable::extras_usage(f.kind);
    let kind = f.kind as u32;
    let fits = |bits: u32| f.addend >= -(1 << (bits - 1)) && f.addend < 1 << (bits - 1);
    let large_index = |large: &mut Vec<i64>| {
        let i = large.iter().position(|&v| v == f.addend).unwrap_or_else(|| {
            large.push(f.addend);
            large.len() - 1
        });
        kind | 0x400 | (i as u32) << 11
    };
    match usage {
        2 if fits(32) => (kind, f.addend as u32),
        2 => (large_index(large), 0),
        4..=8 => {
            let extras = (f.scale as u32) << 8;
            if fits(32) {
                let a = f.addend as u32;
                (kind | (a & 0x1f_ffff) << 11, extras | (a >> 21) << 16)
            } else {
                (large_index(large), extras)
            }
        }
        _ => {
            let w3 = if usage == 1 { from } else { 0 };
            if fits(21) {
                (kind | ((f.addend as u32) & 0x1f_ffff) << 11, w3)
            } else {
                (large_index(large), w3)
            }
        }
    }
}
