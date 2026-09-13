//! This file implements a mark-sweep garbage collector for -gc-sections.
//! In this algorithm, vertices are sections and edges are relocations.
//! Any section that is reachable from a root section is considered alive.

use std::collections::HashMap;
use std::fmt::Write;

use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::input_files::{FileId, ObjectFile};
use crate::input_sections::{InputSection, SectionRef};
use crate::symbol::{is_c_identifier, OriginValue, SymbolId};

fn should_keep<E: Arch>(file: &ObjectFile<E>, isec: &InputSection<E>) -> bool {
    let ty = isec.sh_type(file);
    let flags = isec.sh_flags as u32;
    let name: &[u8] = isec.name(file);
    if E::FAMILY == Family::Ppc32 && name == b".got2" {
        return true;
    }
    flags & SHF_GNU_RETAIN != 0
        || matches!(
            ty,
            SHT_NOTE | SHT_INIT_ARRAY | SHT_FINI_ARRAY | SHT_PREINIT_ARRAY
        )
        || name.starts_with(b".ctors")
        || name.starts_with(b".dtors")
        || name.starts_with(b".init")
        || name.starts_with(b".fini")
}

/// Sections whose names are valid C identifiers can be referenced via
/// __start_<name>/__stop_<name> symbols, which the linker synthesizes.
/// Such sections must be kept alive only if such a marker symbol is
/// referenced from a live section. This map lets us find all sections
/// of a given name when we encounter such a reference during marking.
type StartStopMap<'a, E> = HashMap<&'static [u8], Vec<&'a InputSection<E>>>;

fn build_start_stop_map<'a, E: Arch>(ctx: &'a Context<E>) -> StartStopMap<'a, E> {
    let sections: Vec<(&'static [u8], &'a InputSection<E>)> = ctx
        .objs
        .par_iter()
        .flat_map_iter(|file| {
            file.input_sections()
                .filter(|isec| {
                    isec.is_alive() && isec.is_alloc() && is_c_identifier(isec.name(file))
                })
                .map(|isec| {
                    let name: &'static [u8] = isec.name(file);
                    (name, isec)
                })
        })
        .collect();
    let mut map: StartStopMap<'a, E> = HashMap::new();
    for (name, isec) in sections {
        map.entry(name).or_default().push(isec);
    }
    map
}

#[inline]
fn mark_section<E: Arch>(isec: &InputSection<E>) -> bool {
    isec.is_alive() && isec.visit()
}

fn collect_root_set<'a, E: Arch>(ctx: &'a Context<E>) -> Vec<&'a InputSection<E>> {
    let _t = ctx.timer("collect_root_set");

    let enqueue_symbol = |id: SymbolId, out: &mut Vec<&'a InputSection<E>>| {
        let sym = &ctx.symbols[id];
        match sym.origin() {
            OriginValue::Fragment(frag) => ctx.fragment(frag).set_alive(),
            OriginValue::InputSection(section) => {
                let isec = ctx.input_section(section);
                if mark_section(isec) {
                    out.push(isec);
                }
            }
            _ => {}
        }
    };

    ctx.objs
        .par_iter()
        .flat_map_iter(|file| {
            let file_id = FileId::Obj(file.id());
            let mut roots = Vec::new();

            // Add sections that are not subject to garbage collection.
            //
            // --gc-sections discards only SHF_ALLOC sections. If you want to
            // reduce the amount of non-memory-mapped segments, you should
            // use `strip` command, compile without debug info or use
            // --strip-all linker option.
            for isec in file.input_sections() {
                if !isec.is_alive() {
                    continue;
                }
                if !isec.is_alloc() {
                    isec.set_visited();
                    continue;
                }
                if should_keep::<E>(file, isec) && mark_section(isec) {
                    roots.push(isec);
                }
            }

            // Add sections containing gc root or exported symbols
            for &id in &file.base.symbols {
                let sym = &ctx.symbols[id];
                if sym.file() == Some(file_id) && (sym.gc_root() || sym.is_exported()) {
                    enqueue_symbol(id, &mut roots);
                }
            }

            // .eh_frame consists of variable-length records called CIE and FDE
            // records, and they are a unit of inclusion or exclusion.
            // We just keep all CIEs and everything that are referenced by them.
            for cie in &file.cies {
                for rel in cie.rels(file) {
                    enqueue_symbol(file.base.symbols[rel.r_sym() as usize], &mut roots);
                }
            }
            roots
        })
        .collect()
}

#[inline]
fn start_stop_name(name: &[u8]) -> Option<&[u8]> {
    name.strip_prefix(b"__start_")
        .or_else(|| name.strip_prefix(b"__stop_"))
}

fn visit_section<'scope, E: Arch>(
    ctx: &'scope Context<E>,
    isec: &'scope InputSection<E>,
    depth: usize,
    map: &'scope StartStopMap<'scope, E>,
    scope: &rayon::Scope<'scope>,
    next: &mut Vec<&'scope InputSection<E>>,
) {
    let file = &ctx.objs[isec.file.index()];
    debug_assert!(isec.is_visited());

    // Mark a section alive. Recurse for a few levels before queueing more work
    // so that we do not create a Rayon task for every edge.
    let mut mark = |target: &'scope InputSection<E>| {
        if mark_section(target) {
            if depth < 3 {
                visit_section(ctx, target, depth + 1, map, scope, next);
            } else {
                next.push(target);
            }
        }
    };

    // If this is a text section, .eh_frame may contain records
    // describing how to handle exceptions for that function.
    // We want to keep associated .eh_frame records.
    for fde in isec.fdes(file) {
        for rel in fde.rels(file).iter().skip(1) {
            if let Some(target) =
                ctx.symbols[file.base.symbols[rel.r_sym() as usize]].input_section_ref(ctx)
            {
                mark(target);
            }
        }
    }

    for rel in isec.rels(file) {
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym() as usize]];
        if let Some(FileId::Dso(dso)) = sym.file() {
            ctx.dsos[dso.index()].base.mark_reachable();
            continue;
        }
        // Symbol can refer to either a section fragment or an input section.
        match sym.origin() {
            OriginValue::Fragment(frag) => {
                ctx.fragment(frag).set_alive();
                continue;
            }
            OriginValue::InputSection(section) => {
                mark(ctx.input_section(section));
            }
            _ => {}
        }

        // A reference to __start_<name> or __stop_<name> keeps every
        // section named <name> alive, mirroring how those symbols are
        // defined. A single such reference can keep an enormous number of
        // sections alive, so we spread the fanout over threads instead
        // of marking the sections one by one.
        if let Some(name) = start_stop_name(sym.name()) {
            if let Some(sections) = map.get(name) {
                // Mark targets in parallel. mark_section returns true only for
                // newly marked sections, which the batch visitor must then visit
                // unconditionally.
                sections.par_chunks(GC_BATCH).for_each(|sections| {
                    let mut found = Vec::with_capacity(sections.len());
                    for &target in sections {
                        if mark_section(target) {
                            found.push(target);
                        }
                    }
                    if !found.is_empty() {
                        visit_batch(ctx, &found, map, scope);
                    }
                });
            }
        }
    }

    if E::FAMILY == Family::Arm32 {
        if let Some(exidx) = isec.exidx() {
            mark(file.section_at(exidx));
        }
    }
}

const GC_BATCH: usize = 16;

/// Visits marked sections and publishes newly found work in batches. Batching
/// preserves dynamic load balancing while amortizing Rayon task allocation.
fn visit_batch<'scope, E: Arch>(
    ctx: &'scope Context<E>,
    batch: &[&'scope InputSection<E>],
    map: &'scope StartStopMap<'scope, E>,
    scope: &rayon::Scope<'scope>,
) {
    // Exported symbols can make most sections roots already. Such batches
    // discover no new work and do not need a heap allocation.
    let mut next = Vec::new();
    for &isec in batch {
        visit_section(ctx, isec, 0, map, scope, &mut next);
        if next.len() >= GC_BATCH {
            let found = std::mem::take(&mut next);
            scope.spawn(move |scope| visit_batch(ctx, &found, map, scope));
        }
    }
    if !next.is_empty() {
        scope.spawn(move |scope| visit_batch(ctx, &next, map, scope));
    }
}

// Mark all reachable sections
fn mark<'a, E: Arch>(
    ctx: &'a Context<E>,
    roots: Vec<&'a InputSection<E>>,
    map: &'a StartStopMap<'a, E>,
) {
    let _t = ctx.timer("mark");

    rayon::scope(|scope| {
        roots
            .par_chunks(GC_BATCH)
            .for_each(|batch| visit_batch(ctx, batch, map, scope));
    });
}

// Remove unreachable sections
fn sweep<E: Arch>(ctx: &Context<E>) {
    let _t = ctx.timer("sweep");
    let report = ctx.args.print_gc_sections.is_some();
    let removed: Vec<SectionRef> = ctx
        .objs
        .par_iter()
        .flat_map_iter(|file| {
            let mut removed = Vec::new();
            for isec in file.input_sections() {
                if isec.is_alive() && !isec.is_visited() {
                    file.kill_section(isec.shndx as usize);
                    if report {
                        removed.push(SectionRef {
                            file: isec.file,
                            shndx: isec.shndx,
                        });
                    }
                }
            }
            removed
        })
        .collect();

    let Some(output) = &ctx.args.print_gc_sections else {
        return;
    };
    let mut out = String::new();
    for r in &removed {
        writeln!(out, "removing unused section {}", ctx.section_display(*r)).unwrap();
    }
    out.push_str("GC saved 0 bytes\n");
    output.write("--print-gc-sections", out.as_bytes());
}

pub fn gc_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("gc");

    for file in &ctx.dsos {
        if file.base.as_needed {
            file.base.set_reachable(false);
        }
    }
    for name in ctx.args.undefined.iter().chain(&ctx.args.require_defined) {
        let id = ctx.symbols.get_or_intern(name);
        if let Some(FileId::Dso(dso)) = ctx.symbols[id].file() {
            ctx.dsos[dso.index()].base.set_reachable(true);
        }
    }

    let roots = collect_root_set(ctx);
    let t = ctx.timer("build_start_stop_map");
    let map = build_start_stop_map(ctx);
    drop(t);
    mark(ctx, roots, &map);
    sweep(ctx);

    crate::passes::remove_unreachable_dsos(ctx);
}
