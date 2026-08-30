//! A mark-sweep garbage collector for `--gc-sections`: sections are
//! vertices, relocations are edges, and anything reachable from a root
//! section is kept.

use std::collections::HashMap;
use std::io::Write;

use rayon::prelude::*;

use crate::arch::{Arch, Family};
use crate::context::Context;
use crate::elf::*;
use crate::fatal;
use crate::input_files::{FileId, ObjectFile};
use crate::input_sections::{InputSection, SectionRef};
use crate::symbol::{is_c_identifier, SymbolId};

fn should_keep<E: Arch>(file: &ObjectFile, isec: &InputSection) -> bool {
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

/// Sections with C identifier names can be referenced through the
/// synthesized `__start_<name>`/`__stop_<name>` symbols, so a reference
/// to such a symbol keeps every section of that name alive.
type StartStopMap<'a> = HashMap<&'static [u8], Vec<&'a InputSection>>;

fn build_start_stop_map<'a, E: Arch>(ctx: &'a Context<E>) -> StartStopMap<'a> {
    let per_file: Vec<Vec<(&'static [u8], &'a InputSection)>> = ctx
        .objs
        .par_iter()
        .map(|file| {
            file.input_sections()
                .filter(|isec| {
                    isec.is_alive() && isec.is_alloc() && is_c_identifier(isec.name(file))
                })
                .map(|isec| {
                    let name: &'static [u8] = isec.name(file);
                    (name, isec)
                })
                .collect()
        })
        .collect();
    let mut map: StartStopMap<'a> = HashMap::new();
    for (name, isec) in per_file.into_iter().flatten() {
        map.entry(name).or_default().push(isec);
    }
    map
}

#[inline]
fn mark_section(isec: &InputSection) -> bool {
    isec.is_alive() && isec.visit()
}

fn collect_root_set<'a, E: Arch>(ctx: &'a Context<E>) -> Vec<&'a InputSection> {
    let _t = ctx.timer("collect_root_set");

    let enqueue_symbol = |id: SymbolId, out: &mut Vec<&'a InputSection>| {
        let sym = &ctx.symbols[id];
        if let Some(frag) = sym.fragment() {
            ctx.fragment(frag).set_alive();
        } else if let Some(isec) = sym.input_section_ref() {
            if mark_section(isec) {
                out.push(isec);
            }
        }
    };

    ctx.objs
        .par_iter()
        .enumerate()
        .flat_map_iter(|(fi, file)| {
            let file_id = FileId::Obj(crate::input_files::ObjId(fi as u32));
            let mut roots = Vec::new();

            // Sections not subject to garbage collection. Only SHF_ALLOC
            // sections are discarded; use `strip` for the rest.
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

            // Sections containing GC roots or exported symbols.
            for &id in &file.base.symbols {
                let sym = &ctx.symbols[id];
                if sym.file() == Some(file_id) && (sym.gc_root() || sym.is_exported()) {
                    enqueue_symbol(id, &mut roots);
                }
            }

            // CIEs are always kept along with everything they reference.
            for cie in &file.cies {
                for rel in cie.rels::<E>(file) {
                    enqueue_symbol(file.base.symbols[rel.r_sym as usize], &mut roots);
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
    isec: &'scope InputSection,
    depth: usize,
    map: &'scope StartStopMap<'scope>,
    scope: &rayon::Scope<'scope>,
    next: &mut Vec<&'scope InputSection>,
) {
    let file = &ctx.objs[isec.file.index()];
    debug_assert!(isec.is_visited());

    // Mark a section alive. For better performance, we don't add work
    // to the pool too often.
    let mut mark = |target: &'scope InputSection| {
        if mark_section(target) {
            if depth < 3 {
                visit_section(ctx, target, depth + 1, map, scope, next);
            } else {
                next.push(target);
            }
        }
    };

    // Keep the .eh_frame records describing this section's functions.
    for fde in isec.fdes(file) {
        for rel in fde.rels::<E>(file).iter().skip(1) {
            if let Some(target) =
                ctx.symbols[file.base.symbols[rel.r_sym as usize]].input_section_ref()
            {
                mark(target);
            }
        }
    }

    for rel in isec.rels::<E>(file) {
        let sym = &ctx.symbols[file.base.symbols[rel.r_sym as usize]];
        if let Some(FileId::Dso(dso)) = sym.file() {
            ctx.dsos[dso.index()].base.set_reachable(true);
            continue;
        }
        if let Some(frag) = sym.fragment() {
            ctx.fragment(frag).set_alive();
            continue;
        }
        if let Some(target) = sym.input_section_ref() {
            mark(target);
        }

        // A reference to __start_<name> or __stop_<name> keeps every
        // section named <name> alive.
        if let Some(name) = start_stop_name(sym.name()) {
            if let Some(sections) = map.get(name) {
                // Mark and visit the sections with a nested parallel loop.
                // As in mark() below, a section added to the work pool has
                // already been marked, so a task must visit it unconditionally.
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

/// Visits marked sections and publishes newly found work in batches. Rayon
/// tasks are heavier than TBB feeder items, so a small batch preserves the
/// feeder's dynamic load balancing while amortizing task allocation.
fn visit_batch<'scope, E: Arch>(
    ctx: &'scope Context<E>,
    batch: &[&'scope InputSection],
    map: &'scope StartStopMap<'scope>,
    scope: &rayon::Scope<'scope>,
) {
    let mut next = Vec::with_capacity(GC_BATCH);
    for &isec in batch {
        visit_section(ctx, isec, 0, map, scope, &mut next);
        if next.len() >= GC_BATCH {
            let found = std::mem::replace(&mut next, Vec::with_capacity(GC_BATCH));
            scope.spawn(move |scope| visit_batch(ctx, &found, map, scope));
        }
    }
    if !next.is_empty() {
        scope.spawn(move |scope| visit_batch(ctx, &next, map, scope));
    }
}

// Mark all reachable sections.
fn mark<'a, E: Arch>(ctx: &'a Context<E>, roots: Vec<&'a InputSection>, map: &'a StartStopMap<'a>) {
    let _t = ctx.timer("mark");

    rayon::scope(|scope| {
        roots
            .par_chunks(GC_BATCH)
            .for_each(|batch| visit_batch(ctx, batch, map, scope));
    });
}

fn sweep<E: Arch>(ctx: &Context<E>) {
    let _t = ctx.timer("sweep");
    let removed: Vec<Vec<SectionRef>> = ctx
        .objs
        .par_iter()
        .map(|file| {
            let mut removed = Vec::new();
            for isec in file.input_sections() {
                if isec.is_alive() && !isec.is_visited() {
                    file.kill_section(isec.shndx as usize);
                    removed.push(SectionRef {
                        file: isec.file,
                        shndx: isec.shndx,
                    });
                }
            }
            removed
        })
        .collect();

    let path = &ctx.args.print_gc_sections;
    if path.is_empty() {
        return;
    }
    let mut out = String::new();
    for r in removed.iter().flatten() {
        out.push_str(&format!(
            "removing unused section {}\n",
            ctx.section_display(*r)
        ));
    }
    out.push_str("GC saved 0 bytes\n");
    if path == "-" {
        let _ = std::io::stdout().write_all(out.as_bytes());
    } else {
        std::fs::write(path, out)
            .unwrap_or_else(|e| fatal!(ctx, "--print-gc-sections: cannot open {path}: {e}"));
    }
}

pub fn gc_sections<E: Arch>(ctx: &mut Context<E>) {
    let _t = ctx.timer("gc");

    for file in &ctx.dsos {
        if file.base.as_needed {
            file.base.set_reachable(false);
        }
    }
    for name in ctx
        .args
        .undefined
        .clone()
        .iter()
        .chain(ctx.args.require_defined.clone().iter())
    {
        let id = ctx.get_symbol(name.as_bytes());
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
