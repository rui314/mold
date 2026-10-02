//! DTrace USDT probes (statically defined tracing).
//!
//! A header `dtrace -h` makes from a D script has the code call an
//! undefined function for each probe site, `___dtrace_probe$<provider>$
//! <probe>$v1$<argument types>`, and for each is-enabled test,
//! `___dtrace_isenabled$<provider>$<probe>$v1`, and refer to two more
//! undefined symbols per provider that no relocation uses: its
//! stability attributes and its argument typedefs. None of these is
//! ever defined.
//!
//! A final link turns each site into code that does nothing - a probe's
//! call into a nop, an is-enabled test's into setting its result to 0 -
//! and describes the sites, one provider at a time, in a DOF ("DTrace
//! Object Format") section of __TEXT, __dof_<provider>, which dyld hands
//! to the kernel as it loads the image; enabling a probe has the kernel
//! patch its sites. ld-prime has the OS's libdtrace make the DOF from
//! the symbol names (dtrace_ld_create_dof): it rebuilds the provider's
//! D script from them, compiles it, and registers each site under its
//! probe and the function it is in. We do the same without libdtrace:
//! its checks and messages, the D compiler's names for the argument
//! types, and the DOF's layout, byte for byte (see build_dof). The DOF
//! leaves a slot for each site's distance from it, which a pair of
//! relocations fills in once the output is laid out.
//!
//! ld-prime makes the DOF sections in the order of a hash table of the
//! providers; we make them in the order of their first sites.

use hashbrown::HashMap;
use rayon::prelude::*;

use crate::context::Context;
use crate::error::{Message, raw, render};
use crate::input_sections::{InputSection, NO_REPLACEMENT, Reloc, RelocTarget};
use crate::macho::*;
use crate::symbol::SymbolId;
use crate::target::{RelocClass, Target};
use crate::util::split_once;

/// The prefix of the names of the symbols a `dtrace -h` header makes
/// code refer to. ld-prime takes every undefined symbol whose name
/// starts with it for one of them, whatever follows.
const PREFIX: &[u8] = b"___dtrace_";
const PROBE_PREFIX: &[u8] = b"___dtrace_probe$";
const IS_ENABLED_PREFIX: &[u8] = b"___dtrace_isenabled$";

/// Whether an undefined symbol of this name is a DTrace symbol.
pub fn is_dtrace_symbol(name: &[u8]) -> bool {
    name.starts_with(PREFIX)
}

/// What a call of a DTrace symbol is: ld-prime's dtraceKind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    /// A probe's site, which fires it.
    Probe,
    /// An is-enabled test, which returns whether the probe is on.
    IsEnabled,
}

/// The kind of site a call of DTrace symbol `name` makes, or None for a
/// provider's other symbols (its stability and typedefs, or any other
/// name with the prefix), which no code may call.
fn site_kind_of(name: &[u8]) -> Option<SiteKind> {
    if name.starts_with(PROBE_PREFIX) {
        Some(SiteKind::Probe)
    } else if name.starts_with(IS_ENABLED_PREFIX) {
        Some(SiteKind::IsEnabled)
    } else {
        None
    }
}

/// The provider a DTrace symbol belongs to: for a probe or an is-enabled
/// test the field after the prefix, for another symbol the one after
/// its first `$`; empty if the `$` that ends it is missing.
fn provider_of(name: &[u8]) -> &[u8] {
    let rest = match site_kind_of(name) {
        Some(SiteKind::Probe) => &name[PROBE_PREFIX.len()..],
        Some(SiteKind::IsEnabled) => &name[IS_ENABLED_PREFIX.len()..],
        None => split_once(name, b'$').map_or(&[][..], |(_, rest)| rest),
    };
    split_once(rest, b'$').map_or(&[][..], |(provider, _)| provider)
}

/// A provider's DOF section, as the link makes it: the subsection with
/// the DOF, which refers to each site's subsection (in site order), and
/// the name ld-prime gives that subsection in -why_live.
#[derive(Debug)]
pub struct DofSection {
    pub isec: u32,
    pub sites: Vec<u32>,
    pub subsec_name: Vec<u8>,
}

/// Whether a branch to symbol `sym` from subsection `isec` is a probe
/// site, and of which kind: a branch from code to a probe or an
/// is-enabled test, in a link that makes DOF. Another branch to a
/// DTrace symbol goes to address 0, as ld-prime has it.
#[inline]
pub fn site_kind<E: Target>(ctx: &Context<E>, isec: usize, sym: SymbolId) -> Option<SiteKind> {
    let sym = &ctx.symbols[sym];
    if sym.is_defined() || !ctx.args.dtrace_dof || !is_code(ctx, isec) {
        return None;
    }
    site_kind_of(sym.name())
}

/// ld-prime looks for probe sites in the subsections whose content is
/// code, those of a section of only instructions.
fn is_code<E: Target>(ctx: &Context<E>, isec: usize) -> bool {
    ctx.hdr_of(&ctx.isecs[isec]).flags & S_ATTR_PURE_INSTRUCTIONS != 0
}

/// A probe site: a branch to a probe or an is-enabled test, at offset
/// `offset` of subsection `isec` (the relocated field's).
struct Site {
    isec: u32,
    offset: u32,
    sym: SymbolId,
    kind: SiteKind,
}

/// Makes a DOF section for each provider some code has a probe site of,
/// as ld-prime does after resolving the symbols and before dead
/// stripping: the sites of dead functions count, and keep them alive.
/// Reports an error as ld-prime does, at the first provider whose
/// symbols make none.
pub fn create_dof_sections<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.dtrace_dof || ctx.args.relocatable {
        return;
    }
    let syms = dtrace_symbols(ctx);
    if syms.is_empty() {
        return;
    }
    let Some(sites) = collect_sites(ctx) else {
        crate::error!("Unexpected call to dtrace provider undef");
        return;
    };
    let names = subsec_names(ctx, &sites);
    let infos: Vec<&[u8]> = (syms.iter())
        .map(|&id| ctx.symbols[id].name())
        .filter(|&name| site_kind_of(name).is_none())
        .collect();
    let mut taken: Vec<Vec<u8>> = Vec::new();
    for (provider, sites) in sites_by_provider(ctx, &sites) {
        let mut types: Vec<&[u8]> =
            infos.iter().copied().filter(|&name| provider_of(name) == provider).collect();
        types.sort_unstable();
        let probes: Vec<&[u8]> = sites.iter().map(|s| ctx.symbols[s.sym].name()).collect();
        let functions: Vec<&[u8]> = sites.iter().map(|s| names[&s.isec]).collect();
        let dof = match build_dof(&types, &probes, &functions) {
            Ok(dof) => dof,
            // libdtrace's message, then ld-prime's.
            Err(msg) => {
                crate::error::notice(format_args!(
                    "{}",
                    raw(msg.strip_suffix(b"\n").unwrap_or(&msg))
                ));
                crate::error!("error creating dtrace DOF section");
                return;
            }
        };
        let name = section_name(provider, &taken);
        add_dof_section(ctx, provider, &name, dof, &sites);
        taken.push(name);
    }
}

/// The undefined symbols with the DTrace prefix that live objects refer
/// to; most links have none.
fn dtrace_symbols<E: Target>(ctx: &Context<E>) -> Vec<SymbolId> {
    (0..ctx.symbols.syms.len() as SymbolId)
        .into_par_iter()
        .filter(|&id| {
            let sym = &ctx.symbols[id];
            sym.is_used() && !sym.is_defined() && is_dtrace_symbol(sym.name())
        })
        .collect()
}

/// The probe sites, in ld-prime's order: by file in input order, by
/// section and address in a file (the subsections' order), and by
/// offset in a subsection - as the code is laid out without an order
/// file. A copy of a function another one replaced (a weak definition
/// another file's won) has none. None if code calls one of a
/// provider's other symbols, which ld-prime refuses.
fn collect_sites<E: Target>(ctx: &Context<E>) -> Option<Vec<Site>> {
    let found: Vec<(u32, u32, SymbolId)> = (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx.isecs[i];
            isec.is_alive() && isec.replacement == NO_REPLACEMENT && is_code(ctx, i)
        })
        .flat_map_iter(|i| {
            let file = ctx.isecs[i].file as usize;
            ctx.isec_relocs(i).iter().filter_map(move |r| {
                if E::classify_reloc(r.r_type) != RelocClass::Branch || r.size != 4 {
                    return None;
                }
                let id = ctx.reloc_target_sym(file, r)?;
                let sym = &ctx.symbols[id];
                (!sym.is_defined() && is_dtrace_symbol(sym.name()))
                    .then_some((i as u32, r.offset, id))
            })
        })
        .collect();
    (found.into_iter())
        .map(|(isec, offset, sym)| {
            let kind = site_kind_of(ctx.symbols[sym].name())?;
            Some(Site { isec, offset, sym, kind })
        })
        .collect()
}

/// Each provider's sites, as ld-prime groups them: all its probe sites,
/// then all its is-enabled tests, each in site order. The providers
/// come in the order of their first sites in that walk; a site of no
/// provider (a name the prefix ends) belongs to none.
fn sites_by_provider<'a, E: Target>(
    ctx: &Context<E>,
    sites: &'a [Site],
) -> Vec<(&'static [u8], Vec<&'a Site>)> {
    let mut providers: Vec<(&'static [u8], Vec<&Site>)> = Vec::new();
    let mut index: HashMap<&[u8], usize> = HashMap::new();
    let probes = sites.iter().filter(|s| s.kind == SiteKind::Probe);
    let tests = sites.iter().filter(|s| s.kind == SiteKind::IsEnabled);
    for site in probes.chain(tests) {
        let provider = provider_of(ctx.symbols[site.sym].name());
        if provider.is_empty() {
            continue;
        }
        let i = *index.entry(provider).or_insert_with(|| {
            providers.push((provider, Vec::new()));
            providers.len() - 1
        });
        providers[i].1.push(site);
    }
    providers
}

/// The name of each site's subsection, by subsection, which a site
/// goes by as the function it is in: its label (see
/// Context::subsec_label), or "" if it has none. Each object's symbols
/// are looked through once.
fn subsec_names<E: Target>(ctx: &Context<E>, sites: &[Site]) -> HashMap<u32, &'static [u8]> {
    let mut starts: HashMap<u32, HashMap<(u32, u64), u32>> = HashMap::new();
    for site in sites {
        let isec = &ctx.isecs[site.isec as usize];
        let at = (isec.shndx + 1, isec.input_addr as u64);
        starts.entry(isec.file).or_default().insert(at, site.isec);
    }
    let mut best: HashMap<u32, (u8, &'static [u8], usize)> = HashMap::new();
    for (&file, starts) in &starts {
        let obj = &ctx.objs[file as usize];
        for (i, (nlist, &id)) in obj.nlists.iter().zip(&obj.symbols).enumerate() {
            if nlist.is_stab() || nlist.n_type() != N_SECT {
                continue;
            }
            let Some(&isec) = starts.get(&(nlist.n_sect as u32, nlist.n_value)) else {
                continue;
            };
            let name = ctx.symbols[id].name();
            let key = (crate::input_files::subsec_name_rank(nlist, name), name, i);
            best.entry(isec).and_modify(|b| *b = (*b).max(key)).or_insert(key);
        }
    }
    sites.iter().map(|s| (s.isec, best.get(&s.isec).map_or(&[][..], |b| b.1))).collect()
}

/// The name of a provider's DOF section: "__dof_" and the provider, cut
/// to 15 bytes. If an earlier provider's has that name, ld-prime
/// replaces its last byte with '0', then '1', and so on, until it
/// finds one free.
fn section_name(provider: &[u8], taken: &[Vec<u8>]) -> Vec<u8> {
    let mut name = [b"__dof_", provider].concat();
    name.truncate(15);
    if taken.contains(&name) {
        *name.last_mut().unwrap() = b'0';
        while taken.contains(&name) {
            let last = name.last_mut().unwrap();
            *last = last.wrapping_add(1);
        }
    }
    name
}

/// Adds a provider's DOF to the internal object, a section of its own
/// in __TEXT, with relocations that fill each site's slot with its
/// distance from the DOF: a 4-byte SUBTRACTOR pair from the DOF to the
/// site, as ld-prime's fixups are.
fn add_dof_section<E: Target>(
    ctx: &mut Context<E>,
    provider: &[u8],
    name: &[u8],
    dof: Dof,
    sites: &[&Site],
) {
    let mut sectname = [0; 16];
    sectname[..name.len()].copy_from_slice(name);
    let (file, shndx) = ctx.add_synthetic_section(MachSection {
        sectname,
        segname: bytes_to_name(b"__TEXT"),
        flags: S_DTRACE_DOF,
        ..Default::default()
    });
    let id = ctx.isecs.len() as u32;
    let relocs = &mut ctx.objs[file as usize].relocs;
    let rel_offset = relocs.len() as u32;
    for (&slot, site) in dof.slots.iter().zip(sites) {
        let pair = |r_type, target: RelocTarget, addend, is_subtracted| Reloc {
            offset: slot,
            r_type,
            size: 4,
            is_pcrel: false,
            is_subtracted,
            target: target.pack(),
            addend,
        };
        relocs.push(pair(E::RELOC_SUBTRACTOR, RelocTarget::Section(id), 0, false));
        let to = RelocTarget::Section(site.isec);
        relocs.push(pair(E::RELOC_UNSIGNED, to, site.offset as i64, true));
    }
    let nrels = relocs.len() as u32 - rel_offset;
    let bytes: &'static [u8] = Vec::leak(dof.bytes);
    ctx.isecs.push(InputSection {
        file,
        shndx,
        p2align: 0,
        input_addr: 0,
        size: bytes.len() as u32,
        contents: bytes.as_ptr() as usize,
        rel_offset,
        nrels,
        output_section: u32::MAX,
        offset: 0,
        flags: InputSection::flags_alive(),
        replacement: NO_REPLACEMENT,
        unwind_offset: 0,
        nunwind: 0,
    });
    ctx.dof_sections.push(DofSection {
        isec: id,
        sites: sites.iter().map(|s| s.isec).collect(),
        subsec_name: [b"l__dtrace_dof_for_provider_", provider].concat(),
    });
}

/// Whether an object refers to symbol `sym` as undefined. ld-prime
/// counts a DTrace symbol as the first such object's.
pub fn refers_to(obj: &crate::input_files::ObjectFile, sym: SymbolId) -> bool {
    let r = obj.global_range();
    let mut refs = obj.nlists[r.clone()].iter().zip(&obj.symbols[r]);
    refs.any(|(nlist, &id)| id == sym && !nlist.is_stab() && nlist.n_type() == N_UNDF)
}

/// Whether subsection `isec` is a DOF section the link made.
pub fn is_dof<E: Target>(ctx: &Context<E>, isec: &InputSection) -> bool {
    ctx.is_internal(isec.file as usize) && ctx.hdr_of(isec).section_type() == S_DTRACE_DOF
}

/// What libdtrace's dtrace_ld_create_dof makes of a provider (see
/// build_dof): the DOF, and the offset in it of each site's slot.
#[derive(Debug)]
pub struct Dof {
    pub bytes: Vec<u8>,
    pub slots: Vec<u32>,
}

/// Makes a provider's DOF from its symbols as ld-prime has libdtrace
/// make it: `type_names` are the provider's other symbols, sorted and
/// without duplicates, and `probe_names` and `functions` the symbol and
/// the name of the subsection of each site, in the order of the site's
/// slots. Fails with the text libdtrace prints. The names are bytes, as
/// any symbol's, which libdtrace takes as they are.
pub fn build_dof(
    type_names: &[&[u8]],
    probe_names: &[&[u8]],
    functions: &[&[u8]],
) -> Result<Dof, Message> {
    let (stability, typedefs) = check_type_names(type_names)?;
    // The script declares each probe with the arguments the symbol of
    // its first site gives.
    let mut declared = hashbrown::HashSet::new();
    let decls: Vec<&[u8]> = (probe_names.iter().copied())
        .filter(|name| {
            let f = fields(name);
            f[0] == b"___dtrace_probe" && declared.insert(f.get(2).copied())
        })
        .collect();
    let provider = compile(stability, typedefs, &decls)?;
    let probes = register(&provider, probe_names, functions)?;
    Ok(write_dof(&provider, &probes, probe_names.len()))
}

/// The `$`-separated fields of a name as libdtrace reads them: a `$` at
/// the end starts no field.
fn fields(name: &[u8]) -> Vec<&[u8]> {
    name.strip_suffix(b"$").unwrap_or(name).split(|&c| c == b'$').collect()
}

/// The stability and the typedefs symbol of a provider among its other
/// symbols, which must be one of each.
fn check_type_names<'a>(names: &[&'a [u8]]) -> Result<(&'a [u8], &'a [u8]), Message> {
    let mut stability: Option<&[u8]> = None;
    let mut typedefs: Option<&[u8]> = None;
    for &name in names {
        if name.starts_with(b"___dtrace_stability") {
            match stability {
                Some(first) if first != name => {
                    let (first, name) = (raw(first), raw(name));
                    return Err(render(format_args!(
                        "error: Found conflicting dtrace stability info:\n{first}\n{name}\n"
                    )));
                }
                _ => stability = Some(name),
            }
        } else if name.starts_with(b"___dtrace_typedefs") {
            match typedefs {
                Some(first) if first != name => return Err(conflicting_typedefs(first, name)),
                _ => typedefs = Some(name),
            }
        } else {
            let name = raw(name);
            return Err(render(format_args!(
                "error: Found unhandled dtrace typename prefix: {name}\n"
            )));
        }
    }
    let Some(stability) = stability else {
        return Err(b"error: Must have a valid dtrace stability entry\n".to_vec());
    };
    let Some(typedefs) = typedefs else {
        return Err(b"error: Must have a a valid dtrace typedefs entry\n".to_vec());
    };
    Ok((stability, typedefs))
}

/// libdtrace's message for two typedefs symbols of one provider.
fn conflicting_typedefs(first: &[u8], name: &[u8]) -> Message {
    let version = |name| raw(fields(name).get(2).copied().unwrap_or_default());
    if fields(first).get(2) == fields(name).get(2) {
        let (first, name) = (raw(first), raw(name));
        return render(format_args!(
            "error: Found conflicting dtrace typedefs info:\n{first}\n{name}\n"
        ));
    }
    render(format_args!(
        "error: Found dtrace typedefs generated by different versions of dtrace:\n\
         {} ({})\n{} ({})\n\
         Please try regenerating all dtrace created header files with the same version of \
         dtrace before rebuilding your project.\n",
        raw(first),
        version(first),
        raw(name),
        version(name)
    ))
}

/// A string of bytes in hex, two digits each, as `dtrace -h` encodes
/// the argument types and typedef names in the symbol names. libdtrace
/// reads each pair as strtol does, so a pair ends at a non-digit and an
/// odd digit at the end is dropped.
fn unhex(hex: &[u8]) -> Vec<u8> {
    let digit = |c: u8| (c as char).to_digit(16);
    (hex.as_chunks::<2>().0.iter())
        .map(|&[hi, lo]| match (digit(hi), digit(lo)) {
            (Some(hi), Some(lo)) => (hi * 16 + lo) as u8,
            (Some(hi), None) => hi as u8,
            _ => 0,
        })
        .collect()
}

/// A probe's name as DTrace spells it: each "__" a "-", from the left.
fn hyphenate(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    let mut rest = name;
    while let Some(i) = memchr::memmem::find(rest, b"__") {
        out.extend_from_slice(&rest[..i]);
        out.push(b'-');
        rest = &rest[i + 2..];
    }
    out.extend_from_slice(rest);
    out
}

/// D's stability levels and dependency classes, as libdtrace spells
/// them in the attribute pragmas of the script it rebuilds.
const STABILITY_LEVELS: [&str; 8] =
    ["INTERNAL", "PRIVATE", "OBSOLETE", "EXTERNAL", "UNSTABLE", "EVOLVING", "STABLE", "STANDARD"];
const DEPENDENCY_CLASSES: [&str; 6] = ["UNKNOWN", "CPU", "PLATFORM", "GROUP", "ISA", "COMMON"];

/// The attributes a provider has when its stability symbol gives none:
/// D's default, Private/Private/Unknown, for each of the five.
const DEFAULT_ATTRIBUTES: [u32; 5] = [0x0101_0000; 5];

/// A provider as libdtrace's D compiler makes it of the script it
/// rebuilds from the symbols: its name, the attributes of the provider,
/// module, function, name and args of its probes (the stability levels
/// of the names and the data and the dependency class, a byte each, as
/// the DOF holds them), and the D compiler's names for the types of each
/// probe's arguments, by the probe's name as its symbols spell it.
struct Provider {
    name: Vec<u8>,
    attributes: [u32; 5],
    args: HashMap<Vec<u8>, Vec<String>>,
}

/// Rebuilds a provider's D script from its stability and typedefs
/// symbols and the first probe symbol of each of its probes, as
/// libdtrace does, and compiles it: the provider with its typedefs (each
/// an int: the real type is gone), its probes with the types their
/// symbols give, and the attributes. Fails with libdtrace's message,
/// which prints the script, if it doesn't compile.
fn compile(stability: &[u8], typedefs: &[u8], decls: &[&[u8]]) -> Result<Provider, Message> {
    let (text, user_types) = typedef_lines(typedefs);
    let mut script = [b"\n", &text[..]].concat();
    let mut ok = user_types.is_some();
    let user_types = user_types.unwrap_or_default();

    let st = fields(stability);
    let name = st.get(1).copied().unwrap_or_default().to_vec();
    script.extend(render(format_args!("provider {} {{\n", raw(&name))));
    // The digit at the end would be taken for a process ID.
    ok &= is_identifier(&name) && !name.last().is_some_and(u8::is_ascii_digit);
    let mut args = HashMap::new();
    for decl in decls {
        let f = fields(decl);
        if f.get(3) != Some(&&b"v1"[..]) {
            script.extend_from_slice(b"Unhandled probe encoding version\n");
            ok = false;
            continue;
        }
        let types: Vec<Vec<u8>> = f[4..].iter().map(|hex| unhex(hex)).collect();
        let (probe, types_text) = (raw(f[2]), types.join(&b','));
        script.extend(render(format_args!("\tprobe {probe}({});\n", raw(&types_text))));
        match types.iter().map(|t| d_type_name(t, &user_types)).collect() {
            Some(names) => {
                args.insert(f[2].to_vec(), names);
            }
            None => ok = false,
        }
    }
    script.extend_from_slice(b"};\n\n");

    let (text, attributes) = stability_pragmas(&st, &name);
    script.extend(text);
    script.push(b'\n');
    match attributes {
        Some(attributes) if ok => Ok(Provider { name, attributes, args }),
        _ => {
            Err([&b"error: Could not compile reconstructed dtrace script:\n"[..], &script].concat())
        }
    }
}

/// The typedefs of a provider's script, from its typedefs symbol, and
/// their names, None if they don't compile: each name in the symbol a
/// typedef of int, then an empty line.
fn typedef_lines(typedefs: &[u8]) -> (Vec<u8>, Option<Vec<Vec<u8>>>) {
    let fields = fields(typedefs);
    if !matches!(fields.get(2).copied(), Some(b"v1" | b"v2")) {
        return (b"Unhandled typedefs encoding version\n".to_vec(), None);
    }
    let names: Vec<Vec<u8>> = fields[3..].iter().map(|hex| unhex(hex)).collect();
    let mut text: Vec<u8> = names
        .iter()
        .flat_map(|name| render(format_args!("typedef int {};\n", raw(name))))
        .collect();
    text.push(b'\n');
    let ok = names.iter().all(|name| is_identifier(name));
    (text, ok.then_some(names))
}

/// The attribute pragmas of a provider's script, from the fields of its
/// stability symbol, and the attributes they give, None if they don't
/// compile. The fourth field has a digit at every other byte: a level,
/// a level and a class for the provider, module, function, name and
/// args. Without exactly those 29 bytes libdtrace writes a comment and
/// D's defaults hold; with another version, nothing compiles.
fn stability_pragmas(fields: &[&[u8]], provider: &[u8]) -> (Vec<u8>, Option<[u32; 5]>) {
    if fields.len() != 4 || fields[2] != b"v1" {
        return (b"Unhandled stability encoding version\n".to_vec(), None);
    }
    let digits = fields[3];
    if digits.len() != 29 {
        let comment = b"/* Error decoding v1 stability string */\n".to_vec();
        return (comment, Some(DEFAULT_ATTRIBUTES));
    }
    let provider = raw(provider);
    let mut text = Vec::new();
    let mut attributes = Some([0; 5]);
    let what = ["provider", "module", "function", "name", "args"];
    for (k, what) in what.iter().enumerate() {
        let d = |i: usize| digits[6 * k + i].wrapping_sub(b'0') as usize;
        let name = STABILITY_LEVELS.get(d(0));
        let data = STABILITY_LEVELS.get(d(2));
        let class = DEPENDENCY_CLASSES.get(d(4));
        let spell = |s: Option<&&'static str>| *s.unwrap_or(&"ERROR!");
        text.extend(render(format_args!(
            "#pragma D attributes {}/{}/{} provider {provider} {what}\n",
            spell(name),
            spell(data),
            spell(class)
        )));
        match (name, data, class, &mut attributes) {
            (Some(_), Some(_), Some(_), Some(attrs)) => {
                attrs[k] = (d(0) << 24 | d(2) << 16 | d(4) << 8) as u32;
            }
            _ => attributes = None,
        }
    }
    text.push(b'\n');
    (text, attributes)
}

/// Whether a name is a C identifier.
fn is_identifier(name: &[u8]) -> bool {
    let mut bytes = name.iter();
    bytes.next().is_some_and(|&c| c.is_ascii_alphabetic() || c == b'_')
        && bytes.all(|&c| c.is_ascii_alphanumeric() || c == b'_')
}

/// The typedefs D predefines for an LP64 target, and the types they
/// stand for (libdtrace's _dtrace_typedefs_64, and its string).
const D_TYPEDEFS: [(&str, &str); 21] = [
    ("int8_t", "char"),
    ("int16_t", "short"),
    ("int32_t", "int"),
    ("int64_t", "long"),
    ("intptr_t", "long"),
    ("ssize_t", "long"),
    ("uint8_t", "unsigned char"),
    ("uint16_t", "unsigned short"),
    ("uint32_t", "unsigned"),
    ("uint64_t", "unsigned long"),
    ("uchar_t", "unsigned char"),
    ("ushort_t", "unsigned short"),
    ("uint_t", "unsigned"),
    ("ulong_t", "unsigned long"),
    ("u_longlong_t", "unsigned long long"),
    ("ptrdiff_t", "long"),
    ("uintptr_t", "unsigned long"),
    ("size_t", "unsigned long"),
    ("id_t", "unsigned long long"),
    ("pid_t", "int"),
    ("string", "char [256]"),
];

/// D's intrinsic types, by the names its parser builds from the type
/// specifiers (libdtrace's _dtrace_ints and _dtrace_floats).
const D_INTRINSICS: [&str; 22] = [
    "void",
    "signed",
    "unsigned",
    "char",
    "short",
    "int",
    "long",
    "long long",
    "signed char",
    "signed short",
    "signed int",
    "signed long",
    "signed long long",
    "unsigned char",
    "unsigned short",
    "unsigned int",
    "unsigned long",
    "unsigned long long",
    "_Bool",
    "float",
    "double",
    "long double",
];

/// The types whose pointers D's "C" container predefines; a pointer to
/// any other is made in its "D" container, by the name of the type it
/// points to (see d_type_name).
const C_POINTER_TYPES: [&str; 3] = ["void", "char", "int"];

/// The name libdtrace's D compiler gives the type of an argument
/// spelled `spelled` (which `dtrace -h` wrote already in its own
/// spelling), as the DOF has it: ctf_type_name() of the type it makes
/// of it, with `user_types` (the provider's typedefs) as ints. None
/// for what it can't compile, or we can't tell how it would.
///
/// Qualifiers vanish; the integer specifiers come in D's order (an int
/// after short or long goes); a struct, union or enum is a forward
/// declaration, which prints as a struct; a pointer to what is, through
/// typedefs, void, char or int is that one's pointer, and any other
/// pointer is named after the type it points to; a function pointer is
/// int (*)(). A spelling with a byte no type has, UTF-8 or not, is
/// none.
fn d_type_name(spelled: &[u8], user_types: &[Vec<u8>]) -> Option<String> {
    let tokens = type_tokens(std::str::from_utf8(spelled).ok()?)?;
    let (base, resolved, rest) = match tokens.as_slice() {
        ["struct" | "union" | "enum", tag, rest @ ..] if is_identifier(tag.as_bytes()) => {
            (format!("struct {tag}"), None, rest)
        }
        _ => {
            let n = tokens.iter().take_while(|t| is_identifier(t.as_bytes())).count();
            let base = specifiers_name(&tokens[..n])?;
            let resolved = if user_types.iter().any(|t| t == base.as_bytes()) {
                "int"
            } else if let Some(&(_, t)) = D_TYPEDEFS.iter().find(|(name, _)| *name == base) {
                t
            } else {
                *D_INTRINSICS.iter().find(|&&name| name == base)?
            };
            (base, Some(resolved), &tokens[n..])
        }
    };
    if rest.first() == Some(&"(") {
        return (rest.get(1) == Some(&"*")).then(|| "int (*)()".to_string());
    }
    let stars = rest.iter().take_while(|&&t| t == "*").count();
    if stars == 0 {
        let mut dims = String::new();
        for dim in rest.chunks(3) {
            match dim {
                ["[", n, "]"] if n.bytes().all(|c| c.is_ascii_digit()) => dims += &format!("[{n}]"),
                _ => return None,
            }
        }
        return Some(if dims.is_empty() { base } else { format!("{base} {dims}") });
    }
    if stars != rest.len() {
        return None;
    }
    let pointee = match resolved {
        Some(t) if C_POINTER_TYPES.contains(&t) => t,
        _ => &base,
    };
    Some(format!("{pointee} *{}", "*".repeat(stars - 1)))
}

/// The tokens of a type's spelling - identifiers, numbers and the
/// punctuation of declarators - without the qualifiers, or None for a
/// character no type has.
fn type_tokens(spelled: &str) -> Option<Vec<&str>> {
    let mut tokens = Vec::new();
    let mut rest = spelled.trim_start();
    while !rest.is_empty() {
        let len = match rest.as_bytes()[0] {
            b'*' | b'(' | b')' | b'[' | b']' | b',' => 1,
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                rest.bytes().take_while(|&c| c.is_ascii_alphanumeric() || c == b'_').count()
            }
            _ => return None,
        };
        let (token, after) = rest.split_at(len);
        if !matches!(token, "const" | "volatile" | "restrict") {
            tokens.push(token);
        }
        rest = after.trim_start();
    }
    Some(tokens)
}

/// The name D's parser builds from a type's specifiers: signed,
/// unsigned, short, long and long long in that order, then the type
/// name, but for an int that a size specifier makes redundant. None for
/// specifiers that make no type.
fn specifiers_name(specifiers: &[&str]) -> Option<String> {
    let (mut sign, mut short, mut longs, mut name) = (None, false, 0, None);
    for &s in specifiers {
        match s {
            "signed" | "unsigned" if sign.is_none() => sign = Some(s),
            "short" if !short && longs == 0 => short = true,
            "long" if !short && longs < 2 => longs += 1,
            "signed" | "unsigned" | "short" | "long" => return None,
            _ if name.is_none() => name = Some(s),
            _ => return None,
        }
    }
    let mut parts: Vec<&str> = sign.into_iter().collect();
    if short {
        parts.push("short");
    }
    match longs {
        1 => parts.push("long"),
        2 => parts.push("long long"),
        _ => {}
    }
    match name {
        Some("int") if short || longs > 0 => {}
        Some(name) => parts.push(name),
        None => {}
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A probe as libdtrace registers it: its name, the D compiler's names
/// for the types of its arguments, and the functions it has sites in,
/// newest first.
struct Probe {
    name: Vec<u8>,
    args: Vec<String>,
    instances: Vec<Instance>,
}

/// A function a probe has sites in, by name (cut to 127 bytes, as
/// libdtrace keeps it), with the indices of the sites: the probe's
/// sites, and its is-enabled tests.
struct Instance {
    function: Vec<u8>,
    sites: Vec<u32>,
    tests: Vec<u32>,
}

/// Registers each site with its probe as libdtrace does: under the
/// probe's hyphenated name, in the instance of its function - the name
/// of its subsection with one leading underscore less - put first among
/// the probe's when it is new. libdtrace keeps a function name in 128
/// bytes and compares it with the full name, so the sites of a function
/// with a longer one get an instance each. A test of a probe that has
/// no site is an error. Returns the probes in the DOF's order, by name.
fn register(
    provider: &Provider,
    probe_names: &[&[u8]],
    functions: &[&[u8]],
) -> Result<Vec<Probe>, Message> {
    let mut probes: Vec<Probe> = Vec::new();
    let mut index: HashMap<Vec<u8>, usize> = HashMap::new();
    for (i, (&symbol, &function)) in probe_names.iter().zip(functions).enumerate() {
        let field = fields(symbol).get(2).copied().unwrap_or_default();
        let name = hyphenate(field);
        let Some(args) = provider.args.get(field) else {
            return Err(render(format_args!(
                "error: probe {} doesn't exist\nerror: Could not register probes\n",
                raw(&name)
            )));
        };
        let k = *index.entry(name.clone()).or_insert_with(|| {
            probes.push(Probe { name, args: args.clone(), instances: Vec::new() });
            probes.len() - 1
        });
        let function = function.strip_prefix(b"_").unwrap_or(function);
        let instances = &mut probes[k].instances;
        let inst = match instances.iter().position(|inst| inst.function == function) {
            Some(j) => &mut instances[j],
            None => {
                let function = function[..function.len().min(127)].to_vec();
                instances.insert(0, Instance { function, sites: Vec::new(), tests: Vec::new() });
                &mut instances[0]
            }
        };
        if symbol.starts_with(b"___dtrace_isenabled") {
            inst.tests.push(i as u32);
        } else {
            inst.sites.push(i as u32);
        }
    }
    probes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(probes)
}

/// DOF section types.
const DOF_SECT_STRTAB: u32 = 8;
const DOF_SECT_RELTAB: u32 = 10;
const DOF_SECT_URELHDR: u32 = 12;
const DOF_SECT_PROVIDER: u32 = 15;
const DOF_SECT_PROBES: u32 = 16;
const DOF_SECT_PRARGS: u32 = 17;
const DOF_SECT_PROFFS: u32 = 18;
const DOF_SECT_PRENOFFS: u32 = 26;

/// The sizes of a DOF's header (dof_hdr_t), a section header
/// (dof_sec_t), a probe (dof_probe_t) and a relocation
/// (dof_relodesc_t).
const DOF_HDR_SIZE: usize = 64;
const DOF_SEC_SIZE: usize = 32;
const DOF_PROBE_SIZE: usize = 48;
const DOF_RELO_SIZE: usize = 24;

/// A DOF's string table: a NUL, then each string added, whether or not
/// it is there already. An empty one is not added, and is at 0.
struct StringTable(Vec<u8>);

impl StringTable {
    fn add(&mut self, s: &[u8]) -> u32 {
        if s.is_empty() {
            return 0;
        }
        let off = self.0.len() as u32;
        self.0.extend_from_slice(s);
        self.0.push(0);
        off
    }
}

/// Writes a provider's DOF as libdtrace lays it out: the header, the
/// section headers, then the sections' data - the probes (see
/// ProbeTables), the slots of the probe sites and of the is-enabled
/// tests, the provider, the relocations and their header - and the
/// string table last. A slot holds its site's distance from the DOF,
/// which the link fills in (see add_dof_section).
fn write_dof(provider: &Provider, probes: &[Probe], nsites: usize) -> Dof {
    let mut strtab = StringTable(vec![0]);
    let tables = ProbeTables::new(probes, &mut strtab);
    let name = strtab.add(&provider.name);
    let (sites, tests) = (tables.sites.clone(), tables.tests.clone());
    let (bytes, offsets) = lay_out_dof(tables.sections(provider, name), &strtab.0);
    let mut slots = vec![0; nsites];
    for (list, ty) in [(&sites, DOF_SECT_PROFFS), (&tests, DOF_SECT_PRENOFFS)] {
        let base = offsets.iter().find(|&&(t, _)| t == ty).map_or(0, |&(_, off)| off);
        for (k, &i) in list.iter().enumerate() {
            slots[i as usize] = base + 4 * k as u32;
        }
    }
    Dof { bytes, slots }
}

/// The data of a DOF's probe sections, as libdtrace fills them probe by
/// probe, an instance at a time: a dof_probe_t per instance, the
/// mapping of each probe's arguments (the identity: no translator
/// makes them), a relocation per instance (the kernel adds the address
/// the DOF is loaded at to its dofpr_addr), and the indices of the
/// probe sites and the is-enabled tests whose slots come in that order.
/// The strings go in the string table as they come.
struct ProbeTables {
    records: Vec<u8>,
    prargs: Vec<u8>,
    relocs: Vec<u8>,
    sites: Vec<u32>,
    tests: Vec<u32>,
}

impl ProbeTables {
    fn new(probes: &[Probe], strtab: &mut StringTable) -> Self {
        let mut t = ProbeTables {
            records: Vec::new(),
            prargs: Vec::new(),
            relocs: Vec::new(),
            sites: Vec::new(),
            tests: Vec::new(),
        };
        for probe in probes {
            let name = strtab.add(&probe.name);
            let nargv = strtab.0.len() as u32;
            for arg in &probe.args {
                strtab.add(arg.as_bytes());
            }
            let xargv = strtab.0.len() as u32;
            for arg in &probe.args {
                strtab.add(arg.as_bytes());
            }
            let argidx = t.prargs.len() as u32;
            let nargc = probe.args.len() as u8;
            t.prargs.extend(0..nargc);
            for inst in &probe.instances {
                let func = strtab.add(&inst.function);
                put_u32s(&mut t.relocs, &[func, 1]);
                put_u64s(&mut t.relocs, &[t.records.len() as u64, 0]);
                put_u64s(&mut t.records, &[0]);
                let offidx = t.sites.len() as u32;
                put_u32s(&mut t.records, &[func, name, nargv, xargv, argidx, offidx]);
                t.records.extend_from_slice(&[nargc, nargc]);
                t.records.extend_from_slice(&(inst.sites.len() as u16).to_le_bytes());
                put_u32s(&mut t.records, &[t.tests.len() as u32]);
                t.records.extend_from_slice(&(inst.tests.len() as u16).to_le_bytes());
                t.records.extend_from_slice(&[0; 6]);
                t.sites.extend(&inst.sites);
                t.tests.extend(&inst.tests);
            }
        }
        t
    }

    /// The DOF's sections but the string table, in their order: (type,
    /// alignment, entry size, data). Without is-enabled tests there is
    /// no PRENOFFS, and the provider says so with 0 for its index.
    fn sections(self, provider: &Provider, name: u32) -> Vec<(u32, u32, u32, Vec<u8>)> {
        let mut sections = vec![
            (DOF_SECT_PROBES, 8, DOF_PROBE_SIZE as u32, self.records),
            (DOF_SECT_PRARGS, 1, 1, self.prargs),
            (DOF_SECT_PROFFS, 4, 4, vec![0; 4 * self.sites.len()]),
        ];
        let prenoffs = if self.tests.is_empty() { 0 } else { sections.len() as u32 + 1 };
        if !self.tests.is_empty() {
            sections.push((DOF_SECT_PRENOFFS, 4, 4, vec![0; 4 * self.tests.len()]));
        }
        let reltab = sections.len() as u32 + 2;
        let mut record = Vec::new();
        put_u32s(&mut record, &[0, 1, 2, 3, name]);
        put_u32s(&mut record, &provider.attributes);
        put_u32s(&mut record, &[prenoffs]);
        sections.push((DOF_SECT_PROVIDER, 4, 0, record));
        sections.push((DOF_SECT_RELTAB, 8, DOF_RELO_SIZE as u32, self.relocs));
        let mut urelhdr = Vec::new();
        put_u32s(&mut urelhdr, &[0, reltab, 1]);
        sections.push((DOF_SECT_URELHDR, 4, 0, urelhdr));
        sections
    }
}

/// Lays a DOF out: its header, a header for the string table and each
/// of `sections`, their data each at its alignment, and `strtab` right
/// after. Returns the bytes and where each section's data is, by type.
fn lay_out_dof(
    sections: Vec<(u32, u32, u32, Vec<u8>)>,
    strtab: &[u8],
) -> (Vec<u8>, Vec<(u32, u32)>) {
    let nsec = sections.len() + 1;
    let start = DOF_HDR_SIZE + DOF_SEC_SIZE * nsec;
    let mut data: Vec<u8> = Vec::new();
    let mut headers = Vec::new();
    for (ty, align, entsize, bytes) in sections {
        data.resize(crate::util::align_to(data.len() as u64, align as u64) as usize, 0);
        headers.push((ty, align, entsize, (start + data.len()) as u64, bytes.len() as u64));
        data.extend_from_slice(&bytes);
    }
    let strtab_off = (start + data.len()) as u64;
    headers.insert(0, (DOF_SECT_STRTAB, 1, 0, strtab_off, strtab.len() as u64));
    let total = strtab_off + strtab.len() as u64;

    let mut out = b"\x7fDOF".to_vec();
    // LP64, little-endian, DOF version 3, DIF version 2, 8 integer and
    // 8 tuple registers.
    out.extend_from_slice(&[2, 1, 3, 2, 8, 8, 0, 0, 0, 0, 0, 0]);
    put_u32s(&mut out, &[0, DOF_HDR_SIZE as u32, DOF_SEC_SIZE as u32, nsec as u32]);
    put_u64s(&mut out, &[DOF_HDR_SIZE as u64, total, total, 0]);
    for &(ty, align, entsize, off, size) in &headers {
        put_u32s(&mut out, &[ty, align, 1, entsize]);
        put_u64s(&mut out, &[off, size]);
    }
    out.extend_from_slice(&data);
    out.extend_from_slice(strtab);
    (out, headers.iter().map(|h| (h.0, h.3 as u32)).collect())
}

fn put_u32s(out: &mut Vec<u8>, vals: &[u32]) {
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

fn put_u64s(out: &mut Vec<u8>, vals: &[u64]) {
    for v in vals {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAB: &[u8] = b"___dtrace_stability$stab$v1$5_5_4_1_1_0_1_1_0_5_6_5_7_3_2";
    const TYPEDEFS: &[u8] = b"___dtrace_typedefs$stab$v2";

    /// Names as the symbols spell them.
    fn names<'a>(names: &[&'a str]) -> Vec<&'a [u8]> {
        names.iter().map(|name| name.as_bytes()).collect()
    }

    fn bytes(hex: &[&str]) -> Vec<u8> {
        let hex = hex.concat();
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn symbol_kinds_and_providers() {
        assert_eq!(site_kind_of(b"___dtrace_probe$p$x$v1"), Some(SiteKind::Probe));
        assert_eq!(site_kind_of(b"___dtrace_isenabled$p$x$v1"), Some(SiteKind::IsEnabled));
        assert_eq!(site_kind_of(b"___dtrace_probe"), None);
        assert_eq!(provider_of(b"___dtrace_probe$myapp$x$v1"), b"myapp");
        assert_eq!(provider_of(b"___dtrace_isenabled$myapp$x$v1"), b"myapp");
        assert_eq!(provider_of(STAB), b"stab");
        assert_eq!(provider_of(b"___dtrace_probe$noprovider"), b"");
        assert_eq!(provider_of(b"___dtrace_foo"), b"");
        assert!(is_dtrace_symbol(b"___dtrace_"));
        assert!(!is_dtrace_symbol(b"___dtrace"));
        assert!(!is_dtrace_symbol(b"__dtrace_x"));
    }

    /// A probe with an argument, its provider's attributes set: 7
    /// sections, its one site's slot at 0x154.
    #[test]
    fn dof_of_one_site() {
        let probe = names(&["___dtrace_probe$stab$x$v1$696e74"]);
        let dof = build_dof(&[STAB, TYPEDEFS], &probe, &[b"_main"]);
        let dof = dof.unwrap();
        let expected = bytes(&[
            "7f444f4602010302080800000000000000000000400000002000000007000000",
            "4000000000000000c101000000000000c1010000000000000000000000000000",
            "08000000010000000100000000000000ac010000000000001500000000000000",
            "1000000008000000010000003000000020010000000000003000000000000000",
            "1100000001000000010000000100000050010000000000000100000000000000",
            "1200000004000000010000000400000054010000000000000400000000000000",
            "0f00000004000000010000000000000058010000000000002c00000000000000",
            "0a00000008000000010000001800000088010000000000001800000000000000",
            "0c000000040000000100000000000000a0010000000000000c00000000000000",
            "00000000000000000b0000000100000003000000070000000000000000000000",
            "0101010000000000000000000000000000000000000000000000000001000000",
            "0200000003000000100000000004050500000101000001010005060500020307",
            "00000000000000000b0000000100000000000000000000000000000000000000",
            "000000000500000001000000007800696e7400696e74006d61696e0073746162",
            "00",
        ]);
        assert_eq!(dof.bytes, expected);
        assert_eq!(dof.slots, [0x154]);
    }

    /// Probes of a function, one with an is-enabled test too: 8
    /// sections, the probes by name, the tests' slots after the sites'.
    #[test]
    fn dof_with_is_enabled_tests() {
        let types = [
            "___dtrace_stability$myapp$v1$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0",
            "___dtrace_typedefs$myapp$v2",
        ];
        let probes = [
            "___dtrace_probe$myapp$request__start$v1$696e74$63686172202a",
            "___dtrace_probe$myapp$request__done$v1$696e74",
            "___dtrace_probe$myapp$noargs$v1",
            "___dtrace_isenabled$myapp$request__done$v1",
        ];
        let dof = build_dof(&names(&types), &names(&probes), &[&b"_main"[..]; 4]).unwrap();
        let expected = bytes(&[
            "7f444f4602010302080800000000000000000000400000002000000008000000",
            "4000000000000000ba02000000000000ba020000000000000000000000000000",
            "0800000001000000010000000000000064020000000000005600000000000000",
            "1000000008000000010000003000000040010000000000009000000000000000",
            "11000000010000000100000001000000d0010000000000000300000000000000",
            "12000000040000000100000004000000d4010000000000000c00000000000000",
            "1a000000040000000100000004000000e0010000000000000400000000000000",
            "0f000000040000000100000000000000e4010000000000002c00000000000000",
            "0a00000008000000010000001800000010020000000000004800000000000000",
            "0c00000004000000010000000000000058020000000000000c00000000000000",
            "0000000000000000080000000100000008000000080000000000000000000000",
            "000001000000000000000000000000000000000000000000220000000d000000",
            "1a0000001e000000000000000100000001010100000000000100000000000000",
            "00000000000000004b0000002700000035000000400000000100000002000000",
            "0202010001000000000000000000000000000100000000000000000000000000",
            "0000000000000000010000000200000003000000500000000000010100000101",
            "0000010100000101000001010400000008000000010000000000000000000000",
            "0000000000000000220000000100000030000000000000000000000000000000",
            "4b00000001000000600000000000000000000000000000000000000006000000",
            "01000000006e6f61726773006d61696e00726571756573742d646f6e6500696e",
            "7400696e74006d61696e00726571756573742d737461727400696e7400636861",
            "72202a00696e740063686172202a006d61696e006d7961707000",
        ]);
        assert_eq!(dof.bytes, expected);
        assert_eq!(dof.slots, [0x1dc, 0x1d8, 0x1d4, 0x1e0]);
    }

    /// The D compiler's names for what dtrace -h spells, as ld-prime's
    /// DOFs have them.
    #[test]
    fn d_type_names() {
        let user = [b"myint_t".to_vec(), b"foo_t".to_vec()];
        let cases = [
            ("unsigned", "unsigned"),
            ("signed", "signed"),
            ("unsigned int", "unsigned int"),
            ("int unsigned", "unsigned int"),
            ("long int", "long"),
            ("unsigned long int", "unsigned long"),
            ("short int", "short"),
            ("long long int", "long long"),
            ("long double", "long double"),
            ("_Bool", "_Bool"),
            ("const int", "int"),
            ("int64_t *", "int64_t *"),
            ("int8_t *", "char *"),
            ("int8_t **", "char **"),
            ("uintptr_t *", "uintptr_t *"),
            ("uint64_t **", "uint64_t **"),
            ("unsigned long *", "unsigned long *"),
            ("pid_t *", "int *"),
            ("string", "string"),
            ("union u *", "struct u *"),
            ("union u", "struct u"),
            ("enum e", "struct e"),
            ("const struct s *", "struct s *"),
            ("struct s **", "struct s **"),
            ("myint_t *", "int *"),
            ("const myint_t *", "int *"),
            ("myint_t **", "int **"),
            ("foo_t", "foo_t"),
            ("volatile foo_t", "foo_t"),
            ("void *", "void *"),
            ("void **", "void **"),
            ("char * *", "char **"),
            ("const char *", "char *"),
            ("char * const *", "char **"),
            ("unsigned char * const", "unsigned char *"),
            ("const unsigned char *", "unsigned char *"),
            ("int (*)()", "int (*)()"),
            ("void (*)(int)", "int (*)()"),
            ("int [4]", "int [4]"),
        ];
        for (spelled, name) in cases {
            assert_eq!(d_type_name(spelled.as_bytes(), &user).as_deref(), Some(name), "{spelled}");
        }
        for spelled in ["bar_t", "unsigned myint_t", "long char", "int int", "*", "int @"] {
            assert_eq!(d_type_name(spelled.as_bytes(), &user), None, "{spelled}");
        }
    }

    /// A probe's instances come newest first, those of a function's name
    /// one; one whose name is longer than 127 bytes gets an instance per
    /// site, the name cut.
    #[test]
    fn instances() {
        let long = format!("_{}", "x".repeat(130));
        let probes = [&b"___dtrace_probe$stab$x$v1"[..]; 5];
        let functions = names(&["_fa", "_fb", "_fa", &long, &long]);
        let provider = compile(STAB, TYPEDEFS, &probes[..1]).unwrap();
        let probes = register(&provider, &probes, &functions).unwrap();
        let names: Vec<&[u8]> = probes[0].instances.iter().map(|i| &i.function[..]).collect();
        let cut = &long.as_bytes()[1..128];
        assert_eq!(names, [cut, cut, b"fb".as_slice(), b"fa".as_slice()]);
        assert_eq!(probes[0].instances[3].sites, [0, 2]);
    }

    #[test]
    fn section_names() {
        let providers = [
            "longprovi",
            "longprovidera",
            "longprov",
            "longprovid",
            "longproviderc",
            "longproviderb",
        ];
        let mut taken: Vec<Vec<u8>> = Vec::new();
        for p in providers {
            let name = section_name(p.as_bytes(), &taken);
            taken.push(name);
        }
        let names: Vec<&str> = taken.iter().map(|n| std::str::from_utf8(n).unwrap()).collect();
        let expected = [
            "__dof_longprovi",
            "__dof_longprov0",
            "__dof_longprov",
            "__dof_longprov1",
            "__dof_longprov2",
            "__dof_longprov3",
        ];
        assert_eq!(names, expected);
    }

    #[test]
    fn errors() {
        let probe: [&[u8]; 1] = [b"___dtrace_probe$stab$x$v1"];
        let err = |types: &[&[u8]], probes: &[&[u8]]| {
            let msg = build_dof(types, probes, &vec![&b"_main"[..]; probes.len()]).unwrap_err();
            String::from_utf8(msg).unwrap()
        };
        assert_eq!(err(&[TYPEDEFS], &probe), "error: Must have a valid dtrace stability entry\n");
        assert_eq!(err(&[STAB], &probe), "error: Must have a a valid dtrace typedefs entry\n");
        assert_eq!(
            err(&[b"___dtrace_foo$stab$", STAB, TYPEDEFS], &probe),
            "error: Found unhandled dtrace typename prefix: ___dtrace_foo$stab$\n"
        );
        assert_eq!(
            err(&[STAB, TYPEDEFS], &[b"___dtrace_isenabled$stab$a__b$v1"]),
            "error: probe a-b doesn't exist\nerror: Could not register probes\n"
        );
        let td1 = b"___dtrace_typedefs$stab$v1";
        let msg = err(&[STAB, td1, TYPEDEFS], &probe);
        assert!(
            msg.contains("\n___dtrace_typedefs$stab$v1 (v1)\n___dtrace_typedefs$stab$v2 (v2)\n")
        );

        // A D script that doesn't compile is printed.
        let msg = err(&[STAB, TYPEDEFS], &[b"___dtrace_probe$stab$x$v1$666f6f5f74"]);
        assert!(msg.starts_with(
            "error: Could not compile reconstructed dtrace script:\n\n\nprovider stab {\n\
             \tprobe x(foo_t);\n};\n\n\
             #pragma D attributes EVOLVING/EVOLVING/ISA provider stab provider\n"
        ));
        assert!(msg.ends_with("STANDARD/EXTERNAL/PLATFORM provider stab args\n\n\n"));

        // A stability string of another length leaves D's defaults.
        let short = b"___dtrace_stability$stab$v1$1_1_0";
        assert!(build_dof(&[short, TYPEDEFS], &probe, &[b"_main"]).is_ok());
    }
}
