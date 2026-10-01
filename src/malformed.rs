//! The checks ld-prime makes of a Mach-O object file's layout before it
//! reads the file, in its order and its words: the load commands, one by
//! one, then those that must not repeat, the platforms, the segment and
//! its sections, and last the tables at the file's end. A file that
//! passes is one whose load commands and tables the readers can take as
//! they are (input_files::stage_object checks what it reads beyond:
//! symbols' sections, relocations and such).

use crate::macho::*;

/// What is wrong with an object file, as ld-prime words it.
pub enum Malformed {
    /// Its load commands are no Mach-O file's. An archive member so is
    /// none of the link's: ld-prime passes over it without a word.
    LoadCommands(String),
    /// Anything after: repeated load commands, its platforms, its
    /// segment and sections, the tables at its end.
    Layout(String),
}

impl Malformed {
    pub fn message(&self) -> &str {
        match self {
            Malformed::LoadCommands(msg) | Malformed::Layout(msg) => msg,
        }
    }
}

const LC_SEGMENT: u32 = 0x1;
const LC_ROUTINES: u32 = 0x11;
const LC_SUB_UMBRELLA: u32 = 0x13;
const LC_SUB_LIBRARY: u32 = 0x15;
const LC_ENCRYPTION_INFO: u32 = 0x21;
const LC_FUNCTION_VARIANTS: u32 = 0x37;
const LC_FUNCTION_VARIANT_FIXUPS: u32 = 0x38;
const LC_TARGET_TRIPLE: u32 = 0x39;
const LC_FILESET_ENTRY: u32 = 0x35 | LC_REQ_DYLD;

/// The checks of an object file `data` of CPU type `cputype` (see the
/// module comment). Warns of the load commands only an image has that
/// an object may carry all the same, such as LC_RPATH.
pub fn check_object(data: &[u8], cputype: u32) -> Result<(), Malformed> {
    let cmds = load_commands(data, cputype).map_err(Malformed::LoadCommands)?;
    check_repeats(&cmds).map_err(Malformed::Layout)?;
    check_platforms(&cmds, cputype).map_err(Malformed::Layout)?;
    check_segments(data, &cmds).map_err(Malformed::Layout)?;
    check_linkedit(data, &cmds).map_err(Malformed::Layout)?;
    check_section_contents(data, &cmds).map_err(Malformed::Layout)
}

/// What ld-prime refuses of an image the link reads - a dylib, the
/// executable -bundle_loader names - before it reads it: one without
/// its one LC_UUID. (It checks much else of an image's layout too.)
pub fn check_image(data: &[u8]) -> Result<(), String> {
    let mut uuids = 0;
    let mut off = size_of::<MachHeader>();
    for _ in 0..u32_at(data, 16) {
        let size = u32_at(data, off + 4) as usize;
        if size < 8 || off + size > data.len() {
            break;
        }
        uuids += (u32_at(data, off) == LC_UUID) as u32;
        off += size;
    }
    match uuids {
        0 => Err("missing LC_UUID load command".to_string()),
        1 => Ok(()),
        _ => Err("too many LC_UUID load commands".to_string()),
    }
}

fn u32_at(data: &[u8], off: usize) -> u32 {
    data.get(off..off + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// A load command: its type and bytes.
struct Cmd<'a> {
    cmd: u32,
    data: &'a [u8],
}

/// Walks the load commands, checking each as ld-prime does - first that
/// it lies within the load commands, then what it holds -, numbering
/// them from 0.
fn load_commands(data: &[u8], cputype: u32) -> Result<Vec<Cmd<'_>>, String> {
    let ncmds = u32_at(data, 16);
    let cmds_end = size_of::<MachHeader>() as u64 + u32_at(data, 20) as u64;
    if cmds_end > data.len() as u64 {
        return Err("mh.sizeofcmds extends beyond buffer size".to_string());
    }
    let mut cmds = Vec::new();
    let mut off = size_of::<MachHeader>() as u64;
    for i in 0..ncmds {
        let malformed = |what: String| {
            format!(
                "malformed load command ({i} of {ncmds}) at offset=0x{off:X} with mh={:p}, {what}",
                data.as_ptr()
            )
        };
        if off + 8 >= cmds_end {
            return Err(malformed("off end of load commands".to_string()));
        }
        let (cmd, size) = (u32_at(data, off as usize), u32_at(data, off as usize + 4));
        if size % 4 != 0 {
            return Err(malformed(format!("cmdsize=0x{size:X} is not pointer sized")));
        }
        if size < 8 {
            return Err(malformed(format!("size (0x{size:X}) too small")));
        }
        if off + size as u64 > cmds_end {
            return Err(malformed(format!(
                "size (0x{size:X}) is too large, load commands end at offset 0x{cmds_end:X}"
            )));
        }
        let cmd = Cmd { cmd, data: &data[off as usize..(off + size as u64) as usize] };
        check_load_command(i, &cmd, cputype)?;
        cmds.push(cmd);
        off += size as u64;
    }
    Ok(cmds)
}

/// The names ld-prime gives the load commands an object has no business
/// with in its words.
fn command_name(cmd: u32) -> &'static str {
    match cmd {
        LC_ID_DYLIB => "LC_ID_DYLIB",
        LC_LOAD_DYLIB => "LC_LOAD_DYLIB",
        LC_LOAD_WEAK_DYLIB => "LC_LOAD_WEAK_DYLIB",
        LC_REEXPORT_DYLIB => "LC_REEXPORT_DYLIB",
        LC_LOAD_UPWARD_DYLIB => "LC_LOAD_UPWARD_DYLIB",
        LC_RPATH => "LC_RPATH",
        LC_SUB_UMBRELLA => "LC_SUB_UMBRELLA",
        LC_SUB_FRAMEWORK => "LC_SUB_FRAMEWORK",
        LC_SUB_CLIENT => "LC_SUB_CLIENT",
        LC_SUB_LIBRARY => "LC_SUB_LIBRARY",
        LC_ID_DYLINKER => "LC_ID_DYLINKER",
        LC_LOAD_DYLINKER => "LC_LOAD_DYLINKER",
        LC_DYLD_ENVIRONMENT => "LC_DYLD_ENVIRONMENT",
        _ => "LC_???",
    }
}

/// Checks load command `i` of an object for what ld-prime refuses in
/// it: one only a dylib has, one an object can't have, one of the wrong
/// size, tables too large to be, strings that run out of it. The other
/// commands only an image has draw a warning.
fn check_load_command(i: u32, cmd: &Cmd, cputype: u32) -> Result<(), String> {
    let size = cmd.data.len();
    let word = |off: usize| u32_at(cmd.data, off);
    let wrong = |name: &str| Err(format!("load command #{i} {name} size wrong"));
    match cmd.cmd {
        LC_ID_DYLIB | LC_LOAD_DYLIB | LC_LOAD_WEAK_DYLIB | LC_REEXPORT_DYLIB
        | LC_LOAD_UPWARD_DYLIB => {
            Err(format!("load command #{i} {} not supported in .o files", command_name(cmd.cmd)))
        }
        LC_RPATH | LC_SUB_UMBRELLA | LC_SUB_FRAMEWORK | LC_SUB_CLIENT | LC_SUB_LIBRARY
        | LC_ID_DYLINKER | LC_LOAD_DYLINKER | LC_DYLD_ENVIRONMENT => {
            let name = command_name(cmd.cmd);
            crate::warn!("load command #{i} {name} not supported in .o files");
            Ok(())
        }
        LC_TARGET_TRIPLE => {
            let off = word(8);
            if off as usize >= size {
                return Err(format!(
                    "load command #{i} string offset ({off}) outside cmd size ({size})"
                ));
            }
            match cmd.data[off as usize..].contains(&0) {
                true => Ok(()),
                false => {
                    Err(format!("load command #{i} string extends beyond end of load command"))
                }
            }
        }
        LC_LINKER_OPTION => check_linker_option(i, cmd),
        LC_SYMTAB if size != size_of::<SymtabCommand>() => wrong("LC_SYMTAB"),
        LC_SYMTAB if word(12) > 0x1000_0000 => {
            Err("malformed mach-o image: symbol table too large".to_string())
        }
        LC_DYSYMTAB => check_dysymtab(i, cmd),
        LC_ENCRYPTION_INFO if size != 20 => wrong("LC_ENCRYPTION_INFO"),
        LC_ENCRYPTION_INFO_64 if size != 24 => wrong("LC_ENCRYPTION_INFO_64"),
        LC_DYLD_INFO | LC_DYLD_INFO_ONLY => Err("LC_DYLD_INFO not allowed in .o files".to_string()),
        LC_UUID if size != 24 => wrong("LC_UUID"),
        LC_MAIN => Err("LC_MAIN not allowed in .o files".to_string()),
        LC_UNIXTHREAD => check_thread(cmd, cputype),
        LC_FILESET_ENTRY if size < 32 => {
            Err(format!("load command #{i} LC_FILESET_ENTRY size too small"))
        }
        LC_ROUTINES => Err(format!("load command #{i} LC_ROUTINES not support in 64-bit mach-o")),
        LC_ROUTINES_64 if size < 72 => {
            Err(format!("load command #{i} LC_ROUTINES_64 size too small"))
        }
        LC_SEGMENT | LC_SEGMENT_64 => check_segment_command(i, cmd),
        LC_DYLD_EXPORTS_TRIE | LC_DYLD_CHAINED_FIXUPS | LC_FILESET_ENTRY => Ok(()),
        c if c & LC_REQ_DYLD != 0 => {
            Err(format!("load command #{i} unknown required load command 0x{c:08X}"))
        }
        _ => Ok(()),
    }
}

/// An LC_LINKER_OPTION holds a count and that many NUL-terminated
/// strings, an option and its argument, if it takes one.
fn check_linker_option(i: u32, cmd: &Cmd) -> Result<(), String> {
    if cmd.data.len() < 12 {
        return Err(format!("load command #{i} LC_LINKER_OPTION size too small"));
    }
    let count = u32_at(cmd.data, 8);
    if !(1..=2).contains(&count) {
        return Err(format!("LC_LINKER_OPTION has count={count}, only 1 or 2 is valid"));
    }
    let mut rest = &cmd.data[12..];
    for found in 0..count {
        let Some(len) = rest.iter().position(|&b| b == 0) else {
            return Err(format!(
                "load command #{i} has too few strings ({found}) expected ({count})"
            ));
        };
        rest = &rest[len + 1..];
    }
    Ok(())
}

/// The symbol table's locals, then its defined globals, then its
/// undefined ones, from its first symbol on.
fn check_dysymtab(i: u32, cmd: &Cmd) -> Result<(), String> {
    if cmd.data.len() != size_of::<DysymtabCommand>() {
        return Err(format!("load command #{i} LC_DYSYMTAB size wrong"));
    }
    let d = DysymtabCommand::read_from(cmd.data);
    let why = if d.nindirectsyms > 0x1000_0000 {
        "indirect symbol table too large"
    } else if d.ilocalsym != 0 {
        "indirect symbol table ilocalsym != 0"
    } else if d.iextdefsym != d.nlocalsym {
        "indirect symbol table iextdefsym != nlocalsym"
    } else if d.iundefsym != d.iextdefsym.wrapping_add(d.nextdefsym) {
        "indirect symbol table iundefsym != iextdefsym+nextdefsym"
    } else {
        return Ok(());
    };
    Err(format!("malformed mach-o image: {why}"))
}

/// A thread state ld-prime knows: its flavor and length in words. (It
/// says what an arm64 one would be whatever the CPU.)
fn check_thread(cmd: &Cmd, cputype: u32) -> Result<(), String> {
    let (flavor, count) = (u32_at(cmd.data, 8), u32_at(cmd.data, 12));
    let (arch, ok) = match cputype {
        CPU_TYPE_X86_64 => ("x86_64", (flavor, count) == (4, 42)),
        _ => ("arm64", (flavor, count) == (6, 68)),
    };
    match ok {
        true => Ok(()),
        false => Err(format!(
            "invalid {arch} flavor,count ({flavor},{count}), expected 6,68 in LC_UNIXTHREAD"
        )),
    }
}

/// A segment command must be just long enough for its sections, and
/// place its content within 4GB. (ld-prime gives the offset again for
/// a size too large.)
fn check_segment_command(i: u32, cmd: &Cmd) -> Result<(), String> {
    let (header, section) = match cmd.cmd {
        LC_SEGMENT => (56, 68),
        _ => (size_of::<SegmentCommand>(), size_of::<MachSection>()),
    };
    let size = cmd.data.len();
    if size < header {
        return Err(format!("load command #{i} LC_SEGMENT[_64] size too small"));
    }
    let nsects = u32_at(cmd.data, header - 8);
    if size as u64 != header as u64 + nsects as u64 * section as u64 {
        return Err(format!(
            "load command #{i} LC_SEGMENT size ({size}) does not match section count ({nsects})"
        ));
    }
    if cmd.cmd == LC_SEGMENT_64 {
        let seg = SegmentCommand::read_from(cmd.data);
        if seg.fileoff > u32::MAX as u64 {
            return Err(format!(
                "load command #{i} LC_SEGMENT fileoff ({}) too large",
                seg.fileoff
            ));
        }
        if seg.filesize > u32::MAX as u64 {
            return Err(format!(
                "load command #{i} LC_SEGMENT filesize ({}) too large",
                seg.fileoff
            ));
        }
    }
    Ok(())
}

/// The load commands an image may have one of only. ld-prime looks for
/// a second of each once it has walked them all.
fn check_repeats(cmds: &[Cmd]) -> Result<(), String> {
    let mut seen = hashbrown::HashSet::new();
    for cmd in cmds {
        let name = match cmd.cmd {
            LC_SYMTAB => "LC_SYMTAB",
            LC_DYSYMTAB => "LC_DYSYMTAB",
            LC_SEGMENT_SPLIT_INFO => "LC_SEGMENT_SPLIT_INFO",
            LC_ATOM_INFO => "LC_ATOM_INFO",
            LC_FUNCTION_STARTS => "LC_FUNCTION_STARTS",
            LC_DYLD_EXPORTS_TRIE => "LC_DYLD_EXPORTS_TRIE",
            LC_DYLD_CHAINED_FIXUPS => "LC_DYLD_CHAINED_FIXUPS",
            LC_FUNCTION_VARIANTS => "LC_FUNCTION_VARIANTS",
            LC_FUNCTION_VARIANT_FIXUPS => "LC_FUNCTION_VARIANT_FIXUPS",
            LC_TARGET_TRIPLE => "LC_TARGET_TRIPLE",
            LC_ENCRYPTION_INFO_64 => "LC_ENCRYPTION_INFO_64",
            LC_UNIXTHREAD => "LC_MAIN or LC_UNIXTHREAD",
            LC_ROUTINES_64 => "LC_ROUTINES[_64]",
            LC_UUID => "LC_UUID",
            _ => continue,
        };
        if !seen.insert(cmd.cmd) {
            return Err(match cmd.cmd {
                LC_UUID => "too many LC_UUID load commands".to_string(),
                _ => format!("multiple {name} load commands found"),
            });
        }
    }
    Ok(())
}

/// An object is built for one platform, but a zippered one for macOS
/// and Mac Catalyst both.
fn check_platforms(cmds: &[Cmd], cputype: u32) -> Result<(), String> {
    let mut platforms = cmds.iter().filter_map(|cmd| {
        let platform = match cmd.cmd {
            LC_BUILD_VERSION => u32_at(cmd.data, 8),
            LC_VERSION_MIN_MACOSX => PLATFORM_MACOS,
            LC_VERSION_MIN_IPHONEOS | LC_VERSION_MIN_TVOS | LC_VERSION_MIN_WATCHOS => {
                let simulator = cputype == CPU_TYPE_X86_64;
                match (cmd.cmd, simulator) {
                    (LC_VERSION_MIN_IPHONEOS, false) => PLATFORM_IOS,
                    (LC_VERSION_MIN_IPHONEOS, true) => PLATFORM_IOSSIMULATOR,
                    (LC_VERSION_MIN_TVOS, false) => PLATFORM_TVOS,
                    (LC_VERSION_MIN_TVOS, true) => PLATFORM_TVOSSIMULATOR,
                    (_, false) => PLATFORM_WATCHOS,
                    (_, true) => PLATFORM_WATCHOSSIMULATOR,
                }
            }
            _ => return None,
        };
        Some(platform)
    });
    let Some(first) = platforms.next() else { return Ok(()) };
    let zippered = |a, b| [a, b] == [PLATFORM_MACOS, PLATFORM_MACCATALYST];
    for other in platforms {
        if other != first && !zippered(first, other) && !zippered(other, first) {
            return Err(format!(
                "incompatible platforms: {} - {}",
                platform_name(first),
                platform_name(other)
            ));
        }
    }
    Ok(())
}

/// A name of a section or segment as ld-prime prints it, a C string: one
/// of all 16 bytes runs on into what follows it in its record.
fn c_name(bytes: &[u8]) -> String {
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..len]).into_owned()
}

/// A segment's content must be in the file, its permissions ones there
/// are, its range not wrap around, and each section in it be named and
/// lie within its range.
fn check_segments(data: &[u8], cmds: &[Cmd]) -> Result<(), String> {
    for cmd in cmds.iter().filter(|cmd| cmd.cmd == LC_SEGMENT_64) {
        let seg = SegmentCommand::read_from(cmd.data);
        let segname = c_name(&cmd.data[8..]);
        let end = seg.fileoff.checked_add(seg.filesize);
        if end.is_none_or(|end| end > data.len() as u64) {
            return Err(format!(
                "segment '{segname}' content (fileOffset: 0x{:X} -> 0x{:X}) extends beyond end of \
                 file (length: 0x{:X})",
                seg.fileoff,
                seg.fileoff.wrapping_add(seg.filesize),
                data.len()
            ));
        }
        if seg.initprot & !7 != 0 {
            return Err(format!(
                "{segname} segment permissions has invalid bits set (0x{:08X})",
                seg.initprot
            ));
        }
        let seg_end = seg.vmaddr.wrapping_add(seg.vmsize);
        if seg_end < seg.vmaddr {
            return Err(format!("'{segname}' segment vm range wraps"));
        }
        for k in 0..seg.nsects as usize {
            let off = size_of::<SegmentCommand>() + k * size_of::<MachSection>();
            let raw = &cmd.data[off..];
            let sect = MachSection::read_from(raw);
            let (sectname, sect_segname) = (c_name(raw), c_name(&raw[16..]));
            let sect_end = sect.addr.wrapping_add(sect.size);
            let why = if sect.sectname[0] == 0 {
                format!("section in segment '{sect_segname}' has an empty section name")
            } else if sect.segname[0] == 0 {
                format!("section '{sectname}' has an empty segment name")
            } else if (sect.size as i64) < 0 {
                format!("section '{sectname}' size too large 0x{:X}", sect.size)
            } else if sect.addr < seg.vmaddr {
                format!(
                    "section '{sectname}' start address 0x{:X} is before containing segment's \
                     address 0x{:X}",
                    sect.addr, seg.vmaddr
                )
            } else if sect_end > seg_end {
                format!(
                    "section '{sectname}' end address 0x{sect_end:X} is beyond containing \
                     segment's end address 0x{seg_end:X}"
                )
            } else {
                continue;
            };
            return Err(why);
        }
    }
    Ok(())
}

/// The tables at an object's end, after its sections' content - an
/// object has no __LINKEDIT segment -, must lie apart, in the file, and
/// the commands that place them be of their size.
/// (ld-prime names the external relocations local too, and the code
/// signature, which no object has, by no name at all, as it does the
/// end of the sections' content. The optimization hints it doesn't
/// look at.)
fn check_linkedit(data: &[u8], cmds: &[Cmd]) -> Result<(), String> {
    let mut blobs: Vec<(&str, u64, u64)> = Vec::new();
    for (i, cmd) in cmds.iter().enumerate() {
        let word = |off: usize| u32_at(cmd.data, off) as u64;
        if is_linkedit_data(cmd.cmd) && cmd.data.len() != size_of::<LinkEditDataCommand>() {
            return Err(format!("load command #{i} LC_?? size is wrong"));
        }
        match cmd.cmd {
            LC_SYMTAB => {
                // A count of 2^28 symbols makes a table of no size: the
                // size wraps around to 32 bits.
                let size = (word(12) * size_of::<NList>() as u64) as u32 as u64;
                if word(12) != 0 {
                    blobs.push(("symbol table", word(8), size));
                }
                if word(20) != 0 {
                    blobs.push(("symbol strings", word(16), word(20)));
                }
            }
            LC_DYSYMTAB => {
                if word(76) != 0 {
                    blobs.push(("local relocations", word(72), word(76) * 8));
                }
                if word(68) != 0 {
                    blobs.push(("local relocations", word(64), word(68) * 8));
                }
                if word(60) != 0 {
                    blobs.push(("indirect symbol table", word(56), word(60) * 4));
                }
            }
            c if is_linkedit_data(c) && word(12) != 0 => {
                let name = match cmd.cmd {
                    LC_SEGMENT_SPLIT_INFO => "dyld cache info",
                    LC_ATOM_INFO => "mergeable dylib info",
                    LC_FUNCTION_STARTS => "function starts",
                    LC_DATA_IN_CODE => "data in code",
                    LC_DYLD_EXPORTS_TRIE => "exports trie",
                    LC_DYLD_CHAINED_FIXUPS => "chained fixups",
                    LC_FUNCTION_VARIANTS => "function variants",
                    LC_FUNCTION_VARIANT_FIXUPS => "function variant fixups",
                    _ => "unknown",
                };
                blobs.push((name, word(8), word(12)));
            }
            _ => {}
        }
    }
    if blobs.is_empty() {
        return Ok(());
    }
    blobs.sort_by_key(|&(_, off, _)| off);

    let mut start = sections(cmds)
        .filter(|sect| !is_zerofill(sect.flags))
        .map(|sect| sect.offset as u64 + sect.size)
        .max()
        .unwrap_or(0);
    if start == 0 {
        let symtab = cmds.iter().rfind(|cmd| cmd.cmd == LC_SYMTAB);
        start = symtab.map_or(0, |cmd| u32_at(cmd.data, 8) as u64);
    }
    let (mut prev_end, mut prev_name) = (start, "unknown");
    for (name, off, size) in blobs {
        if off < prev_end {
            return Err(format!("LINKEDIT overlap of {prev_name} and {name}"));
        }
        if off + size > data.len() as u64 {
            return Err(format!("LINKEDIT content '{name}' extends beyond end of segment"));
        }
        (prev_end, prev_name) = (off + size, name);
    }
    Ok(())
}

/// What ld-prime doesn't check, which mold reads all the same: a
/// section's content and relocations must be in the file. (ld-prime
/// reads past its end, if the tables at the end of the file don't lie
/// after the content - see check_linkedit -, and crashes.)
fn check_section_contents(data: &[u8], cmds: &[Cmd]) -> Result<(), String> {
    let len = data.len() as u64;
    for sect in sections(cmds) {
        let name = format!("{}/{}", sect.segname(), sect.sectname());
        if !is_zerofill(sect.flags) && sect.offset as u64 + sect.size > len {
            return Err(format!("section '{name}' content extends beyond end of file"));
        }
        if sect.reloff as u64 + sect.nreloc as u64 * 8 > len {
            return Err(format!("section '{name}' relocations extend beyond end of file"));
        }
    }
    Ok(())
}

/// Whether a load command places a table at the end of the file (a
/// linkedit_data_command) ld-prime looks at: not so those of the
/// optimization hints and the code signing requirements.
fn is_linkedit_data(cmd: u32) -> bool {
    matches!(
        cmd,
        LC_SEGMENT_SPLIT_INFO
            | LC_ATOM_INFO
            | LC_FUNCTION_STARTS
            | LC_DATA_IN_CODE
            | LC_CODE_SIGNATURE
            | LC_DYLD_EXPORTS_TRIE
            | LC_DYLD_CHAINED_FIXUPS
            | LC_FUNCTION_VARIANTS
            | LC_FUNCTION_VARIANT_FIXUPS
    )
}

/// The section headers of an object's segment commands.
fn sections<'a>(cmds: &'a [Cmd]) -> impl Iterator<Item = MachSection> + 'a {
    cmds.iter().filter(|cmd| cmd.cmd == LC_SEGMENT_64).flat_map(|cmd| {
        let nsects = u32_at(cmd.data, 64) as usize;
        (0..nsects).map(move |k| {
            let off = size_of::<SegmentCommand>() + k * size_of::<MachSection>();
            MachSection::read_from(&cmd.data[off..])
        })
    })
}

fn is_zerofill(flags: u32) -> bool {
    matches!(flags & SECTION_TYPE, S_ZEROFILL | S_THREAD_LOCAL_ZEROFILL)
}
