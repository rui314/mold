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
//! probe and the function it is in. We make the DOF from the symbols
//! ourselves, in libdtrace's layout (see build_dof). The DOF leaves a
//! slot for each site's distance from it, which a pair of relocations
//! fills in once the output is laid out.
//!
//! ld-prime makes the DOF sections in the order of a hash table of the
//! providers; we make them in the order of their first sites.

use hashbrown::HashMap;
use rayon::prelude::*;

use crate::context::Context;
use crate::error::raw;
use crate::input_sections::{InputSection, Reloc, RelocTarget};
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
/// name with the prefix), a call of which is no site.
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

/// A provider's DOF section, as the link makes it: the subsections of
/// the sites it refers to (in site order).
#[derive(Debug)]
pub struct DofSection {
    pub sites: Vec<u32>,
}

/// Whether a branch to symbol `sym` is a probe site, and of which kind:
/// a branch to a probe or an is-enabled test, in a link that makes DOF.
/// Another branch to a DTrace symbol goes to address 0, as ld-prime has
/// it.
#[inline]
pub fn site_kind<E: Target>(ctx: &Context<E>, sym: SymbolId) -> Option<SiteKind> {
    let sym = &ctx.symbols[sym];
    if sym.is_defined() || !ctx.args.dtrace_dof {
        return None;
    }
    site_kind_of(sym.name())
}

/// A probe site: a branch to a probe or an is-enabled test, at offset
/// `offset` of subsection `isec` (the relocated field's).
struct Site {
    isec: u32,
    offset: u32,
    sym: SymbolId,
}

/// Makes a DOF section for each provider some live code has a probe site
/// of, once dead stripping is done. (ld-prime makes them before, so that
/// a function nothing calls stays for its probe sites.)
pub fn create_dof_sections<E: Target>(ctx: &mut Context<E>) {
    if !ctx.args.dtrace_dof || ctx.args.relocatable {
        return;
    }
    let syms = dtrace_symbols(ctx);
    if syms.is_empty() {
        return;
    }
    let sites = collect_sites(ctx);
    let names: Vec<&[u8]> = syms.iter().map(|&id| ctx.symbols[id].name()).collect();
    let mut taken: Vec<Vec<u8>> = Vec::new();
    for (provider, sites) in sites_by_provider(ctx, &sites) {
        // The provider's stability and typedefs symbols, the first of
        // each if there are more.
        let info = |prefix: &[u8]| {
            (names.iter().copied())
                .find(|&name| name.starts_with(prefix) && provider_of(name) == provider)
        };
        let (stability, typedefs) = (info(b"___dtrace_stability$"), info(b"___dtrace_typedefs$"));
        let probes: Vec<&[u8]> = sites.iter().map(|s| ctx.symbols[s.sym].name()).collect();
        // A site goes by the function it is in, its subsection's label.
        let functions: Vec<&[u8]> =
            sites.iter().map(|s| ctx.subsec_label(s.isec as usize).unwrap_or_default()).collect();
        let dof = match build_dof(provider, stability, typedefs, &probes, &functions) {
            Ok(dof) => dof,
            Err(name) => {
                crate::error!("{}: unsupported DTrace probe encoding", raw(name));
                return;
            }
        };
        let name = section_name(provider, &taken);
        add_dof_section(ctx, &name, dof, &sites);
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

/// The probe sites: by file in input order, by section and address in a
/// file (the subsections' order), and by offset in a subsection. A copy
/// of a function another one replaced (a weak definition another file's
/// won) has none.
fn collect_sites<E: Target>(ctx: &Context<E>) -> Vec<Site> {
    (0..ctx.isecs.len())
        .into_par_iter()
        .filter(|&i| {
            let isec = &ctx.isecs[i];
            isec.is_emitted()
        })
        .flat_map_iter(|i| {
            let file = ctx.isecs[i].file as usize;
            ctx.isec_relocs(i).iter().filter_map(move |r| {
                if E::classify_reloc(r.r_type) != RelocClass::Branch || r.size != 4 {
                    return None;
                }
                let sym = ctx.reloc_target_sym(file, r)?;
                let (defined, name) = (ctx.symbols[sym].is_defined(), ctx.symbols[sym].name());
                (!defined && site_kind_of(name).is_some()).then_some(Site {
                    isec: i as u32,
                    offset: r.offset,
                    sym,
                })
            })
        })
        .collect()
}

/// Each provider's sites, in site order, the providers in the order of
/// their first sites; a site of no provider (a name the prefix ends)
/// belongs to none.
fn sites_by_provider<'a, E: Target>(
    ctx: &Context<E>,
    sites: &'a [Site],
) -> Vec<(&'static [u8], Vec<&'a Site>)> {
    let mut providers: Vec<(&'static [u8], Vec<&Site>)> = Vec::new();
    let mut index: HashMap<&[u8], usize> = HashMap::new();
    for site in sites {
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
fn add_dof_section<E: Target>(ctx: &mut Context<E>, name: &[u8], dof: Dof, sites: &[&Site]) {
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
        rel_offset,
        nrels,
        ..InputSection::new(file, shndx, 0, bytes.len() as u32, bytes)
    });
    ctx.dof_sections.push(DofSection { sites: sites.iter().map(|s| s.isec).collect() });
}

/// What libdtrace's dtrace_ld_create_dof makes of a provider (see
/// build_dof): the DOF, and the offset in it of each site's slot.
#[derive(Debug)]
pub struct Dof {
    pub bytes: Vec<u8>,
    pub slots: Vec<u32>,
}

/// Makes a provider's DOF from its symbols as libdtrace would make it:
/// `stability` and `typedefs` are its symbols of those, if it has them,
/// and `probe_names` and `functions` the symbol and the name of the
/// subsection of each site, in the order of the site's slots. Fails
/// with the symbol of a probe of an encoding other than v1, whose fields
/// would be misread. The names are bytes, as any symbol's.
pub fn build_dof<'a>(
    provider: &[u8],
    stability: Option<&[u8]>,
    typedefs: Option<&[u8]>,
    probe_names: &[&'a [u8]],
    functions: &[&[u8]],
) -> Result<Dof, &'a [u8]> {
    let typedefs = typedefs
        .map_or(Vec::new(), |name| fields(name).iter().skip(3).map(|hex| unhex(hex)).collect());
    let probes = register(probe_names, functions, &typedefs)?;
    Ok(write_dof(provider, attributes(stability), &probes, probe_names.len()))
}

/// The `$`-separated fields of a name as libdtrace reads them: a `$` at
/// the end starts no field.
fn fields(name: &[u8]) -> Vec<&[u8]> {
    name.strip_suffix(b"$").unwrap_or(name).split(|&c| c == b'$').collect()
}

/// A string of bytes in hex, two digits each, as `dtrace -h` encodes
/// the argument types and typedef names in the symbol names.
fn unhex(hex: &[u8]) -> Vec<u8> {
    let digit = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    hex.as_chunks::<2>().0.iter().map(|&[hi, lo]| digit(hi) << 4 | digit(lo)).collect()
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

/// The attributes a provider has when its stability symbol gives none:
/// D's default, Private/Private/Unknown, for each of the five.
const DEFAULT_ATTRIBUTES: [u32; 5] = [0x0101_0000; 5];

/// The attributes of a provider and of the module, function, name and
/// args of its probes, from its stability symbol: the last field has 15
/// digits, a stability level for the names, one for the data and a
/// dependency class each, which the DOF holds a byte each of.
fn attributes(stability: Option<&[u8]>) -> [u32; 5] {
    let digits: Vec<u32> = stability.map_or(Vec::new(), |name| {
        let f = fields(name);
        let digits = f.get(3).copied().unwrap_or_default();
        digits.iter().step_by(2).map(|&c| c.wrapping_sub(b'0') as u32).collect()
    });
    if digits.len() != 15 {
        return DEFAULT_ATTRIBUTES;
    }
    std::array::from_fn(|k| digits[3 * k] << 24 | digits[3 * k + 1] << 16 | digits[3 * k + 2] << 8)
}

/// An argument's type as the DOF names it: as `dtrace -h` spelled it in
/// the symbol, but for each name of the provider's `typedefs`, which is
/// an int (the type it stood for is gone). D compiles the name when a
/// script uses the argument.
fn arg_type(spelled: &[u8], typedefs: &[Vec<u8>]) -> Vec<u8> {
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    (spelled.chunk_by(|&a, &b| is_ident(a) && is_ident(b)))
        .flat_map(|token| if typedefs.iter().any(|t| t == token) { b"int" } else { token })
        .copied()
        .collect()
}

/// A probe: its name, the types of its arguments, none if no probe site
/// declares them, and the functions it has sites in.
struct Probe {
    name: Vec<u8>,
    args: Option<Vec<Vec<u8>>>,
    instances: Vec<Instance>,
}

/// A function a probe has sites in, by name, with the indices of the
/// sites: the probe's sites, and its is-enabled tests.
struct Instance {
    function: Vec<u8>,
    sites: Vec<u32>,
    tests: Vec<u32>,
}

/// Registers each site with its probe, by the probe's hyphenated name,
/// in the instance of its function: the name of its subsection with one
/// leading underscore less, cut to the 127 bytes the kernel takes. The
/// probes and their instances come in the order of their first sites. A
/// probe's arguments are those the symbol of its first probe site gives
/// (see arg_type).
fn register<'a>(
    probe_names: &[&'a [u8]],
    functions: &[&[u8]],
    typedefs: &[Vec<u8>],
) -> Result<Vec<Probe>, &'a [u8]> {
    let mut probes: Vec<Probe> = Vec::new();
    let mut index: HashMap<&[u8], usize> = HashMap::new();
    for (i, (&symbol, &function)) in probe_names.iter().zip(functions).enumerate() {
        let f = fields(symbol);
        let field = f.get(2).copied().unwrap_or_default();
        let k = *index.entry(field).or_insert_with(|| {
            probes.push(Probe { name: hyphenate(field), args: None, instances: Vec::new() });
            probes.len() - 1
        });
        let test = symbol.starts_with(IS_ENABLED_PREFIX);
        if !test && probes[k].args.is_none() {
            if f.get(3) != Some(&&b"v1"[..]) {
                return Err(symbol);
            }
            probes[k].args =
                Some(f[4..].iter().map(|hex| arg_type(&unhex(hex), typedefs)).collect());
        }
        let function = function.strip_prefix(b"_").unwrap_or(function);
        let function = &function[..function.len().min(127)];
        let instances = &mut probes[k].instances;
        let j = match instances.iter().position(|inst| inst.function == function) {
            Some(j) => j,
            None => {
                let function = function.to_vec();
                instances.push(Instance { function, sites: Vec::new(), tests: Vec::new() });
                instances.len() - 1
            }
        };
        let inst = &mut instances[j];
        if test {
            inst.tests.push(i as u32);
        } else {
            inst.sites.push(i as u32);
        }
    }
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
fn write_dof(provider: &[u8], attributes: [u32; 5], probes: &[Probe], nsites: usize) -> Dof {
    let mut strtab = StringTable(vec![0]);
    let tables = ProbeTables::new(probes, &mut strtab);
    let name = strtab.add(provider);
    let (sites, tests) = (tables.sites.clone(), tables.tests.clone());
    let (bytes, offsets) = lay_out_dof(tables.sections(&attributes, name), &strtab.0);
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
            let args = probe.args.as_deref().unwrap_or_default();
            let nargv = strtab.0.len() as u32;
            for arg in args {
                strtab.add(arg);
            }
            let xargv = strtab.0.len() as u32;
            for arg in args {
                strtab.add(arg);
            }
            let argidx = t.prargs.len() as u32;
            let nargc = args.len() as u8;
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
    fn sections(self, attributes: &[u32; 5], name: u32) -> Vec<(u32, u32, u32, Vec<u8>)> {
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
        put_u32s(&mut record, attributes);
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
        let dof = build_dof(b"stab", Some(STAB), Some(TYPEDEFS), &probe, &[b"_main"]);
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
    /// sections, the probes in the order of their first sites, the tests'
    /// slots after the sites'.
    #[test]
    fn dof_with_is_enabled_tests() {
        let stability = b"___dtrace_stability$myapp$v1$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0";
        let probes = [
            "___dtrace_probe$myapp$request__start$v1$696e74$63686172202a",
            "___dtrace_probe$myapp$request__done$v1$696e74",
            "___dtrace_probe$myapp$noargs$v1",
            "___dtrace_isenabled$myapp$request__done$v1",
        ];
        let dof = build_dof(b"myapp", Some(stability), None, &names(&probes), &[&b"_main"[..]; 4]);
        let dof = dof.unwrap();
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
            "000000000000000025000000010000000f0000001a0000000000000000000000",
            "0202010000000000000000000000000000000000000000003f0000002a000000",
            "370000003b000000020000000100000001010100000000000100000000000000",
            "00000000000000004b000000440000004b0000004b0000000300000002000000",
            "0000010001000000000000000000000000010000000000000000000000000000",
            "0000000000000000010000000200000003000000500000000000010100000101",
            "0000010100000101000001010400000025000000010000000000000000000000",
            "00000000000000003f0000000100000030000000000000000000000000000000",
            "4b00000001000000600000000000000000000000000000000000000006000000",
            "0100000000726571756573742d737461727400696e740063686172202a00696e",
            "740063686172202a006d61696e00726571756573742d646f6e6500696e740069",
            "6e74006d61696e006e6f61726773006d61696e006d7961707000",
        ]);
        assert_eq!(dof.bytes, expected);
        assert_eq!(dof.slots, [0x1d4, 0x1d8, 0x1dc, 0x1e0]);
    }

    /// A probe's arguments as dtrace -h spells them, but the provider's
    /// typedefs, which are ints.
    #[test]
    fn arg_types() {
        let user = [b"myint_t".to_vec(), b"foo_t".to_vec()];
        let cases = [
            ("unsigned long int", "unsigned long int"),
            ("const char *", "const char *"),
            ("myint_t", "int"),
            ("const myint_t **", "const int **"),
            ("myint_t2 *", "myint_t2 *"),
            ("void (*)(foo_t)", "void (*)(int)"),
        ];
        for (spelled, name) in cases {
            assert_eq!(arg_type(spelled.as_bytes(), &user), name.as_bytes(), "{spelled}");
        }
    }

    /// A probe's instances come in the order of their first sites, one
    /// per function, a name longer than 127 bytes cut.
    #[test]
    fn instances() {
        let long = format!("_{}", "x".repeat(130));
        let probes = [&b"___dtrace_probe$stab$x$v1"[..]; 5];
        let functions = names(&["_fa", "_fb", "_fa", &long, &long]);
        let probes = register(&probes, &functions, &[]).unwrap();
        let names: Vec<&[u8]> = probes[0].instances.iter().map(|i| &i.function[..]).collect();
        let cut = &long.as_bytes()[1..128];
        assert_eq!(names, [b"fa".as_slice(), b"fb".as_slice(), cut]);
        assert_eq!(probes[0].instances[0].sites, [0, 2]);
        assert_eq!(probes[0].instances[2].sites, [3, 4]);
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

    /// A provider without a stability symbol has D's default attributes;
    /// a probe that only is-enabled tests name has no arguments; a
    /// probe of another encoding than v1 can't be read.
    #[test]
    fn defaults_and_refusals() {
        assert_eq!(attributes(None), DEFAULT_ATTRIBUTES);
        assert_eq!(attributes(Some(b"___dtrace_stability$stab$v1$1_1_0")), DEFAULT_ATTRIBUTES);
        let stab = [0x0505_0400, 0x0101_0000, 0x0101_0000, 0x0506_0500, 0x0703_0200];
        assert_eq!(attributes(Some(STAB)), stab);

        let test: [&[u8]; 1] = [b"___dtrace_isenabled$stab$a__b$v1"];
        let probes = register(&test, &[b"_main"], &[]).unwrap();
        assert_eq!(probes[0].name, b"a-b");
        assert!(probes[0].args.is_none());
        assert_eq!(probes[0].instances[0].tests, [0]);
        assert!(build_dof(b"stab", None, None, &test, &[b"_main"]).is_ok());

        let v2: [&[u8]; 1] = [b"___dtrace_probe$stab$x$v2$696e74"];
        let err = build_dof(b"stab", Some(STAB), Some(TYPEDEFS), &v2, &[b"_main"]).unwrap_err();
        assert_eq!(err, v2[0]);
    }
}
