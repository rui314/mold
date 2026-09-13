//! Merged sections: constant pools such as string literals, deduplicated
//! across all input files.
//!
//! Fragments are deduplicated through a concurrent hash table sized for
//! 2/3 occupancy from an estimate of their number, and the output layout
//! follows the table: shard by shard, in the deterministic bucket order
//! of [`FrozenMap::sorted_entries`].

use std::sync::atomic::Ordering;
use std::sync::RwLock;

use bstr::BStr;
use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkHeader;
use crate::cmdline::Args;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::display_file;
use crate::input_sections::{MergeInfo, SectionFragment, SectionRef};
use crate::out;
use crate::output_file::split_at_offsets;
use crate::util::align_to;
use crate::util::concurrent_map::{ConcurrentMap, EntryId, FrozenMap, NUM_SHARDS};
use crate::util::hyperloglog::HyperLogLog;
use crate::util::perf::Timers;

/// Index of a merged section in `Context::merged_sections`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MergedSectionId(pub u32);

impl MergedSectionId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A conversion worker's bounded cache. Keys are copied because the shared
/// section vector can move as other workers add sections; IDs remain stable.
#[derive(Default)]
pub struct MergedSectionCache {
    entries: Vec<(MergedSectionKey, MergedSectionId)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MergedSectionKey {
    name: &'static BStr,
    flags: u64,
    sh_type: u32,
    entsize: u64,
}

/// The deterministically ordered fragments and the two output sizes of one
/// hash-table shard.
#[derive(Debug)]
struct ShardLayout {
    near_size: u64,
    far_size: u64,
    fragments: Vec<EntryId>,
}

// MergedSection represents an output section containing a constant pool such
// as string literals or floating-point constants.
#[derive(Debug)]
pub struct MergedSection<E: Layout> {
    pub hdr: ChunkHeader<E>,

    /// The input sections merged into this one.
    pub members: Vec<SectionRef>,

    /// The fragment map while fragments are being inserted.
    pub map: ConcurrentMap<SectionFragment>,

    /// The fragments once insertion is done.
    pub fragments: FrozenMap<SectionFragment>,
    pub estimation: u64,
    pub resolved: bool,

    /// The fragments in deterministic order within each hash-table shard.
    shards: Vec<ShardLayout>,

    /// Starts of the 32-bit and unrestricted halves of every shard, followed
    /// by the section size.
    shard_offsets: Vec<u64>,
}

/// A direct reference to one input member while allocated merged sections
/// are resolved in parallel. C++ mold keeps the same pointers in
/// `MergedSection::members`; Rust keeps the durable section references there
/// and borrows the stable objects directly for this phase.
pub struct ResolveMember<'a> {
    pub merge_info: &'a mut MergeInfo,
    pub data: &'static [u8],
    pub filename: &'a str,
    pub archive_name: &'a std::path::Path,
    pub name: &'static BStr,
}

pub struct ResolveOptions<'a> {
    pub allocated_only: bool,
    pub gc_sections: bool,
    pub comment: Option<MergedSectionId>,
    pub cmdline_args: &'a [std::ffi::OsString],
    pub timers: &'a Timers,
}

struct BackgroundMember {
    reference: SectionRef,
    info: MergeInfo,
    data: &'static [u8],
    filename: std::sync::Arc<str>,
    archive_name: &'static std::path::Path,
    name: &'static BStr,
}

/// Owns non-allocated merging state while foreground passes use the context.
/// Input bytes are immutable and remain mapped for the entire link.
pub struct BackgroundMerge<E: Layout> {
    sections: Vec<MergedSection<E>>,
    members: Vec<Vec<BackgroundMember>>,
}

impl<E: Arch> BackgroundMerge<E> {
    pub fn prepare(ctx: &Context<E>) -> Self {
        let sections: Vec<_> = ctx
            .merged_sections
            .iter()
            .map(|section| {
                let hdr = &section.hdr;
                let mut copy = MergedSection::new(
                    hdr.name,
                    hdr.shdr.sh_flags.get(),
                    hdr.shdr.sh_type.get(),
                    hdr.shdr.sh_entsize.get(),
                );
                if !section.resolved {
                    copy.members = section.members.clone();
                }
                // Allocated sections have already been resolved in the foreground.
                copy.resolved = section.resolved;
                copy
            })
            .collect();
        let mut members: Vec<Vec<BackgroundMember>> =
            (0..sections.len()).map(|_| Vec::new()).collect();
        for file in &ctx.objs {
            let mut filename = None;
            for info in file.merge_infos() {
                if sections[info.parent.index()].resolved {
                    continue;
                }
                let filename = filename.get_or_insert_with(|| {
                    std::sync::Arc::<str>::from(file.base.filename.as_ref())
                });
                let input = file.section_at(info.shndx);
                members[info.parent.index()].push(BackgroundMember {
                    reference: SectionRef {
                        file: file.id(),
                        shndx: info.shndx,
                    },
                    info: info.clone(),
                    data: input.contents(),
                    filename: filename.clone(),
                    archive_name: file.archive_name,
                    name: input.name(file),
                });
            }
        }
        Self { sections, members }
    }

    pub fn run(mut self, options: ResolveOptions<'_>) -> Self {
        let mut members: Vec<Vec<_>> = self
            .members
            .iter_mut()
            .map(|members| {
                members
                    .iter_mut()
                    .map(|member| ResolveMember {
                        merge_info: &mut member.info,
                        data: member.data,
                        filename: &member.filename,
                        archive_name: member.archive_name,
                        name: member.name,
                    })
                    .collect()
            })
            .collect();
        resolve_sections(&mut self.sections, &mut members, options);
        self
    }

    pub fn finish(self, ctx: &mut Context<E>) {
        for (i, (mut section, members)) in self.sections.into_iter().zip(self.members).enumerate() {
            if ctx.merged_sections[i].resolved {
                continue;
            }
            let size = section.hdr.shdr.sh_size.get();
            let align = section.hdr.shdr.sh_addralign.get();
            // Preserve header bookkeeping done by foreground passes.
            std::mem::swap(&mut section.hdr, &mut ctx.merged_sections[i].hdr);
            section.hdr.shdr.sh_size.set(size);
            section.hdr.shdr.sh_addralign.set(align);
            ctx.merged_sections[i] = section;
            for member in members {
                let file = &mut ctx.objs[member.reference.file.index()];
                let (info, _) = file
                    .sections
                    .merge_info_with_section_mut(member.reference.shndx as usize)
                    .unwrap();
                *info = member.info;
            }
        }
    }
}

fn merged_output_name(
    args: &Args,
    name: &'static BStr,
    flags: u64,
    entsize: u64,
    addralign: u64,
) -> &'static BStr {
    if args.relocatable && !args.relocatable_merge_sections {
        return name;
    }
    if !args.unique.is_empty() && args.unique.find(name) != -1 {
        return name;
    }

    // GCC seems to create sections named ".rodata.strN.<mangled-symbol-name>.M"
    // or ".rodata.cst.<mangled-symbol-name.cstN". We want to eliminate the
    // symbol name part from the section name.
    if name.starts_with(b".rodata.") {
        let name2 = if flags & SHF_STRINGS as u64 != 0 {
            format!(".rodata.str{entsize}.{addralign}")
        } else {
            format!(".rodata.cst{entsize}")
        };
        if name == name2.as_bytes() {
            return name;
        }
        return BStr::new(crate::util::leak_bytes(name2.into_bytes()));
    }
    name
}

impl<E: Layout> MergedSection<E> {
    fn new(name: &'static BStr, flags: u64, sh_type: u32, entsize: u64) -> MergedSection<E> {
        let mut hdr = ChunkHeader::<E>::with_name(name, sh_type, flags);
        hdr.shdr.sh_entsize.set(entsize);
        MergedSection {
            hdr,
            members: Vec::new(),
            map: ConcurrentMap::default(),
            fragments: FrozenMap::default(),
            estimation: 0,
            resolved: false,
            shards: Vec::new(),
            shard_offsets: Vec::new(),
        }
    }

    /// Finds or creates the merged section an input section belongs to.
    /// Files are converted in parallel, and nearly every lookup finds an
    /// existing section, so the list is only write-locked to add one.
    pub fn get_instance(
        args: &Args,
        sections: &RwLock<Vec<MergedSection<E>>>,
        name: &'static BStr,
        shdr: &ElfShdr<E>,
        cache: &mut MergedSectionCache,
    ) -> Option<MergedSectionId> {
        let sh_flags = shdr.sh_flags.get();
        if sh_flags & SHF_MERGE as u64 == 0 {
            return None;
        }
        let addralign = shdr.sh_addralign.get().max(1);
        let flags = sh_flags & !(SHF_GROUP as u64) & !(SHF_COMPRESSED as u64);
        let mut entsize = shdr.sh_entsize.get();
        if entsize == 0 {
            entsize = if sh_flags & SHF_STRINGS as u64 != 0 {
                1
            } else {
                shdr.sh_addralign.get()
            };
        }
        if entsize == 0 {
            return None;
        }

        let name = merged_output_name(args, name, flags, entsize, addralign);
        let key = MergedSectionKey {
            name,
            flags,
            sh_type: shdr.sh_type.get(),
            entsize,
        };
        if let Some((_, id)) = cache.entries.iter().find(|(k, _)| *k == key) {
            return Some(*id);
        }
        let mut remember = |id| {
            // Bound lookup cost even when --unique creates many sections.
            if cache.entries.len() < 32 {
                cache.entries.push((key, id));
            }
            Some(id)
        };
        let find = |sections: &[MergedSection<E>]| {
            sections
                .iter()
                .position(|s| {
                    s.hdr.name == name
                        && s.hdr.shdr.sh_flags.get() == flags
                        && s.hdr.shdr.sh_type.get() == shdr.sh_type.get()
                        && s.hdr.shdr.sh_entsize.get() == entsize
                })
                .map(|i| MergedSectionId(i as u32))
        };

        // Search for an existing output section.
        if let Some(id) = find(&sections.read().unwrap()) {
            return remember(id);
        }

        // Create a new output section.
        let mut sections = sections.write().unwrap();
        if let Some(id) = find(&sections) {
            return remember(id);
        }
        sections.push(MergedSection::new(name, flags, shdr.sh_type.get(), entsize));
        remember(MergedSectionId(sections.len() as u32 - 1))
    }

    pub fn insert(
        &self,
        data: &'static [u8],
        hash: u64,
        p2align: u8,
        gc_sections: bool,
    ) -> EntryId {
        // Even if GC is enabled, we garbage-collect only memory-mapped strings.
        // Non-memory-allocated strings are typically identifiers used by debug info.
        // To remove such strings, use the `strip` command.
        let is_alive = !gc_sections || !self.is_alloc();
        let (id, frag, _) = self
            .map
            .insert_with(data, hash, || SectionFragment::new(is_alive));
        // Most insertions find the fragment there already, so the alignment
        // is only written when it grows: an atomic update on every insertion
        // would bounce the cache line of a popular fragment between cores.
        if frag.p2align.load(Ordering::Relaxed) < p2align {
            frag.p2align.fetch_max(p2align, Ordering::Relaxed);
        }
        id
    }

    pub fn is_alloc(&self) -> bool {
        self.hdr.shdr.sh_flags.get() & SHF_ALLOC as u64 != 0
    }
}

/// Splits the members into fragments and deduplicates them.
pub fn resolve<E: Arch>(ctx: &mut Context<E>, id: MergedSectionId) {
    let timers = ctx.timers.clone();
    let Context {
        objs,
        merged_sections,
        args,
        ..
    } = ctx;
    let msec = &merged_sections[id.index()];
    let gc_sections = args.gc_sections;

    // The members, by file: a file's sections are worked on by one task.
    let mut member_shndx: Vec<Vec<u32>> = vec![Vec::new(); objs.pool_len()];
    for m in &msec.members {
        member_shndx[m.file.index()].push(m.shndx);
    }

    // Split the members into pieces, estimating the number of distinct
    // ones to size the table.
    let t = timers.start("split_contents");
    let estimate = objs
        .par_iter_mut()
        .filter_map(|file| {
            let shndx = &member_shndx[file.id().index()];
            (!shndx.is_empty()).then_some((file, shndx))
        })
        .fold(HyperLogLog::default, |mut sketch, (file, shndx)| {
            let mut slots = std::mem::take(&mut file.sections);
            for &i in shndx {
                let (m, isec) = slots
                    .merge_info_with_section_mut(i as usize)
                    .expect("a mergeable section");
                let name = isec.name(file);
                m.split_contents::<E>(file, isec.contents(), name, msec, &mut sketch);
            }
            file.sections = slots;
            sketch
        })
        .reduce(HyperLogLog::default, |a, b| a.merged(&b));
    drop(t);

    // We aim 2/3 occupation ratio
    let t = timers.start("resize");
    let msec = &mut merged_sections[id.index()];
    msec.estimation = estimate.cardinality();
    msec.map = ConcurrentMap::with_capacity(msec.estimation as usize * 3 / 2);
    let msec = &*msec;
    drop(t);

    let t = timers.start("resolve_contents");
    objs.par_iter_mut()
        .filter_map(|file| {
            let shndx = &member_shndx[file.id().index()];
            (!shndx.is_empty()).then_some((file, shndx))
        })
        .for_each(|(file, shndx)| {
            for &i in shndx {
                let (m, isec) = file
                    .sections
                    .merge_info_with_section_mut(i as usize)
                    .expect("a mergeable section");
                m.resolve_contents(isec.contents(), msec, gc_sections);
            }
        });
    drop(t);

    if ctx.comment == Some(id) {
        let msec = &ctx.merged_sections[id.index()];
        add_comment_strings(msec, ctx.args.gc_sections, &ctx.cmdline_args);
    }

    // Compute section alignment
    let msec = &ctx.merged_sections[id.index()];
    let p2align = msec
        .members
        .iter()
        .filter_map(|m| ctx.objs[m.file.index()].merge_info(m.shndx as usize))
        .map(|m| m.p2align)
        .max()
        .unwrap_or(0);

    let msec = &mut ctx.merged_sections[id.index()];
    msec.hdr.shdr.sh_addralign.set(1 << p2align);
    msec.fragments = std::mem::take(&mut msec.map).freeze();
    msec.resolved = true;
    if !msec.is_alloc() {
        layout(msec);
    }
}

/// Resolves selected merged sections concurrently, as C++ mold does.
/// Direct member borrows let different parent sections mutate disjoint
/// `MergeInfo`s even when they belong to the same object file.
pub fn resolve_sections<E: Arch>(
    sections: &mut [MergedSection<E>],
    members: &mut [Vec<ResolveMember<'_>>],
    options: ResolveOptions<'_>,
) {
    let ResolveOptions {
        allocated_only,
        gc_sections,
        comment,
        cmdline_args,
        timers,
    } = options;
    let t = timers.start("split_contents");
    let estimates: Vec<Option<HyperLogLog>> = sections
        .par_iter()
        .zip(members.par_iter_mut())
        .map(|(section, members)| {
            (!section.resolved && (!allocated_only || section.is_alloc())).then(|| {
                members
                    .par_iter_mut()
                    .fold(HyperLogLog::default, |mut sketch, member| {
                        member.merge_info.split_contents::<E>(
                            &display_file(member.filename, member.archive_name),
                            member.data,
                            member.name,
                            section,
                            &mut sketch,
                        );
                        sketch
                    })
                    .reduce(HyperLogLog::default, |a, b| a.merged(&b))
            })
        })
        .collect();
    drop(t);

    // We aim 2/3 occupation ratio
    let t = timers.start("resize");
    sections
        .par_iter_mut()
        .zip(&estimates)
        .for_each(|(section, estimate)| {
            if let Some(estimate) = estimate {
                section.estimation = estimate.cardinality();
                section.map = ConcurrentMap::with_capacity(section.estimation as usize * 3 / 2);
            }
        });
    drop(t);

    let t = timers.start("resolve_contents");
    sections
        .par_iter()
        .zip(members.par_iter_mut())
        .filter(|(section, _)| !section.resolved && (!allocated_only || section.is_alloc()))
        .for_each(|(section, members)| {
            members.par_iter_mut().for_each(|member| {
                member
                    .merge_info
                    .resolve_contents(member.data, section, gc_sections);
            });
        });
    drop(t);

    sections
        .par_iter_mut()
        .zip(members.par_iter())
        .enumerate()
        .filter(|(_, (section, _))| !section.resolved && (!allocated_only || section.is_alloc()))
        .for_each(|(i, (section, members))| {
            if comment == Some(MergedSectionId(i as u32)) {
                add_comment_strings(section, gc_sections, cmdline_args);
            }

            // Compute section alignment
            let p2align = members
                .iter()
                .map(|member| member.merge_info.p2align)
                .max()
                .unwrap_or(0);
            section.hdr.shdr.sh_addralign.set(1 << p2align);
            section.fragments = std::mem::take(&mut section.map).freeze();
            section.resolved = true;
            if !section.is_alloc() {
                layout(section);
            }
        });
}

// Add strings to .comment
fn add_comment_strings<E: Layout>(
    msec: &MergedSection<E>,
    gc_sections: bool,
    cmdline_args: &[std::ffi::OsString],
) {
    let add = |mut bytes: Vec<u8>| {
        bytes.push(0);
        let data = crate::util::leak_bytes(bytes);
        msec.insert(data, xxhash_rust::xxh3::xxh3_64(data), 0, gc_sections);
    };
    // Add an identification string to .comment.
    add(crate::cmdline::VERSION.as_bytes().to_vec());

    // Embed command line arguments for debugging.
    if std::env::var_os("MOLD_DEBUG").is_some_and(|v| !v.is_empty()) {
        let mut bytes = b"mold command line: ".to_vec();
        for (i, arg) in cmdline_args[1..].iter().enumerate() {
            if i != 0 {
                bytes.push(b' ');
            }
            bytes.extend_from_slice(arg.as_encoded_bytes());
        }
        add(bytes);
    }
}

/// Lays out the live fragments and computes the section size.
pub fn compute_section_size<E: Arch>(ctx: &mut Context<E>, id: MergedSectionId) {
    if !ctx.merged_sections[id.index()].resolved {
        resolve(ctx, id);
    }
    if ctx.merged_sections[id.index()].is_alloc() {
        layout(&mut ctx.merged_sections[id.index()]);
    }
}

/// Lays out one resolved merged section. Different merged sections have no
/// shared mutable state, so callers can run this for all of them in parallel.
pub fn layout<E: Layout>(msec: &mut MergedSection<E>) {
    debug_assert!(msec.resolved);
    let frags = &msec.fragments;

    // Within a shard, fragments that must be reachable with 32-bit
    // relocations come first; the 32-bit halves of all shards precede the
    // other halves.
    let shards: Vec<ShardLayout> = (0..NUM_SHARDS)
        .into_par_iter()
        .map(|shard| {
            let fragments = frags.sorted_entries(shard);
            let mut near_size = 0;
            let mut far_size = 0;
            for &entry in &fragments {
                let frag = frags.get(entry);
                if !frag.is_alive() {
                    continue;
                }
                if frag.is_32bit() {
                    near_size = align_to(near_size, 1 << frag.p2align.load(Ordering::Relaxed));
                    frag.set_offset(near_size);
                    near_size += frags.key(entry).len() as u64;
                } else {
                    far_size = align_to(far_size, 1 << frag.p2align.load(Ordering::Relaxed));
                    frag.set_offset(far_size);
                    far_size += frags.key(entry).len() as u64;
                }
            }
            ShardLayout {
                near_size,
                far_size,
                fragments,
            }
        })
        .collect();

    let addralign = msec.hdr.shdr.sh_addralign.get();
    let mut shard_offsets = Vec::with_capacity(NUM_SHARDS * 2 + 1);
    shard_offsets.push(0);
    for size in shards
        .iter()
        .map(|shard| shard.near_size)
        .chain(shards.iter().map(|shard| shard.far_size))
    {
        shard_offsets.push(align_to(
            shard_offsets.last().copied().unwrap() + size,
            addralign,
        ));
    }

    shards.par_iter().enumerate().for_each(|(i, shard)| {
        for &entry in &shard.fragments {
            let frag = frags.get(entry);
            if frag.is_alive() {
                let base = if frag.is_32bit() {
                    shard_offsets[i]
                } else {
                    shard_offsets[i + NUM_SHARDS]
                };
                frag.set_offset(frag.offset() + base);
            }
        }
    });

    let size = shard_offsets.last().copied().unwrap();
    msec.hdr.shdr.sh_size.set(size);
    msec.shards = shards;
    msec.shard_offsets = shard_offsets;
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, id: MergedSectionId, buf: &mut [u8]) {
    write_to(ctx, id, buf);
}

pub fn write_to<E: Arch>(ctx: &Context<E>, id: MergedSectionId, buf: &mut [u8]) {
    let msec = &ctx.merged_sections[id.index()];
    let frags = &msec.fragments;

    // There might be gaps between strings to satisfy alignment requirements.
    // If that's the case, we need to zero-clear them.
    let has_gaps = msec.hdr.shdr.sh_addralign.get() > 1
        && msec.hdr.shdr.sh_addralign.get() != msec.hdr.shdr.sh_entsize.get();
    if has_gaps {
        split_at_offsets(buf, &msec.shard_offsets[..NUM_SHARDS * 2])
            .into_par_iter()
            .for_each(|region| region.fill(0));
    }

    // Copy strings
    let output = buf.as_mut_ptr() as usize;
    let output_len = buf.len();
    msec.shards.par_iter().for_each(|shard| {
        for &entry in &shard.fragments {
            let frag = frags.get(entry);
            if frag.is_alive() {
                let key = frags.key(entry);
                let offset = frag.offset() as usize;
                debug_assert!(offset + key.len() <= output_len);
                // SAFETY: layout assigns every live fragment a distinct range
                // within `buf`; each entry occurs in exactly one shard, so the
                // parallel copies do not overlap.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        key.as_ptr(),
                        (output as *mut u8).add(offset),
                        key.len(),
                    );
                }
            }
        }
    });
}

pub fn print_stats<E: Layout>(msec: &MergedSection<E>) {
    out!(
        "{} estimation={} actual={}",
        msec.hdr.name,
        msec.estimation,
        msec.fragments.len()
    );
}
