//! `--print-map` / `-Map`: a listing of output sections, their input
//! sections and symbols.

use std::collections::HashMap;

use rayon::prelude::*;

use crate::arch::Arch;
use crate::chunks::ChunkId;
use crate::cmdline::ReportOutput;
use crate::context::Context;
use crate::elf::*;
use crate::input_files::FileId;
use crate::input_sections::InputSectionId;
use crate::symbol::SymbolId;

// Construct a section-to-symbol map.
fn section_symbols<E: Arch>(ctx: &Context<E>) -> HashMap<InputSectionId, Vec<SymbolId>> {
    let mut map: HashMap<InputSectionId, Vec<SymbolId>> = HashMap::new();
    for file in &ctx.objs {
        let file_id = FileId::Obj(file.id());
        for &id in &file.base.symbols {
            let sym = &ctx.symbols[id];
            if sym.file() == Some(file_id) && sym.ty() != STT_SECTION {
                if let Some(r) = sym.input_section() {
                    map.entry(r).or_default().push(id);
                }
            }
        }
    }
    for syms in map.values_mut() {
        syms.sort_by_key(|&id| ctx.symbols[id].value);
    }
    map
}

pub fn print_map<E: Arch>(ctx: &Context<E>, output: &ReportOutput) {
    // Print a mapfile.
    let _t = ctx.timer("print_map");
    let map = section_symbols(ctx);

    let mut out = String::from("               VMA       Size Align Out     In      Symbol\n");
    for &id in &ctx.chunks {
        let hdr = ctx.chunk_header(id);
        out.push_str(&format!(
            "{:>18}{:>11}{:>6} {}\n",
            format!("{:#x}", hdr.shdr.sh_addr.get()),
            hdr.shdr.sh_size.get(),
            hdr.shdr.sh_addralign.get(),
            hdr.name
        ));

        let ChunkId::Output(osec_id) = id else {
            continue;
        };
        let osec = &ctx.output_sections[osec_id.index()];
        let lines: Vec<String> = osec
            .members
            .par_iter()
            .map(|&member| {
                let isec = ctx.input_section(member);
                let addr = if osec.hdr.is_alloc() {
                    osec.hdr.shdr.sh_addr.get() + isec.offset()
                } else {
                    0
                };
                let mut s = format!(
                    "{:>18}{:>11}{:>6}         {}\n",
                    format!("{addr:#x}"),
                    isec.sh_size,
                    1u64 << isec.p2align(),
                    isec.display(&ctx.objs[isec.file.index()])
                );
                if let Some(syms) = map.get(&member) {
                    for &id in syms {
                        let sym = &ctx.symbols[id];
                        s.push_str(&format!(
                            "{:>18}          0     0                 {sym}\n",
                            format!("{:#x}", sym.addr(ctx))
                        ));
                    }
                }
                s
            })
            .collect();
        for line in lines {
            out.push_str(&line);
        }
    }

    output.write("--print-map", out.as_bytes());
}
