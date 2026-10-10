//! This file creates the image's __objc_imageinfo section, which tells the
//! Objective-C runtime how the image's Objective-C and Swift code was
//! compiled.
//!
//! Every object file with Objective-C or Swift code has an 8-byte
//! __objc_imageinfo record: a version, which is always 0, and a word of
//! flags, such as the Swift ABI and language versions the code was
//! compiled for and whether its classes' read-only data pointers are
//! signed. The runtime reads one record per image, so the linker can't
//! concatenate the objects' records as it does other sections; it merges
//! them into one (see merge_objc_info) and writes it in a section of its
//! own. An image that dyld doesn't load, such as a -static one, gets no
//! record, because no runtime sets up its Objective-C.

use crate::arch::Target;
use crate::chunks::{ChunkHeader, ChunkId};
use crate::context::Context;
use crate::input_files::ObjcImageInfo;
use crate::macho::*;

/// The merged __objc_imageinfo section: the Objective-C runtime reads
/// exactly one 8-byte record per image.
#[derive(Debug)]
pub struct ObjcImageInfoSection {
    pub hdr: ChunkHeader,
    /// The merged flags word.
    pub flags: u32,
}

impl ObjcImageInfoSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::new(b"__DATA", b"__objc_imageinfo");
        hdr.p2align = 2;
        Self { hdr, flags: 0 }
    }
}

impl Default for ObjcImageInfoSection {
    fn default() -> Self {
        Self::new()
    }
}

/// Merges the objects' __objc_imageinfo records into the image's, cut
/// to the flags an image keeps, a lone record's too (see
/// objc_image_flags and merge_objc_info), in the order ld-prime checks
/// the objects in (see passes::check_objc_flags, which gave the
/// diagnostics). An image no dyld loads (-static, -preload, a kext),
/// whose Objective-C no runtime sets up, gets none from ld-prime.
pub fn create<E: Target>(ctx: &mut Context<E>) {
    let mut objs: Vec<&crate::input_files::ObjectFile> =
        ctx.objs.iter().filter(|o| o.is_reachable && o.objc_image_info.is_some()).collect();
    objs.sort_by_key(|o| o.priority);
    let info = objs
        .iter()
        .filter_map(|o| o.objc_image_info)
        .map(|info| ObjcImageInfo { flags: objc_image_flags(info.flags), ..info })
        .reduce(merge_objc_info);
    let Some(ObjcImageInfo { flags, .. }) = info else { return };
    if ctx.args.without_dyld() {
        return;
    }
    ctx.objc_imageinfo.flags = flags;
    ctx.objc_imageinfo.hdr.segname = crate::chunks::data_seg(ctx);
    ctx.objc_imageinfo.hdr.size = 8;
    ctx.chunks.push(ChunkId::ObjcImageInfo);
}

/// The Objective-C image info of objects whose info so far is `merged`,
/// and of an object with `info`, as ld-prime merges them: the first
/// Swift ABI version given stays, the Swift language version is the
/// oldest given, and the image's categories may have class properties
/// if every object's may (see objc_image_flags). Its class_ro_t
/// pointers are signed if those of every object with classes are; an
/// object without classes has none to sign, and its flag counts only
/// until one with classes comes, and only if it is set.
pub fn merge_objc_info(merged: ObjcImageInfo, info: ObjcImageInfo) -> ObjcImageInfo {
    let (a, b) = (merged.flags, info.flags);
    let abi = if a & 0xff00 != 0 { a & 0xff00 } else { b & 0xff00 };
    let lang = match (a >> 16, b >> 16) {
        (0, lang) | (lang, 0) => lang,
        (a, b) => a.min(b),
    };
    let signed = match (merged.classes, info.classes) {
        (false, false) => a | b,
        (false, true) => b,
        (true, false) => a,
        (true, true) => a & b,
    };
    ObjcImageInfo {
        flags: (lang << 16)
            | abi
            | (a & b & OBJC_HAS_CATEGORY_CLASS_PROPERTIES)
            | (signed & OBJC_SIGNED_CLASS_RO),
        classes: merged.classes || info.classes,
    }
}

/// The Objective-C image info flags of an object as the image keeps
/// them, alone or to merge with others' (see merge_objc_info): the
/// Swift versions, category class properties and signed class_ro_t
/// pointers, which describe the code. ld-prime drops the rest: the
/// simulator bit (0x20) clang sets in a simulator's objects, the bits
/// of the garbage collector the runtime no longer has, and those of
/// dyld's optimizations (0x08 and 0x80), which dyld sets itself.
fn objc_image_flags(flags: u32) -> u32 {
    flags & (0xffff_ff00 | OBJC_HAS_CATEGORY_CLASS_PROPERTIES | OBJC_SIGNED_CLASS_RO)
}

/// Writes the record: its version, 0, and the merged flags. A
/// relocatable output's __objc_imageinfo is written so too.
pub fn copy_buf<E: Target>(ctx: &Context<E>, buf: &mut [u8]) {
    buf[..4].copy_from_slice(&0u32.to_le_bytes());
    buf[4..8].copy_from_slice(&ctx.objc_imageinfo.flags.to_le_bytes());
}
