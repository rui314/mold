//! -map file output: a report of where every object file, section and
//! symbol ended up, in ld64's format.

use std::io::Write;

use crate::fatal;
use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::files::FileName;
use crate::macho::input_files::FileId;

fn json_string(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x00'..='\x1f' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Xcode's version-1 API import report. Despite its name, sdkImports
/// includes imports from non-SDK dylibs too, grouped by install name.
pub fn write_sdk_imports<E: Arch>(ctx: &Context<E>) {
    use crate::macho::format::{format_version, platform_name};
    let Some(path) = &ctx.args.sdk_imports else { return };
    let mut imports = std::collections::BTreeMap::<&str, Vec<&str>>::new();
    for sym in &ctx.symbols.syms {
        if !sym.is_imported() || !sym.is_used() {
            continue;
        }
        let Some(FileId::Dylib(idx)) = sym.file() else { continue };
        // Dynamic-lookup symbols have no defining library to report.
        let Some(dylib) = ctx.dylibs.get(idx as usize) else { continue };
        imports.entry(&dylib.install_name).or_default().push(sym.name());
    }
    let libraries: Vec<String> = imports
        .into_iter()
        .map(|(name, mut symbols)| {
            symbols.sort_unstable();
            symbols.dedup();
            let symbols: Vec<String> = symbols.into_iter().map(json_string).collect();
            format!("{{\"installName\":{},\"symbols\":[{}]}}", json_string(name), symbols.join(","))
        })
        .collect();
    let output = json_string(&ctx.args.output);
    let report = format!(
        "{{\"version\":1,\"output\":{output},\"arch\":{},\"linker\":{},\"apiListVersion\":0,\
         \"platform\":{},\"deploymentVersion\":{},\"sdkVersion\":{},\
         \"inputs\":[{{\"path\":{output},\"sdkImports\":[{}]}}]}}\n",
        json_string(E::NAME),
        json_string(concat!("mold-macho-", env!("CARGO_PKG_VERSION"))),
        json_string(&platform_name(ctx.args.platform)),
        json_string(&format_version(ctx.args.platform_minos)),
        json_string(&format_version(ctx.args.platform_sdk)),
        libraries.join(",")
    );
    std::fs::write(path, report).unwrap_or_else(|e| fatal!("cannot write {path}: {e}"));
}

/// Writes the -dependency_info file: Xcode's incremental build system
/// reads it to learn which files the link actually consumed. The
/// format is binary: an opcode byte then a NUL-terminated string -
/// 0x00 version, 0x10 input file, 0x11 file that was looked up but
/// missing, 0x40 output file.
pub fn write_dependency_info<E: Arch>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.dependency_info else {
        return;
    };
    let file = std::fs::File::create(path).unwrap_or_else(|e| fatal!("cannot open {path}: {e}"));
    let mut out = std::io::BufWriter::new(file);
    let mut emit = |op: u8, s: &str| {
        let _ = out.write_all(&[op]);
        let _ = out.write_all(s.as_bytes());
        let _ = out.write_all(&[0]);
    };

    emit(0x00, concat!("mold-macho ", env!("CARGO_PKG_VERSION")));
    let mut inputs: Vec<&str> = ctx
        .objs
        .iter()
        .enumerate()
        .filter(|(i, o)| o.is_alive && !ctx.is_internal(*i))
        .map(|(_, o)| o.mf.parent.map(|p| p.name_str()).unwrap_or(o.mf.name_str()))
        .collect();
    inputs.extend(ctx.visited_files.iter().map(String::as_str));
    inputs.sort_unstable();
    inputs.dedup();
    for name in inputs {
        emit(0x10, name);
    }
    emit(0x40, &ctx.args.output);
}

pub fn print_map<E: Arch>(ctx: &Context<E>) {
    let Some(path) = &ctx.args.map else { return };
    let file = std::fs::File::create(path).unwrap_or_else(|e| fatal!("cannot open {path}: {e}"));
    let mut out = std::io::BufWriter::new(file);

    let _ = writeln!(out, "# Path: {}", ctx.args.output);
    let _ = writeln!(out, "# Arch: {}", E::NAME);

    // ld64 reserves file number 0 for atoms the linker itself creates
    // (the Mach-O header symbol, unwind info, stubs); real objects are
    // numbered from 1 in link order.
    let _ = writeln!(out, "# Object files:");
    let _ = writeln!(out, "[  0] linker synthesized");
    let mut file_no = vec![0usize; ctx.objs.len()];
    let mut next = 1usize;
    for (i, obj) in ctx.objs.iter().enumerate() {
        if obj.is_alive && !ctx.is_internal(i) {
            file_no[i] = next;
            let _ = writeln!(out, "[{next:3}] {}", obj.mf.name_str());
            next += 1;
        }
    }

    let _ = writeln!(out, "# Sections:");
    let _ = writeln!(out, "# Address\tSize    \tSegment\tSection");
    for seg in &ctx.segments {
        for &id in &seg.chunks {
            let hdr = ctx.chunk_header(id);
            if hdr.is_sect {
                let _ = writeln!(
                    out,
                    "0x{:08X}\t0x{:08X}\t{}\t{}",
                    hdr.addr, hdr.size, hdr.segname, hdr.sectname
                );
            }
        }
    }

    // Defined symbols with their addresses, sizes and owning objects,
    // sorted by address. A symbol's size is the span to the next
    // symbol in its subsection (or the subsection's end) - the same
    // atom size ld64 reports. Compiler temp labels (l/L prefixes)
    // are not atoms and are skipped.
    let mut syms: Vec<(u64, usize, &str, usize, u64)> = Vec::new();
    for i in 0..ctx.symbols.syms.len() {
        let sym = &ctx.symbols[i];
        let Some(FileId::Obj(obj)) = sym.file() else {
            continue;
        };
        let obj = obj as usize;
        let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
        let isec = ctx.resolve_isec(isec);
        if !ctx.isecs[isec].is_alive() || sym.name().is_empty() {
            continue;
        }
        if !sym.is_extern() && (sym.name().starts_with('l') || sym.name().starts_with('L')) {
            continue;
        }
        syms.push((ctx.sym_addr(i as u32), file_no[obj], sym.name(), isec, sym.value));
    }

    let mut sizes = vec![0u64; syms.len()];
    let mut order: Vec<usize> = (0..syms.len()).collect();
    order.sort_by_key(|&i| (syms[i].3, syms[i].4));
    for (i, &idx) in order.iter().enumerate() {
        let (_, _, _, isec, value) = syms[idx];
        let end = match order.get(i + 1) {
            Some(&next) if syms[next].3 == isec => syms[next].4,
            _ => ctx.isecs[isec].size as u64,
        };
        sizes[idx] = end.saturating_sub(value);
    }

    let mut order: Vec<usize> = (0..syms.len()).collect();
    order.sort_by_key(|&i| (syms[i].0, syms[i].1));

    // Symbols removed by -dead_strip appear in their own section
    // with "<<dead>>" in the address column, the way ld64 reports
    // them; sizes are the atom extents they would have had.
    let mut dead: Vec<(usize, u64, usize, &str)> = Vec::new();
    if ctx.args.dead_strip {
        for i in 0..ctx.symbols.syms.len() {
            let sym = &ctx.symbols[i];
            let Some(FileId::Obj(obj)) = sym.file() else {
                continue;
            };
            let obj = obj as usize;
            let Some(isec) = sym.input_section().map(|i| i as usize) else { continue };
            let isec = ctx.resolve_isec(isec);
            if ctx.isecs[isec].is_alive()
                || !ctx.objs[obj].is_alive
                || sym.name().is_empty()
                || (!sym.is_extern()
                    && (sym.name().starts_with('l') || sym.name().starts_with('L')))
            {
                continue;
            }
            dead.push((file_no[obj], sym.value, isec, sym.name()));
        }
        dead.sort();
    }

    let _ = writeln!(out, "# Symbols:");
    let _ = writeln!(out, "# Address\tSize    \tFile  Name");
    if ctx.args.output_type == crate::macho::format::MH_EXECUTE {
        let _ = writeln!(
            out,
            "0x{:08X}\t0x00000000\t[  0] __mh_execute_header",
            ctx.args.pagezero_size
        );
    }
    for idx in order {
        let (addr, file, name, _, _) = syms[idx];
        let _ = writeln!(out, "0x{addr:08X}\t0x{:08X}\t[{file:3}] {name}", sizes[idx]);
    }

    if !dead.is_empty() {
        let _ = writeln!(out, "# Dead Stripped Symbols:");
        let _ = writeln!(out, "#        \tSize    \tFile  Name");
        for (i, &(file, value, isec, name)) in dead.iter().enumerate() {
            let end = match dead.get(i + 1) {
                Some(&(_, next_value, next_isec, _)) if next_isec == isec => next_value,
                _ => ctx.isecs[isec].size as u64,
            };
            let _ =
                writeln!(out, "<<dead>> \t0x{:08X}\t[{file:3}] {name}", end.saturating_sub(value));
        }
    }
}
