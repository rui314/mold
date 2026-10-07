use std::collections::BTreeMap;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::ops::Range;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use memmap2::Mmap;
use zerocopy::byteorder::little_endian::{U32, U64};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, TryFromBytes, Unaligned};

use crate::arch::Target;
use crate::cmdline::{Args, BuildId, DebugCompression, ShuffleSections};
use crate::context::Context;
use crate::elf::*;
use crate::mapped_file;
use crate::micro_link::{
    ArchiveImage, BuildIdImage, ObjectImage, ObjectTopology, ResolvedValue, SectionImage,
    SectionRevision, SectionTopology,
};

const ENTRY_LIMIT: usize = 64 * 1024 * 1024;
const CACHE_LIMIT: u64 = 512 * 1024 * 1024;
pub(crate) const SHARD: usize = 4 * 1024 * 1024;
const MAGIC: [u8; 8] = *b"MOLDINC\0";
const VERSION: u32 = 4;
fn state_page() -> usize {
    static SIZE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| match std::env::var("MOLD_INCREMENTAL_STATE_PAGE").ok().as_deref() {
        Some("16384") => 16 * 1024,
        Some("65536") => 64 * 1024,
        _ => 4 * 1024,
    })
}
const ROOT_BYTES: usize = 4096;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned,
)]
#[repr(C)]
pub(crate) struct Identity {
    dev: U64,
    ino: U64,
    size: U64,
    mtime: U64,
    mtime_nsec: U64,
    ctime: U64,
    ctime_nsec: U64,
    mode: U32,
    present: U32,
}

impl Identity {
    pub(crate) fn from_metadata(m: &Metadata) -> Option<Self> {
        #[cfg(unix)]
        return Some(Self {
            dev: m.dev().into(),
            ino: m.ino().into(),
            size: m.len().into(),
            mtime: (m.mtime() as u64).into(),
            mtime_nsec: (m.mtime_nsec() as u64).into(),
            ctime: (m.ctime() as u64).into(),
            ctime_nsec: (m.ctime_nsec() as u64).into(),
            mode: m.mode().into(),
            present: 1.into(),
        });
        #[cfg(not(unix))]
        {
            let _ = m;
            None
        }
    }

    pub(crate) fn path(path: &Path) -> Option<Self> {
        match std::fs::metadata(path) {
            Ok(m) => Self::from_metadata(&m),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Self::new_zeroed()),
            Err(_) => None,
        }
    }
}

#[derive(Clone, Copy, TryFromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(u8)]
enum Capability {
    NullOnly = 0,
    Delta = 1,
}

#[derive(Clone, Copy, TryFromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct Header {
    magic: [u8; 8],
    version: U32,
    target: U32,
    length: U64,
    capability: Capability,
    reserved: [u8; 7],
    command: [u8; 32],
    output: Identity,
    directory_offset: U64,
    directory_count: U32,
    reserved2: U32,
    checksum: [u8; 32],
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct InputRecord {
    identity: Identity,
    path_offset: U32,
    path_length: U32,
    directory: U32,
    reserved: U32,
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct TableRecord {
    kind: U32,
    version: U32,
    offset: U64,
    length: U64,
    record_size: U32,
    count: U32,
    checksum: [u8; 32],
}

const INPUTS: u32 = 1;
const STRINGS: u32 = 2;
const LEAVES: u32 = 3;
const OBJECTS: u32 = 4;
const SECTIONS: u32 = 5;
const RESOLVED: u32 = 6;
const BUILD_ID: u32 = 7;
const ARCHIVES: u32 = 8;
const SUBTREES: u32 = 9;
const BOOT: u32 = 10;
const TELEMETRY: u32 = 11;
const INPUT_OBJECTS: u32 = 12;
const MICRO_CONFIG: u32 = 13;
const OBJECT_REVISIONS: u32 = 14;
const SECTION_REVISIONS: u32 = 15;
const MERGE_FRAGMENTS: u32 = 16;
const MERGE_REFS: u32 = 17;
const MERGE_OBJECTS: u32 = 18;
const MERGE_USES: u32 = 19;
const CELLS: u32 = 20;
const CELL_CONSUMERS: u32 = 21;
const SECTION_CELLS: u32 = 22;
const CELL_EDGES: u32 = 23;

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct MicroConfig {
    version: U32,
    target: U32,
    flags: U32,
    build_id_size: U32,
    output_offset: U32,
    output_length: U32,
    compatibility: [u8; 32],
    reserved: [u8; 8],
}
const _: () = assert!(size_of::<MicroConfig>() == 64);

fn cache_directory() -> Option<PathBuf> {
    Some(
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))?
            .join("mold/incremental-v1"),
    )
}

fn compatibility() -> Option<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    h.update(std::env::current_dir().ok()?.as_os_str().as_encoded_bytes());
    h.update(&(crate::build_id_tree::granularity() as u64).to_le_bytes());
    h.update(&(state_page() as u64).to_le_bytes());
    h.update(crate::cmdline::VERSION.as_bytes());
    h.update(Identity::path(&std::env::current_exe().ok()?)?.as_bytes());
    #[cfg(not(windows))]
    h.update(&crate::output_file::umask().to_le_bytes());
    Some(*h.finalize().as_bytes())
}

pub(crate) fn fast_invocation() -> Option<i32> {
    let started = std::time::Instant::now();
    let contract = crate::change_set::ChangeSet::read()?;
    let manifest_ns = started.elapsed().as_nanos();
    if contract.header.flags.get() & 4 == 0
        || !cfg!(target_os = "linux")
        || std::env::var_os("MOLD_DEBUG").is_some_and(|v| !v.is_empty())
    {
        return None;
    }
    let cache = cache_directory()?;
    let path = cache.join(format!("{}.state", blake3::Hash::from(contract.state_key?).to_hex()));
    let lock = key_lock(&cache, &path)?;
    let file = File::open(&path).ok()?;
    if file.metadata().ok()?.len() > ENTRY_LIMIT as u64 {
        return None;
    }
    let state = mapped_file::map_state(&file).ok()?;
    let view = View::load(&state)?;
    if contract.header.generation.get() != view.generation
        || contract.header.command != view.header.command
        || contract.header.output != *blake3::hash(view.header.output.as_bytes()).as_bytes()
        || view.records::<[u8; 32]>(BOOT)?.first().copied() != Some(boot()?)
    {
        return None;
    }
    if view.descriptor(MICRO_CONFIG)?.count.get() != 1 {
        return None;
    }
    let root_ns = started.elapsed().as_nanos().saturating_sub(manifest_ns);
    let config = *view.range::<MicroConfig>(MICRO_CONFIG, 0, 1)?.first()?;
    if config.version.get() != 1
        || config.target.get() != EM_X86_64
        || config.flags.get() & !63 != 0
        || config.reserved != [0; 8]
        || config.compatibility != compatibility()?
        || config.build_id_size.get() > 32
    {
        return None;
    }
    let output = view.bytes(
        STRINGS,
        config.output_offset.get() as usize,
        config.output_length.get() as usize,
    )?;
    if output.first() != Some(&b'/') || output.contains(&0) {
        return None;
    }
    let output = PathBuf::from(crate::util::os_str(&output));
    let mut key = blake3::Hasher::new();
    key.update(output.as_os_str().as_encoded_bytes());
    key.update(b"x86_64");
    if key.finalize().as_bytes() != contract.state_key.as_ref()?
        || !std::fs::symlink_metadata(&output).ok()?.is_file()
        || Identity::path(&output) != Some(view.header.output)
    {
        return None;
    }
    let flags = config.flags.get();
    let args = Args {
        output: output.clone(),
        incremental: true,
        fork: false,
        perf: flags & 1 != 0,
        incremental_verify: flags & 2 != 0,
        relax: flags & 4 != 0,
        icf: flags & 8 != 0,
        gc_sections: flags & 16 != 0,
        overwrite_output_file: flags & 32 != 0,
        build_id: if config.build_id_size.get() == 0 {
            BuildId::None
        } else {
            BuildId::Hash(config.build_id_size.get() as usize)
        },
        ..Args::default()
    };
    let cmdline: crate::driver::Cmdline = if args.incremental_verify {
        crate::cmdline::expand_response_files(std::env::args_os().collect()).into()
    } else {
        Vec::new().into()
    };
    let command = view.header.command;
    drop(view);
    let ctx = Context::<crate::arch::X86_64>::new(args, cmdline);
    TRACK_ICF.store(ctx.args.icf, Ordering::Relaxed);
    let mut session = Session {
        _lock: lock,
        cache,
        path,
        output,
        command,
        started,
        state: Some(state),
        dependencies: Vec::new(),
        changed: None,
        content_digests: BTreeMap::new(),
        change_contract: false,
        trusted_digests: false,
        reason: "fast invocation proof",
    };
    if ctx.args.perf {
        eprintln!(
            "incremental: fast_setup manifest_ns={manifest_ns} state_root_ns={root_ns} config_ns={}",
            started.elapsed().as_nanos().saturating_sub(manifest_ns + root_ns)
        );
        eprintln!(
            "incremental: Fast Invocation attempt, command expansion/parsing/hash bypass={}",
            !ctx.args.incremental_verify
        );
    }
    let success = session.null_hit(&ctx) || session.micro_link(&ctx);
    if !success {
        return None;
    }
    if ctx.args.perf {
        eprintln!(
            "incremental: Fast Invocation completed, command expansion/parsing/hash skipped={}",
            !ctx.args.incremental_verify
        );
    }
    if ctx.args.perf
        && session.changed.as_ref().is_some_and(|d| d.is_empty())
        && !ctx.args.incremental_verify
    {
        eprintln!("incremental: null link, rewritten=0, parsing/resolution/copy skipped");
        ctx.timers.print();
    }
    crate::mapped_file::drop_mappings();
    crate::error::checkpoint();
    Some(0)
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct PageHeader {
    kind: U32,
    index: U32,
    generation: U64,
    records: U32,
    reserved: U32,
    checksum: [u8; 32],
    padding: [u8; 8],
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct InputObjects {
    begin: U32,
    count: U32,
}

const _: () = {
    assert!(size_of::<Identity>() == 64);
    assert!(size_of::<Header>() == 176);
    assert!(size_of::<InputRecord>() == 80);
    assert!(size_of::<TableRecord>() == 64);
};

struct TablePages {
    data: usize,
    levels: [usize; 4],
    depth: usize,
    index: usize,
}
impl TablePages {
    fn new(count: usize, size: usize) -> Option<Self> {
        let capacity = (state_page() - 64).checked_div(size)?;
        if capacity == 0 {
            return None;
        }
        let data = count.div_ceil(capacity);
        if data > ENTRY_LIMIT / state_page() {
            return None;
        }
        let mut pages = Self { data, levels: [0; 4], depth: 0, index: 0 };
        let mut children = data;
        while children != 0 {
            let nodes = children.div_ceil((state_page() - 64) / 32);
            *pages.levels.get_mut(pages.depth)? = nodes;
            pages.depth += 1;
            pages.index = pages.index.checked_add(nodes)?;
            if nodes == 1 {
                break;
            }
            children = nodes;
        }
        Some(pages)
    }
    fn flat(&self, level: usize, index: usize) -> Option<usize> {
        if level >= self.depth || index >= self.levels[level] {
            return None;
        }
        self.levels[..level].iter().try_fold(index, |sum, n| sum.checked_add(*n))
    }
    fn total(&self) -> Option<usize> {
        self.data.checked_add(self.index)
    }
}

struct View<'a> {
    header: &'a Header,
    directory: &'a [TableRecord],
    data: &'a [u8],
    generation: u64,
    root: usize,
    inputs: Vec<InputRecord>,
    strings: Vec<u8>,
    validated: std::cell::RefCell<std::collections::BTreeSet<(u32, usize)>>,
}

impl<'a> View<'a> {
    fn root(data: &'a [u8], root: usize) -> Option<Self> {
        let bytes = data.get(root..root.checked_add(ROOT_BYTES)?)?;
        let (header, _) = Header::try_ref_from_prefix(bytes).ok()?;
        if header.magic != MAGIC
            || header.version.get() != VERSION
            || header.target.get() != EM_X86_64
            || header.length.get() != data.len() as u64
            || header.reserved != [0; 7]
            || header.reserved2.get() as usize != state_page()
            || header.output.present.get() != 1
            || header.output.mode.get() & libc::S_IFMT != libc::S_IFREG
            || header.directory_offset.get() != (size_of::<Header>() + 8) as u64
            || header.directory_count.get() > 32
        {
            return None;
        }
        let generation = U64::ref_from_bytes(bytes.get(176..184)?).ok()?.get();
        if generation == 0 {
            return None;
        }
        let end = 184usize.checked_add((header.directory_count.get() as usize).checked_mul(64)?)?;
        let directory = <[TableRecord]>::ref_from_bytes(bytes.get(184..end)?).ok()?;
        let mut hash = blake3::Hasher::new();
        hash.update(&bytes[..144]);
        hash.update(&[0; 32]);
        hash.update(&bytes[176..end]);
        if hash.finalize().as_bytes() != &header.checksum {
            return None;
        }
        let mut cursor = 2 * ROOT_BYTES;
        let mut kind = 0;
        for t in directory {
            let size = t.record_size.get() as usize;
            let count = t.count.get() as usize;
            let pages = TablePages::new(count, size)?;
            if t.kind.get() <= kind
                || t.version.get() != 1
                || t.offset.get() != cursor as u64
                || (count as u64).checked_mul(size as u64)? != t.length.get()
            {
                return None;
            }
            cursor = cursor.checked_add(pages.total()?.checked_mul(state_page())?)?;
            if cursor > data.len() {
                return None;
            }
            kind = t.kind.get();
        }
        if cursor != data.len() {
            return None;
        }
        Some(Self {
            header,
            directory,
            data,
            generation,
            root,
            inputs: Vec::new(),
            strings: Vec::new(),
            validated: std::cell::RefCell::new(std::collections::BTreeSet::new()),
        })
    }

    fn load(data: &'a [u8]) -> Option<Self> {
        if data.len() > ENTRY_LIMIT {
            return None;
        }
        match (Self::root(data, 0), Self::root(data, ROOT_BYTES)) {
            (Some(a), Some(b)) => Some(if a.generation >= b.generation { a } else { b }),
            (Some(a), None) | (None, Some(a)) => Some(a),
            _ => None,
        }
    }

    fn parse(data: &'a [u8]) -> Option<Self> {
        let mut view = Self::load(data)?;
        view.inputs = view.records(INPUTS)?;
        view.strings = view.records(STRINGS)?;
        if view.inputs.is_empty() {
            return None;
        }
        let mut previous: Option<&[u8]> = None;
        for r in &view.inputs {
            let path = view.path_bytes(r)?;
            if path.is_empty()
                || path[0] != b'/'
                || path.contains(&0)
                || previous.is_some_and(|p| p >= path)
                || r.directory.get() > 1
                || r.reserved.get() != 0
                || r.identity.present.get() > 1
                || (r.directory.get() == 0
                    && (r.identity.present.get() != 1
                        || r.identity.mode.get() & libc::S_IFMT != libc::S_IFREG))
            {
                return None;
            }
            previous = Some(path);
        }
        Some(view)
    }

    fn record<T: FromBytes + KnownLayout + Immutable + Unaligned>(
        &self,
        kind: u32,
        index: usize,
    ) -> Option<&'a T> {
        let capacity = (state_page() - 64).checked_div(size_of::<T>())?;
        self.page::<T>(kind, index / capacity)?.get(index % capacity)
    }

    fn placement(&self, image: &ObjectImage, shndx: u32) -> Option<&'a SectionTopology> {
        let mut begin = image.sections_begin.get() as usize;
        let mut end = begin.checked_add(image.sections_count.get() as usize)?;
        if end > self.descriptor(SECTIONS)?.count.get() as usize {
            return None;
        }
        while begin < end {
            let row = begin + (end - begin) / 2;
            let value = self.record::<SectionTopology>(SECTIONS, row)?;
            match value.shndx.get().cmp(&shndx) {
                std::cmp::Ordering::Less => begin = row + 1,
                std::cmp::Ordering::Greater => end = row,
                std::cmp::Ordering::Equal => return Some(value),
            }
        }
        None
    }

    fn resolved(&self, image: &ObjectImage, index: u32, kind: u32) -> Option<ResolvedValue> {
        let start = image.resolved_begin.get() as usize;
        let mut begin = start;
        let mut end = start.checked_add(image.resolved_count.get() as usize)?;
        if end > self.descriptor(RESOLVED)?.count.get() as usize {
            return None;
        }
        while begin < end {
            let row = begin + (end - begin) / 2;
            let value = *self.record::<ResolvedValue>(RESOLVED, row)?;
            if value.reserved.get() > 5 {
                return None;
            }
            match (value.index.get(), value.reserved.get()).cmp(&(index, kind)) {
                std::cmp::Ordering::Less => begin = row + 1,
                std::cmp::Ordering::Greater => end = row,
                std::cmp::Ordering::Equal => return Some(value),
            }
        }
        None
    }

    fn cell(&self, key: &[u8; 32], kind: u32) -> Option<(usize, crate::semantic_cells::Cell)> {
        let mut begin = 0;
        let mut end = self.descriptor(CELLS)?.count.get() as usize;
        while begin < end {
            let index = begin + (end - begin) / 2;
            let cell = *self.record::<crate::semantic_cells::Cell>(CELLS, index)?;
            if cell.properties.get() > 1
                || (cell.kind.get() != 0 && cell.properties.get() != 0)
                || cell.kind.get() > 5
                || cell.capacity.get() == 0
                || cell.begin.get().checked_add(cell.capacity.get())?
                    > self.descriptor(CELL_CONSUMERS)?.count.get()
            {
                return None;
            }
            match (cell.key, cell.kind.get()).cmp(&(*key, kind)) {
                std::cmp::Ordering::Less => begin = index + 1,
                std::cmp::Ordering::Greater => end = index,
                std::cmp::Ordering::Equal => return Some((index, cell)),
            }
        }
        None
    }

    fn merge_fragment(&self, key: &[u8; 32]) -> Option<(usize, crate::merge_image::Fragment)> {
        let mut begin = 0;
        let mut end = self.descriptor(MERGE_FRAGMENTS)?.count.get() as usize;
        if end > 8192 || self.descriptor(MERGE_REFS)?.count.get() as usize != end {
            return None;
        }
        while begin < end {
            let index = begin + (end - begin) / 2;
            let fragment =
                *self.range::<crate::merge_image::Fragment>(MERGE_FRAGMENTS, index, 1)?.first()?;
            if fragment.reserved.get() != 0
                || fragment.length.get() == 0
                || fragment.offset.get().checked_add(u64::from(fragment.length.get()))?
                    > self.header.output.size.get()
            {
                return None;
            }
            match fragment.key.cmp(key) {
                std::cmp::Ordering::Less => begin = index + 1,
                std::cmp::Ordering::Greater => end = index,
                std::cmp::Ordering::Equal => {
                    if self.range::<U32>(MERGE_REFS, index, 1)?.first()?.get() == 0 {
                        return None;
                    }
                    return Some((index, fragment));
                }
            }
        }
        None
    }

    fn descriptor(&self, kind: u32) -> Option<&TableRecord> {
        self.directory.iter().find(|t| t.kind.get() == kind)
    }

    fn checked_page(
        &self,
        offset: usize,
        kind: u32,
        index: usize,
        level: u32,
        count: usize,
        expected: &[u8; 32],
    ) -> Option<&'a [u8]> {
        let bytes = self.data.get(offset..offset.checked_add(state_page())?)?;
        let (h, _) = PageHeader::ref_from_prefix(bytes).ok()?;
        if h.kind.get() != kind
            || h.index.get() as usize != index
            || h.reserved.get() != level
            || h.generation.get() == 0
            || h.generation.get() > self.generation
            || h.records.get() as usize != count
            || h.padding != [0; 8]
            || &h.checksum != expected
        {
            return None;
        }
        if !self.validated.borrow().contains(&(kind, index)) {
            let mut hash = blake3::Hasher::new();
            hash.update(&bytes[..24]);
            hash.update(&[0; 32]);
            hash.update(&bytes[56..]);
            if hash.finalize().as_bytes() != expected {
                return None;
            }
            self.validated.borrow_mut().insert((kind, index));
        }
        bytes.get(64..)
    }

    fn node(&self, kind: u32, level: usize, index: usize) -> Option<&'a [[u8; 32]]> {
        let t = self.descriptor(kind)?;
        let pages = TablePages::new(t.count.get() as usize, t.record_size.get() as usize)?;
        let capacity = (state_page() - 64) / 32;
        if level >= pages.depth || index >= pages.levels[level] {
            return None;
        }
        let mut indices = [0; 4];
        indices[level] = index;
        for l in level + 1..pages.depth {
            indices[l] = indices[l - 1] / capacity;
        }
        let mut expected = t.checksum;
        for l in (level..pages.depth).rev() {
            let flat = pages.flat(l, indices[l])?;
            let offset = usize::try_from(t.offset.get())
                .ok()?
                .checked_add(flat.checked_mul(state_page())?)?;
            let children = if l == 0 { pages.data } else { pages.levels[l - 1] };
            let count = children.checked_sub(indices[l].checked_mul(capacity)?)?.min(capacity);
            let bytes = self.checked_page(
                offset,
                kind,
                flat | 0x80000000,
                (l + 1) as u32,
                count,
                &expected,
            )?;
            let values = <[[u8; 32]]>::ref_from_bytes(bytes.get(..count.checked_mul(32)?)?).ok()?;
            if l == level {
                return Some(values);
            }
            expected = *values.get(indices[l - 1] % capacity)?;
        }
        None
    }

    fn page_data(&self, kind: u32, index: usize) -> Option<&'a [u8]> {
        let t = self.descriptor(kind)?;
        let size = t.record_size.get() as usize;
        let pages = TablePages::new(t.count.get() as usize, size)?;
        if index >= pages.data {
            return None;
        }
        let capacity = (state_page() - 64) / size;
        let count =
            (t.count.get() as usize).checked_sub(index.checked_mul(capacity)?)?.min(capacity);
        let offset = usize::try_from(t.offset.get())
            .ok()?
            .checked_add(pages.index.checked_add(index)?.checked_mul(state_page())?)?;
        let expected = self
            .node(kind, 0, index / ((state_page() - 64) / 32))?
            .get(index % ((state_page() - 64) / 32))?;
        self.checked_page(offset, kind, index, 0, count, expected)?.get(..count.checked_mul(size)?)
    }

    fn page<T: FromBytes + KnownLayout + Immutable + Unaligned>(
        &self,
        kind: u32,
        index: usize,
    ) -> Option<&'a [T]> {
        if self.descriptor(kind)?.record_size.get() as usize != size_of::<T>() {
            return None;
        }
        <[T]>::ref_from_bytes(self.page_data(kind, index)?).ok()
    }

    fn bytes(&self, kind: u32, start: usize, count: usize) -> Option<std::borrow::Cow<'a, [u8]>> {
        let t = self.descriptor(kind)?;
        let end = start.checked_add(count)?;
        if t.record_size.get() != 1 || end > t.count.get() as usize {
            return None;
        }
        let capacity = state_page() - 64;
        if count == 0 {
            return Some(std::borrow::Cow::Borrowed(&[]));
        }
        if start / capacity == (end - 1) / capacity {
            return Some(std::borrow::Cow::Borrowed(
                self.page_data(kind, start / capacity)?
                    .get(start % capacity..start % capacity + count)?,
            ));
        }
        Some(std::borrow::Cow::Owned(self.range(kind, start, count)?))
    }

    fn range<T: FromBytes + KnownLayout + Immutable + Unaligned + Copy>(
        &self,
        kind: u32,
        start: usize,
        count: usize,
    ) -> Option<Vec<T>> {
        let t = self.descriptor(kind)?;
        let end = start.checked_add(count)?;
        if end > t.count.get() as usize || t.record_size.get() as usize != size_of::<T>() {
            return None;
        }
        let capacity = (state_page() - 64) / size_of::<T>();
        let mut values = Vec::with_capacity(count);
        let mut cursor = start;
        while cursor < end {
            let page = self.page::<T>(kind, cursor / capacity)?;
            let from = cursor % capacity;
            let n = (end - cursor).min(page.len().checked_sub(from)?);
            values.extend_from_slice(page.get(from..from.checked_add(n)?)?);
            cursor += n;
        }
        Some(values)
    }

    fn records<T: FromBytes + KnownLayout + Immutable + Unaligned + Copy>(
        &self,
        kind: u32,
    ) -> Option<Vec<T>> {
        self.range(kind, 0, self.descriptor(kind)?.count.get() as usize)
    }

    fn path_bytes(&self, r: &InputRecord) -> Option<&[u8]> {
        let start = r.path_offset.get() as usize;
        self.strings.get(start..start.checked_add(r.path_length.get() as usize)?)
    }
    fn path(&self, r: &InputRecord) -> &Path {
        Path::new(crate::util::os_str(self.path_bytes(r).unwrap()))
    }
}

fn index_bytes(
    kind: u32,
    index: usize,
    level: usize,
    generation: u64,
    values: &[[u8; 32]],
) -> Option<Vec<u8>> {
    let mut bytes = page_bytes(kind, index | 0x80000000, generation, 32, values.as_bytes())?;
    bytes[20..24].copy_from_slice(U32::new((level + 1) as u32).as_bytes());
    bytes[24..56].fill(0);
    let hash = *blake3::hash(&bytes).as_bytes();
    bytes[24..56].copy_from_slice(&hash);
    Some(bytes)
}

fn page_bytes(
    kind: u32,
    index: usize,
    generation: u64,
    size: usize,
    bytes: &[u8],
) -> Option<Vec<u8>> {
    if bytes.len() > state_page() - 64 || !bytes.len().is_multiple_of(size) {
        return None;
    }
    let header = PageHeader {
        kind: kind.into(),
        index: u32::try_from(index).ok()?.into(),
        generation: generation.into(),
        records: u32::try_from(bytes.len() / size).ok()?.into(),
        reserved: 0.into(),
        checksum: [0; 32],
        padding: [0; 8],
    };
    let mut page = vec![0; state_page()];
    page[..64].copy_from_slice(header.as_bytes());
    page[64..64 + bytes.len()].copy_from_slice(bytes);
    let hash = *blake3::hash(&page).as_bytes();
    page[24..56].copy_from_slice(&hash);
    Some(page)
}

static TRACK_ICF: AtomicBool = AtomicBool::new(false);

pub(crate) fn icf_tracking() -> bool {
    TRACK_ICF.load(Ordering::Relaxed)
}

static TRACK_SEMANTIC: AtomicBool = AtomicBool::new(false);
static TRACK_SEARCH: AtomicBool = AtomicBool::new(false);

static SEARCH_PATHS: Mutex<BTreeMap<PathBuf, Option<Identity>>> = Mutex::new(BTreeMap::new());

pub(crate) fn record_search_path(path: &Path) {
    if !TRACK_SEARCH.load(Ordering::Relaxed) {
        return;
    }
    let Ok(path) = std::path::absolute(path) else {
        return;
    };
    SEARCH_PATHS.lock().unwrap().entry(path.clone()).or_insert_with(|| Identity::path(&path));
}

fn eligible<E: Target>(ctx: &Context<E>) -> bool {
    let a = &ctx.args;
    cfg!(target_os = "linux")
        && E::NAME == "x86_64"
        && a.incremental
        && std::env::var_os("MOLD_DEBUG").is_none_or(|v| v.is_empty())
        && !a.default_symver
        && a.chroot.as_os_str().is_empty()
        && a.orig_cwd.is_none()
        && !a.relocatable
        && !a.oformat_binary
        && a.plugin.as_os_str().is_empty()
        && !a.lto_pass2
        && a.map.is_none()
        && a.dependency_file.as_os_str().is_empty()
        && !a.repro
        && !a.trace
        && a.trace_symbol.is_empty()
        && !a.print_dependencies
        && !a.stats
        && a.print_gc_sections.is_none()
        && a.print_icf_sections.is_none()
        && a.separate_debug_file.as_os_str().is_empty()
        && !a.warn_common
        && !a.warn_textrel
        && a.z_cet_report == crate::cmdline::CetReportKind::None
        && a.unresolved_symbols != crate::cmdline::UnresolvedKind::Warn
        && a.shuffle_sections == ShuffleSections::None
        && !matches!(a.build_id, BuildId::Uuid)
        && a.output != Path::new("-")
}

pub(crate) fn delta_eligible(args: &Args) -> bool {
    args.mmap_output_file
        && args.overwrite_output_file
        && args.filler.is_none()
        && !args.gdb_index
        && !args.emit_relocs
        && !args.z_rewrite_endbr
        && args.compress_debug_sections == DebugCompression::None
}

fn command<E: Target>(ctx: &Context<E>) -> Option<[u8; 32]> {
    let mut h = blake3::Hasher::new();
    for arg in ctx.cmdline_args.iter() {
        let bytes = arg.as_encoded_bytes();
        h.update(&(bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    let cwd = std::env::current_dir().ok()?;
    h.update(cwd.as_os_str().as_encoded_bytes());
    h.update(E::NAME.as_bytes());
    h.update(&(crate::build_id_tree::granularity() as u64).to_le_bytes());
    h.update(&(state_page() as u64).to_le_bytes());
    h.update(crate::cmdline::VERSION.as_bytes());
    let executable = Identity::path(&std::env::current_exe().ok()?)?;
    h.update(executable.as_bytes());
    #[cfg(not(windows))]
    h.update(&crate::output_file::umask().to_le_bytes());
    Some(*h.finalize().as_bytes())
}

struct Dependency {
    path: PathBuf,
    identity: Identity,
    directory: bool,
}

pub(crate) struct Session {
    _lock: File,
    cache: PathBuf,
    path: PathBuf,
    output: PathBuf,
    command: [u8; 32],
    started: std::time::Instant,
    state: Option<Mmap>,
    dependencies: Vec<Dependency>,
    changed: Option<Vec<(usize, InputRecord, PathBuf)>>,
    content_digests: BTreeMap<PathBuf, [u8; 32]>,
    change_contract: bool,
    trusted_digests: bool,

    pub(crate) reason: &'static str,
}

impl Session {
    pub(crate) fn begin<E: Target>(ctx: &Context<E>) -> Option<Self> {
        TRACK_SEARCH.store(false, Ordering::Relaxed);
        TRACK_SEMANTIC.store(false, Ordering::Relaxed);
        TRACK_ICF.store(ctx.args.icf, Ordering::Relaxed);
        if !eligible(ctx) {
            return None;
        }
        let _t = ctx.timer("state_load");
        let cache = cache_directory()?;
        std::fs::create_dir_all(&cache).ok()?;
        let output =
            std::path::absolute(mapped_file::apply_chroot(&ctx.args.chroot, &ctx.args.output))
                .ok()?;
        let output = std::fs::canonicalize(output.parent()?).ok()?.join(output.file_name()?);
        if std::fs::symlink_metadata(&output).is_ok_and(|m| !m.is_file()) {
            return None;
        }
        let mut key = blake3::Hasher::new();
        key.update(output.as_os_str().as_encoded_bytes());
        key.update(E::NAME.as_bytes());
        let path = cache.join(format!("{}.state", key.finalize().to_hex()));
        let lock = key_lock(&cache, &path)?;
        let state = File::open(&path).ok().and_then(|f| {
            let len = f.metadata().ok()?.len();
            if len > ENTRY_LIMIT as u64 || len < size_of::<Header>() as u64 {
                return None;
            }
            mapped_file::map_state(&f).ok()
        });
        let mut session = Self {
            _lock: lock,
            cache,
            path,
            output,
            command: command(ctx)?,
            started: std::time::Instant::now(),
            state,
            dependencies: Vec::new(),
            changed: None,
            content_digests: BTreeMap::new(),
            change_contract: false,
            trusted_digests: false,

            reason: "missing or invalid state",
        };
        if let Some(state) = &session.state {
            if let Some(view) = View::load(state) {
                if view.header.command == session.command {
                    let current_boot = boot()?;
                    let stored_boot =
                        view.records::<[u8; 32]>(BOOT).and_then(|r| r.first().copied());
                    session.reason = "boot recertification";
                    if stored_boot != Some(current_boot) {
                        let _t = ctx.timer("boot_recertify");
                        let strong = crate::change_set::ChangeSet::read().is_some_and(|c| {
                            c.header.flags.get() & 2 != 0
                                && c.changed.is_empty()
                                && c.header.generation.get() == view.generation
                                && c.header.command == session.command
                                && c.header.output
                                    == *blake3::hash(view.header.output.as_bytes()).as_bytes()
                        });
                        if stored_boot.is_some()
                            && strong
                            && View::parse(state).is_some_and(|v| recertify(&v, &session.output))
                        {
                            drop(view);
                            session.commit_records(
                                vec![(BOOT, 0, current_boot.to_vec())],
                                Identity::path(&session.output)?,
                            )?;
                            session.state = File::open(&session.path)
                                .ok()
                                .and_then(|f| mapped_file::map_state(&f).ok());
                            if ctx.args.perf {
                                eprintln!("incremental: output recertified after boot change");
                            }
                        } else {
                            session.reason = "boot proof requires strong ChangeSet";
                            session.state = None;
                        }
                    } else {
                        session.reason = "output identity";
                        if Identity::path(&session.output) == Some(view.header.output) {
                            session.reason = "input state";
                        } else {
                            session.state = None;
                        }
                    }
                } else {
                    session.reason = "command signature";
                    session.state = None;
                }
            } else {
                session.state = None;
            }
        }
        TRACK_SEARCH.store(true, Ordering::Relaxed);
        TRACK_SEMANTIC.store(delta_eligible(&ctx.args), Ordering::Relaxed);
        Some(session)
    }

    pub(crate) fn null_hit<E: Target>(&mut self, ctx: &Context<E>) -> bool {
        let _t = ctx.timer("state_validate");
        if let Some(changes) = self.contract_diff(ctx) {
            let hit = changes.is_empty() && !ctx.args.incremental_verify;
            self.changed = Some(changes);
            self.change_contract = true;
            return hit;
        }
        let Some(view) = self.state.as_deref().and_then(View::load) else {
            return false;
        };
        let validate = || -> Option<Vec<(usize, InputRecord, PathBuf)>> {
            let count = view.descriptor(INPUTS)?.count.get() as usize;
            if count == 0 {
                return None;
            }
            let capacity = (state_page() - 64) / size_of::<InputRecord>();
            let mut changed = Vec::new();
            let mut previous: Option<std::borrow::Cow<'_, [u8]>> = None;
            for page in 0..count.div_ceil(capacity) {
                for (index, r) in view.page::<InputRecord>(INPUTS, page)?.iter().enumerate() {
                    let path = view.bytes(
                        STRINGS,
                        r.path_offset.get() as usize,
                        r.path_length.get() as usize,
                    )?;
                    if path.first() != Some(&b'/')
                        || path.contains(&0)
                        || previous.as_ref().is_some_and(|p| p.as_ref() >= path.as_ref())
                        || r.directory.get() > 1
                        || r.reserved.get() != 0
                        || r.identity.present.get() > 1
                        || (r.directory.get() == 0
                            && (r.identity.present.get() != 1
                                || r.identity.mode.get() & libc::S_IFMT != libc::S_IFREG))
                    {
                        return None;
                    }
                    let name = Path::new(crate::util::os_str(&path));
                    let identity = Identity::path(name)?;
                    if identity != r.identity {
                        let mut next = *r;
                        next.identity = identity;
                        changed.push((page * capacity + index, next, name.to_path_buf()));
                    }
                    previous = Some(path);
                }
            }
            if ctx.args.perf {
                eprintln!("incremental: standalone dependency_operations={count}");
            }
            Some(changed)
        };
        let Some(changed) = validate() else {
            return false;
        };
        let hit = changed.is_empty() && !ctx.args.incremental_verify;
        self.changed = Some(changed);
        hit
    }

    fn contract_diff<E: Target>(
        &mut self,
        ctx: &Context<E>,
    ) -> Option<Vec<(usize, InputRecord, PathBuf)>> {
        let contract = crate::change_set::ChangeSet::read()?;
        let view = View::load(self.state.as_deref()?)?;
        if contract.header.generation.get() != view.generation
            || contract.header.command != self.command
            || contract.header.output != *blake3::hash(view.header.output.as_bytes()).as_bytes()
        {
            return None;
        }
        let mut changed = Vec::with_capacity(contract.changed.len());
        let mut digests = BTreeMap::new();
        for c in &contract.changed {
            if c.kind.get() != crate::change_set::EntityKind::Input as u32 {
                return None;
            }
            let mut r = *view.range::<InputRecord>(INPUTS, c.index.get() as usize, 1)?.first()?;
            if r.identity.as_bytes() != c.old_identity {
                return None;
            }
            let path = view.range::<u8>(
                STRINGS,
                r.path_offset.get() as usize,
                r.path_length.get() as usize,
            )?;
            if path.first() != Some(&b'/')
                || path.contains(&0)
                || *blake3::hash(&path).as_bytes() != c.path
            {
                return None;
            }
            let path = PathBuf::from(crate::util::os_str(&path));
            r.identity = Identity::path(&path)?;
            changed.push((c.index.get() as usize, r, path.clone()));
            if c.digest != [0; 32] {
                digests.insert(path, c.digest);
            }
        }
        self.content_digests = digests;
        self.trusted_digests = contract.header.flags.get() & 8 != 0;
        if ctx.args.perf {
            eprintln!(
                "incremental: ChangeSet accepted generation={} reported={} dependency_operations={}",
                view.generation,
                changed.len(),
                changed.len()
            );
        }
        Some(changed)
    }

    pub(crate) fn prepare<E: Target>(&mut self, ctx: &Context<E>, _size: u64) -> Option<Plan> {
        self.dependencies = dependencies(ctx)?;
        None
    }

    pub(crate) fn publish<E: Target>(&mut self, ctx: &Context<E>) -> Option<usize> {
        let _t = ctx.timer("state_write");
        if crate::error::has_warning() {
            return None;
        }
        if self.dependencies.is_empty()
            || self.dependencies.iter().any(|d| Identity::path(&d.path) != Some(d.identity))
        {
            return None;
        }
        let output = Identity::path(&self.output)?;
        let mut strings = Vec::new();
        let mut inputs = Vec::with_capacity(self.dependencies.len());
        for d in &self.dependencies {
            let path = d.path.as_os_str().as_encoded_bytes();
            inputs.push(InputRecord {
                identity: d.identity,
                path_offset: u32::try_from(strings.len()).ok()?.into(),
                path_length: u32::try_from(path.len()).ok()?.into(),
                directory: u32::from(d.directory).into(),
                reserved: 0.into(),
            });
            strings.extend_from_slice(path);
            if strings.len() > ENTRY_LIMIT {
                return None;
            }
        }
        let base =
            size_of::<Header>().checked_add(inputs.as_bytes().len())?.checked_add(strings.len())?;
        if base > ENTRY_LIMIT {
            return None;
        }
        let (objects, sections, resolved, archives) = crate::micro_link::take_images(&mut strings);
        let merge = crate::merge_image::take();
        let merge_uses = crate::micro_link::take_merge_uses();
        let dependency_tables = crate::semantic_cells::take();
        let object_links: Vec<_> = inputs
            .iter()
            .map(|r| {
                let path = &strings[r.path_offset.get() as usize
                    ..(r.path_offset.get() + r.path_length.get()) as usize];
                let mut matching = objects.iter().enumerate().filter(|(_, o)| {
                    &strings[o.path_offset.get() as usize
                        ..(o.path_offset.get() + o.path_length.get()) as usize]
                        == path
                });
                let first = matching.next();
                let count = usize::from(first.is_some()) + matching.count();
                let begin = first.map_or_else(
                    || {
                        archives
                            .iter()
                            .position(|a| {
                                &strings[a.path_offset.get() as usize
                                    ..(a.path_offset.get() + a.path_length.get()) as usize]
                                    == path
                            })
                            .map_or(0, |i| i + 1)
                    },
                    |(i, _)| i,
                );
                InputObjects { begin: (begin as u32).into(), count: (count as u32).into() }
            })
            .collect();
        let build_id = ctx.buildid.as_ref().map(|b| BuildIdImage {
            offset: (b.hdr.shdr.sh_offset.get() + 16).into(),
            size: (ctx.args.build_id.size() as u64).into(),
        });
        let config = if delta_eligible(&ctx.args)
            && matches!(ctx.args.build_id, BuildId::None | BuildId::Hash(_))
        {
            let output = self.output.as_os_str().as_encoded_bytes();
            let config = MicroConfig {
                version: 1.into(),
                target: EM_X86_64.into(),
                flags: (u32::from(ctx.args.perf)
                    | (u32::from(ctx.args.incremental_verify) << 1)
                    | (u32::from(ctx.args.relax) << 2)
                    | (u32::from(ctx.args.icf) << 3)
                    | (u32::from(ctx.args.gc_sections) << 4)
                    | (u32::from(ctx.args.overwrite_output_file) << 5))
                    .into(),
                build_id_size: (ctx.args.build_id.size() as u32).into(),
                output_offset: u32::try_from(strings.len()).ok()?.into(),
                output_length: u32::try_from(output.len()).ok()?.into(),
                compatibility: compatibility()?,
                reserved: [0; 8],
            };
            strings.extend_from_slice(output);
            Some(config)
        } else {
            None
        };
        let mut tables = vec![
            (INPUTS, size_of::<InputRecord>(), inputs.as_bytes().to_vec()),
            (STRINGS, 1, strings),
            (LEAVES, 32, ctx.build_id_leaves.as_bytes().to_vec()),
            (SUBTREES, 32, ctx.build_id_subtrees.as_bytes().to_vec()),
            (BOOT, 32, boot()?.to_vec()),
            (
                TELEMETRY,
                size_of::<Telemetry>(),
                Telemetry {
                    full_ns: self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
                    read_byte_ns: 0,
                    metadata_item_ns: 0,
                    hash_byte_ns: 0,
                    fixed_ns: 0,
                    samples: 0,
                    last_micro_ns: 0,
                }
                .disk()
                .as_bytes()
                .to_vec(),
            ),
        ];
        if let Some(config) = config {
            tables.push((MICRO_CONFIG, size_of::<MicroConfig>(), config.as_bytes().to_vec()));
        }
        if let Some(b) = build_id {
            tables.push((BUILD_ID, size_of::<BuildIdImage>(), b.as_bytes().to_vec()));
        }
        let rich_size = base
            .checked_add(objects.as_bytes().len())?
            .checked_add(sections.as_bytes().len())?
            .checked_add(resolved.as_bytes().len())?
            .checked_add(ctx.build_id_leaves.as_bytes().len())?
            .checked_add(32 * 64)?;
        let rich = rich_size <= ENTRY_LIMIT && delta_eligible(&ctx.args) && !objects.is_empty();
        if rich {
            tables.push((
                INPUT_OBJECTS,
                size_of::<InputObjects>(),
                object_links.as_bytes().to_vec(),
            ));
            let object_topology: Vec<_> = objects.iter().map(|o| o.topology()).collect();
            let object_revisions: Vec<_> = objects.iter().map(|o| o.source_digest).collect();
            let section_topology: Vec<_> = sections.iter().map(|s| s.topology()).collect();
            let section_revisions: Vec<_> = sections.iter().map(|s| s.revision()).collect();
            tables.push((
                OBJECTS,
                size_of::<ObjectTopology>(),
                object_topology.as_bytes().to_vec(),
            ));
            tables.push((OBJECT_REVISIONS, 32, object_revisions.as_bytes().to_vec()));
            tables.push((
                SECTIONS,
                size_of::<SectionTopology>(),
                section_topology.as_bytes().to_vec(),
            ));
            tables.push((
                SECTION_REVISIONS,
                size_of::<SectionRevision>(),
                section_revisions.as_bytes().to_vec(),
            ));
            tables.push((RESOLVED, size_of::<ResolvedValue>(), resolved.as_bytes().to_vec()));
            tables.push((ARCHIVES, size_of::<ArchiveImage>(), archives.as_bytes().to_vec()));
            if let Some((cells, consumers, links, edges)) = dependency_tables
                && !cells.is_empty()
            {
                tables.push((
                    CELLS,
                    size_of::<crate::semantic_cells::Cell>(),
                    cells.as_bytes().to_vec(),
                ));
                tables.push((CELL_CONSUMERS, 4, consumers.as_bytes().to_vec()));
                tables.push((
                    SECTION_CELLS,
                    size_of::<crate::semantic_cells::SectionCells>(),
                    links.as_bytes().to_vec(),
                ));
                tables.push((CELL_EDGES, 4, edges.as_bytes().to_vec()));
            }
            if let (Some((fragments, refs)), Some((links, uses))) = (merge, merge_uses)
                && !fragments.is_empty()
            {
                let links: Vec<_> = links
                    .into_iter()
                    .map(|(begin, count)| InputObjects { begin: begin.into(), count: count.into() })
                    .collect();
                tables.push((
                    MERGE_FRAGMENTS,
                    size_of::<crate::merge_image::Fragment>(),
                    fragments.as_bytes().to_vec(),
                ));
                tables.push((MERGE_REFS, 4, refs.as_bytes().to_vec()));
                tables.push((MERGE_OBJECTS, size_of::<InputObjects>(), links.as_bytes().to_vec()));
                tables.push((
                    MERGE_USES,
                    size_of::<crate::merge_image::Contribution>(),
                    uses.as_bytes().to_vec(),
                ));
            }
        }
        let data = encode(self.command, output, rich, tables.clone())
            .or_else(|| {
                tables.retain(|t| {
                    !matches!(
                        t.0,
                        MERGE_FRAGMENTS
                            | MERGE_REFS
                            | MERGE_OBJECTS
                            | MERGE_USES
                            | CELLS
                            | CELL_CONSUMERS
                            | SECTION_CELLS
                            | CELL_EDGES
                    )
                });
                encode(self.command, output, rich, tables.clone())
            })
            .or_else(|| {
                tables.retain(|t| matches!(t.0, INPUTS | STRINGS | BOOT));
                encode(self.command, output, false, tables)
            })?;
        self.state = None;
        self.write_state(&data)?;
        Some(data.len())
    }
}

fn dependencies<E: Target>(_ctx: &Context<E>) -> Option<Vec<Dependency>> {
    let mut paths = BTreeMap::new();
    for mf in mapped_file::file_pool() {
        if mf.parent.is_some() {
            continue;
        }
        let identity = mf.identity?;
        if identity.mode.get() & libc::S_IFMT != libc::S_IFREG {
            return None;
        }
        let path = std::path::absolute(&mf.name).ok()?;
        if RESPONSES.lock().unwrap().contains(&path) {
            continue;
        }
        paths.insert(path.clone(), Dependency { path, identity, directory: false });
    }
    for (path, identity) in SEARCH_PATHS.lock().unwrap().iter() {
        paths.entry(path.clone()).or_insert(Dependency {
            path: path.clone(),
            identity: (*identity)?,
            directory: true,
        });
    }
    Some(paths.into_values().collect())
}

fn coalesce(mut ranges: Vec<Range<u64>>) -> Vec<Range<u64>> {
    ranges.retain(|r| !r.is_empty());
    ranges.sort_by_key(|r| r.start);
    let mut result: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        if let Some(last) = result.last_mut()
            && r.start <= last.end
        {
            last.end = last.end.max(r.end);
        } else {
            result.push(r);
        }
    }
    result
}

pub(crate) struct Plan {
    pub(crate) dirty_objects: Vec<bool>,
    ranges: Vec<Range<u64>>,
    pub(crate) dirty_shards: Vec<bool>,
    pub(crate) leaves: Vec<[u8; 32]>,
    rewritten: u64,
    pages: u64,
    size: u64,
}

impl Plan {
    pub(crate) fn print(&self) {
        eprintln!(
            "incremental: patch reused={} rewritten={} ranges={} dirty_pages={} reuse={:.2}% build_id_shards={}/{}",
            self.size - self.rewritten,
            self.rewritten,
            self.ranges.len(),
            self.pages,
            100.0 * (self.size - self.rewritten) as f64 / self.size as f64,
            self.dirty_shards.iter().filter(|&&v| v).count(),
            self.dirty_shards.len()
        );
    }
}

pub(crate) fn verify<E: Target>(ctx: &Context<E>, buf: &[u8]) {
    let _t = ctx.timer("incremental_verify");
    let output =
        std::path::absolute(mapped_file::apply_chroot(&ctx.args.chroot, &ctx.args.output)).unwrap();
    let path = output.with_file_name(format!(".mold-verify-{}", std::process::id()));
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.env("MOLD_JOBS", "0").env_remove("MOLD_CHANGESET_FD");
    if let Some(cwd) = &ctx.args.orig_cwd {
        cmd.current_dir(cwd);
    }
    cmd.args(ctx.cmdline_args.iter().skip(1).filter(|arg| {
        !matches!(arg.as_encoded_bytes(), b"--incremental" | b"--incremental-verify" | b"--perf")
    }))
    .args(["--no-incremental", "--no-fork", "-o"])
    .arg(&path);
    let result = (|| -> std::io::Result<()> {
        if !cmd.status()?.success() {
            return Err(std::io::Error::other("full verifier failed"));
        }
        let mut file = File::open(&path)?;
        if file.metadata()?.len() != buf.len() as u64 {
            return Err(std::io::Error::other("output size differs"));
        }
        let mut block = vec![0; 1024 * 1024];
        for (i, expected) in buf.chunks(block.len()).enumerate() {
            file.read_exact(&mut block[..expected.len()])?;
            if expected != &block[..expected.len()] {
                return Err(std::io::Error::other(format!(
                    "output differs near byte {}",
                    i * block.len()
                )));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&path);
    if let Err(e) = result {
        crate::fatal!("incremental verification: {e}");
    }
    if ctx.args.perf {
        eprintln!("incremental: verified byte-for-byte against forced full link");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_state_is_a_miss() {
        let identity =
            Identity { mode: libc::S_IFREG.into(), present: 1.into(), ..Identity::new_zeroed() };
        let input = InputRecord {
            identity,
            path_offset: 0.into(),
            path_length: 2.into(),
            directory: 0.into(),
            reserved: 0.into(),
        };
        let valid = encode(
            [0; 32],
            identity,
            false,
            vec![(INPUTS, 80, input.as_bytes().to_vec()), (STRINGS, 1, b"/a".to_vec())],
        )
        .unwrap();
        assert!(View::parse(&valid).is_some());
        for length in [0, 7, 175, valid.len() - 1] {
            assert!(View::parse(&valid[..length]).is_none());
        }
        for offset in [0, 8, 24, 144, 176, 192, 208, valid.len() - 1] {
            let mut data = valid.clone();
            data[offset] ^= 255;
            assert!(View::parse(&data).is_none());
        }
        let mut data = valid.clone();
        data[184..192].fill(255);
        resign(&mut data);
        assert!(View::parse(&data).is_none());
        let cell = crate::semantic_cells::Cell {
            key: [7; 32],
            kind: 0.into(),
            properties: 0.into(),
            begin: 2.into(),
            capacity: 1.into(),
        };
        let image = encode(
            [0; 32],
            identity,
            true,
            vec![
                (INPUTS, 80, input.as_bytes().to_vec()),
                (STRINGS, 1, b"/a".to_vec()),
                (CELLS, 48, cell.as_bytes().to_vec()),
                (CELL_CONSUMERS, 4, U32::new(0).as_bytes().to_vec()),
            ],
        )
        .unwrap();
        assert!(View::load(&image).unwrap().cell(&[7; 32], 0).is_none());
        let identity = Identity { size: 4.into(), ..identity };
        let fragment = crate::merge_image::Fragment {
            key: [7; 32],
            offset: 0.into(),
            length: 4.into(),
            reserved: 0.into(),
        };
        let image = encode(
            [0; 32],
            identity,
            true,
            vec![
                (INPUTS, 80, input.as_bytes().to_vec()),
                (STRINGS, 1, b"/a".to_vec()),
                (MERGE_FRAGMENTS, 48, fragment.as_bytes().to_vec()),
                (MERGE_REFS, 4, U32::new(0).as_bytes().to_vec()),
            ],
        )
        .unwrap();
        assert!(View::load(&image).unwrap().merge_fragment(&[7; 32]).is_none());
    }
    #[test]
    fn partial_state_commit_is_bounded() {
        let directory = std::env::temp_dir().join(format!("mold-pages-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("image.state");
        let identity =
            Identity { mode: libc::S_IFREG.into(), present: 1.into(), ..Identity::new_zeroed() };
        let input = InputRecord {
            identity,
            path_offset: 0.into(),
            path_length: 2.into(),
            directory: 0.into(),
            reserved: 0.into(),
        };
        let data = encode(
            [0; 32],
            identity,
            false,
            vec![
                (INPUTS, 80, input.as_bytes().to_vec()),
                (STRINGS, 1, b"/a".to_vec()),
                (LEAVES, 32, vec![0; 32 * 20_000]),
            ],
        )
        .unwrap();
        std::fs::write(&path, &data).unwrap();
        let file = File::open(&path).unwrap();
        let mut session = Session {
            _lock: file.try_clone().unwrap(),
            cache: directory.clone(),
            path: path.clone(),
            output: path.clone(),
            command: [0; 32],
            started: std::time::Instant::now(),
            state: Some(mapped_file::map_state(&file).unwrap()),
            dependencies: Vec::new(),
            changed: None,
            content_digests: BTreeMap::new(),
            change_contract: false,
            trusted_digests: false,
            reason: "test",
        };
        let (data_pages, index_pages, bytes) =
            session.commit_records(vec![(LEAVES, 1, vec![9; 32])], identity).unwrap();
        let layout = TablePages::new(20_000, 32).unwrap();
        assert_eq!(
            (data_pages + index_pages, bytes),
            (1 + layout.depth, (1 + layout.depth) * state_page() + ROOT_BYTES)
        );
        let updated = std::fs::read(&path).unwrap();
        let old = View::parse(&data).unwrap();
        let new = View::parse(&updated).unwrap();
        assert_eq!(new.generation, 2);
        assert_eq!(new.range::<[u8; 32]>(LEAVES, 1, 1).unwrap(), [[9; 32]]);
        for kind in [INPUTS, STRINGS] {
            let t = old.descriptor(kind).unwrap();
            let start = t.offset.get() as usize;
            assert_eq!(&data[start..start + state_page()], &updated[start..start + state_page()]);
        }
        let leaf = old.descriptor(LEAVES).unwrap();
        let layout = TablePages::new(leaf.count.get() as usize, 32).unwrap();
        let offset = leaf.offset.get() as usize + layout.index * state_page();
        let mut stale = updated.clone();
        stale[offset..offset + state_page()].copy_from_slice(&data[offset..offset + state_page()]);
        assert!(View::parse(&stale).unwrap().records::<[u8; 32]>(LEAVES).is_none());
        let mut interrupted = updated.clone();
        interrupted[ROOT_BYTES..2 * ROOT_BYTES].fill(0);
        let view = View::parse(&interrupted).unwrap();
        assert_eq!(view.generation, 1);
        assert!(view.records::<[u8; 32]>(LEAVES).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn output_recertification_rejects_changed_bytes() {
        let path = std::env::temp_dir().join(format!("mold-recert-{}", std::process::id()));
        let mut bytes = vec![0u8; 4096];
        bytes[128] = 7;
        let leaf = *blake3::hash(&bytes).as_bytes();
        let digest = blake3::hash(&leaf);
        bytes[32..52].copy_from_slice(&digest.as_bytes()[..20]);
        std::fs::write(&path, &bytes).unwrap();
        let identity = Identity::path(&path).unwrap();
        let name = path.as_os_str().as_encoded_bytes();
        let input = InputRecord {
            identity,
            path_offset: 0.into(),
            path_length: (name.len() as u32).into(),
            directory: 0.into(),
            reserved: 0.into(),
        };
        let id = BuildIdImage { offset: 32.into(), size: 20.into() };
        let data = encode(
            [0; 32],
            identity,
            false,
            vec![
                (INPUTS, 80, input.as_bytes().to_vec()),
                (STRINGS, 1, name.to_vec()),
                (LEAVES, 32, leaf.to_vec()),
                (BUILD_ID, 16, id.as_bytes().to_vec()),
            ],
        )
        .unwrap();
        let view = View::parse(&data).unwrap();
        assert!(recertify(&view, &path));
        bytes[128] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        let new_identity = Identity::path(&path).unwrap();
        let new_input = InputRecord { identity: new_identity, ..input };
        let corrupt = encode(
            [0; 32],
            new_identity,
            false,
            vec![
                (INPUTS, 80, new_input.as_bytes().to_vec()),
                (STRINGS, 1, name.to_vec()),
                (LEAVES, 32, leaf.to_vec()),
                (BUILD_ID, 16, id.as_bytes().to_vec()),
            ],
        )
        .unwrap();
        assert!(!recertify(&View::parse(&corrupt).unwrap(), &path));
        std::fs::remove_file(path).unwrap();
    }
}

fn resign(data: &mut [u8]) -> Option<()> {
    let (header, _) = Header::try_ref_from_prefix(data).ok()?;
    let end = 184usize.checked_add((header.directory_count.get() as usize).checked_mul(64)?)?;
    data.get_mut(144..176)?.fill(0);
    let hash = *blake3::hash(data.get(..end)?).as_bytes();
    data[144..176].copy_from_slice(&hash);
    Some(())
}

fn encode(
    command: [u8; 32],
    output: Identity,
    rich: bool,
    mut tables: Vec<(u32, usize, Vec<u8>)>,
) -> Option<Vec<u8>> {
    tables.sort_by_key(|t| t.0);
    let mut length = 2 * ROOT_BYTES;
    for (_, size, bytes) in &tables {
        if !bytes.len().is_multiple_of(*size) {
            return None;
        }
        let pages = TablePages::new(bytes.len() / size, *size)?;
        length = length.checked_add(pages.total()?.checked_mul(state_page())?)?;
    }
    if length > ENTRY_LIMIT || 184 + tables.len() * 64 > ROOT_BYTES {
        return None;
    }
    let header = Header {
        magic: MAGIC,
        version: VERSION.into(),
        target: EM_X86_64.into(),
        length: (length as u64).into(),
        capability: if rich { Capability::Delta } else { Capability::NullOnly },
        reserved: [0; 7],
        command,
        output,
        directory_offset: 184.into(),
        directory_count: (tables.len() as u32).into(),
        reserved2: (state_page() as u32).into(),
        checksum: [0; 32],
    };
    let mut data = vec![0; length];
    data[..176].copy_from_slice(header.as_bytes());
    data[176..184].copy_from_slice(U64::new(1).as_bytes());
    let mut offset = 2 * ROOT_BYTES;
    for (i, (kind, size, bytes)) in tables.iter().enumerate() {
        let pages = TablePages::new(bytes.len() / size, *size)?;
        let mut t = TableRecord {
            kind: (*kind).into(),
            version: 1.into(),
            offset: (offset as u64).into(),
            length: (bytes.len() as u64).into(),
            record_size: (*size as u32).into(),
            count: ((bytes.len() / size) as u32).into(),
            checksum: [0; 32],
        };
        let capacity = (state_page() - 64) / size;
        let mut digests = Vec::with_capacity(pages.data);
        for (index, chunk) in bytes.chunks(capacity * size).enumerate() {
            let page = page_bytes(*kind, index, 1, *size, chunk)?;
            digests.push(<[u8; 32]>::read_from_bytes(&page[24..56]).ok()?);
            let start = offset.checked_add((pages.index + index).checked_mul(state_page())?)?;
            data[start..start + state_page()].copy_from_slice(&page);
        }
        for level in 0..pages.depth {
            let mut parents = Vec::with_capacity(pages.levels[level]);
            for (index, values) in digests.chunks((state_page() - 64) / 32).enumerate() {
                let flat = pages.flat(level, index)?;
                let page = index_bytes(*kind, flat, level, 1, values)?;
                parents.push(<[u8; 32]>::read_from_bytes(&page[24..56]).ok()?);
                let start = offset.checked_add(flat.checked_mul(state_page())?)?;
                data[start..start + state_page()].copy_from_slice(&page);
            }
            digests = parents;
        }
        if let Some(root) = digests.first() {
            t.checksum = *root;
        }
        data[184 + i * 64..184 + (i + 1) * 64].copy_from_slice(t.as_bytes());
        offset = offset.checked_add(pages.total()?.checked_mul(state_page())?)?;
    }
    resign(&mut data)?;
    View::parse(&data)?;
    Some(data)
}

static RESPONSES: Mutex<std::collections::BTreeSet<PathBuf>> =
    Mutex::new(std::collections::BTreeSet::new());
pub(crate) fn record_response(path: &Path) {
    if let Ok(path) = std::path::absolute(path) {
        RESPONSES.lock().unwrap().insert(path);
    }
}
pub(crate) fn semantic_tracking() -> bool {
    TRACK_SEMANTIC.load(Ordering::Relaxed)
}
pub(crate) fn tracking() -> bool {
    TRACK_SEARCH.load(Ordering::Relaxed)
}

fn quota_lock(cache: &Path) -> Option<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache.join("quota.lock"))
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

fn key_lock(cache: &Path, state: &Path) -> Option<File> {
    loop {
        let quota = quota_lock(cache)?;
        let path = state.with_extension("keylock");
        if !path.exists() {
            let mut locks: Vec<_> = std::fs::read_dir(cache)
                .ok()?
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|e| e == "keylock"))
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect();
            locks.sort_by_key(|e| e.0);
            let mut count = locks.len();
            for (_, old) in locks {
                if count < 4096 {
                    break;
                }
                if let Ok(file) = OpenOptions::new().read(true).write(true).open(&old)
                    && file.try_lock().is_ok()
                {
                    let _ = std::fs::remove_file(old.with_extension("state"));
                    let _ = std::fs::remove_file(old.with_extension("pending"));
                    std::fs::remove_file(old).ok()?;
                    count -= 1;
                }
            }
            if count >= 4096 {
                return None;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .ok()?;
        match file.try_lock() {
            Ok(()) => {
                let _ = file.set_times(
                    std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()),
                );
                drop(quota);
                return Some(file);
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                drop(quota);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(_) => return None,
        }
    }
}

struct Patch {
    offset: u64,
    bytes: Vec<u8>,
}

struct MicroPlan {
    inputs: Vec<(usize, InputRecord)>,
    objects: Vec<(usize, ObjectImage)>,
    sections: Vec<(usize, SectionImage)>,
    patches: Vec<Patch>,
    leaves: Vec<[u8; 32]>,
    dirty_leaves: std::collections::BTreeSet<usize>,
    subtrees: BTreeMap<usize, [u8; 32]>,
    subtree_count: usize,
    dirty_subtrees: std::collections::BTreeSet<usize>,
    build_id: Option<BuildIdImage>,
    candidate_bytes: u64,
    parsed_bytes: u64,
    changed_objects: usize,
    selective_objects: usize,
    affected_sections: usize,
    semantic_updates: Vec<(u32, usize, Vec<u8>)>,
    changed_fragments: usize,
    validated_pages: usize,
    read_bytes: u64,
    read_ns: u64,
    metadata_items: u64,
    metadata_ns: u64,
    hash_bytes: u64,
    hash_ns: u64,
}

impl Session {
    fn micro_plan<E: Target>(&mut self, ctx: &Context<E>) -> Option<MicroPlan> {
        let _t = ctx.timer("micro_dependency_validate");
        if !delta_eligible(&ctx.args) {
            self.reason = "unsupported delta option";
            return None;
        }
        let view = View::load(self.state.as_deref()?)?;
        if !matches!(view.header.capability, Capability::Delta) {
            return None;
        }
        self.reason = "semantic tables missing or corrupt";
        let telemetry = view.records::<TelemetryDisk>(TELEMETRY).and_then(|v| v.first().copied());
        let leaves = view.records::<[u8; 32]>(LEAVES)?;
        let subtree_count = view.descriptor(SUBTREES).map_or(0, |t| t.count.get() as usize);
        if subtree_count != 0
            && subtree_count as u64
                != view
                    .header
                    .output
                    .size
                    .get()
                    .div_ceil(crate::build_id_tree::granularity() as u64)
        {
            return None;
        }
        let build_id = view.records::<BuildIdImage>(BUILD_ID).and_then(|b| b.first().copied());
        if matches!(ctx.args.build_id, BuildId::Hash(_))
            && (leaves.len() as u64 != view.header.output.size.get().div_ceil(SHARD as u64)
                || build_id.is_none())
        {
            return None;
        }
        let diff = self.changed.as_ref()?;
        if (diff.is_empty() && !ctx.args.incremental_verify) || diff.len() > 8 {
            self.reason = "changed-input budget";
            return None;
        }
        let mut inputs = Vec::with_capacity(diff.len());
        let mut new_objects = Vec::new();
        let mut new_sections = Vec::new();
        let mut changed = Vec::new();
        for (i, r, path) in diff {
            if r.directory.get() != 0 || r.identity.present.get() != 1 {
                self.reason = "search witness changed";
                return None;
            }
            let link = *view.range::<InputObjects>(INPUT_OBJECTS, *i, 1)?.first()?;
            if link.count.get() == 0 && link.begin.get() == 0 {
                self.reason = "object admission";
                return None;
            }
            let images = if link.count.get() == 0 {
                Vec::new()
            } else {
                let begin = link.begin.get() as usize;
                let count = link.count.get() as usize;
                let topology = view.range::<ObjectTopology>(OBJECTS, begin, count)?;
                let revisions = view.range::<[u8; 32]>(OBJECT_REVISIONS, begin, count)?;
                topology
                    .into_iter()
                    .zip(revisions)
                    .map(|(t, r)| ObjectImage::from_topology(t, r))
                    .collect()
            };
            changed.push((path, images, r.identity, link.begin.get() as usize));
            inputs.push((*i, *r));
        }
        drop(_t);
        let output = File::open(&self.output).ok()?;
        let mut patches = Vec::new();
        let mut candidate_bytes = 0;
        let mut parsed_bytes = 0;
        let mut changed_objects = 0usize;
        let mut selective_objects = 0usize;
        let mut affected_sections = 0usize;
        let mut semantic_updates = Vec::new();
        let mut changed_fragments = 0usize;
        let mut merge_delta: BTreeMap<usize, i64> = BTreeMap::new();
        let mut consumer_changes: BTreeMap<
            usize,
            (std::collections::BTreeSet<u32>, std::collections::BTreeSet<u32>),
        > = BTreeMap::new();
        let (
            mut read_bytes,
            mut read_ns,
            mut metadata_items,
            mut metadata_ns,
            mut hash_bytes,
            mut hash_ns,
        ) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        for (path, images, identity, object_begin) in &changed {
            let _t = ctx.timer("micro_changed_parse");
            let mut file = File::open(path).ok()?;
            if Identity::from_metadata(&file.metadata().ok()?) != Some(*identity) {
                return None;
            }
            let mut data = vec![0; usize::try_from(identity.size.get()).ok()?];
            let reading = std::time::Instant::now();
            file.read_exact(&mut data).ok()?;
            read_ns = read_ns.saturating_add(reading.elapsed().as_nanos() as u64);
            read_bytes = read_bytes.saturating_add(data.len() as u64);
            if !self.trusted_digests
                && self
                    .content_digests
                    .get(*path)
                    .is_some_and(|d| blake3::hash(&data).as_bytes() != d)
            {
                self.reason = "ChangeSet content digest";
                return None;
            }
            if images.is_empty() {
                let archive = *view
                    .range::<ArchiveImage>(ARCHIVES, object_begin.checked_sub(1)?, 1)?
                    .first()?;
                if !data.starts_with(b"!<thin>\n")
                    || crate::micro_link::archive_surface(&data)?
                        != (archive.guard, archive.member_count.get())
                {
                    self.reason = "archive topology";
                    return None;
                }
                continue;
            }
            if images.iter().any(|i| i.source_offset.get() != 0) {
                let archives = view.records::<ArchiveImage>(ARCHIVES)?;
                let strings = view.records::<u8>(STRINGS)?;
                let archive = archives.iter().find(|a| {
                    let start = a.path_offset.get() as usize;
                    let end = start.checked_add(a.path_length.get() as usize);
                    end.and_then(|end| strings.get(start..end))
                        == Some(path.as_os_str().as_encoded_bytes())
                })?;
                if archive.reserved.get() != 0
                    || images.len() != archive.member_count.get() as usize
                    || crate::micro_link::archive_surface(&data)?
                        != (archive.guard, archive.member_count.get())
                {
                    return None;
                }
            }
            drop(_t);
            for (object_index, image) in images.iter().enumerate() {
                let start = usize::try_from(image.source_offset.get()).ok()?;
                let end = start.checked_add(usize::try_from(image.source_size.get()).ok()?)?;
                let source = data.get(start..end)?;
                if image.source_offset.get() != 0
                    && blake3::hash(source).as_bytes() == &image.source_digest
                {
                    continue;
                }
                if let Some(t) = telemetry
                    && t.samples.get() != 0
                    && t.full_ns.get() != 0
                {
                    let predicted = u128::from(t.fixed_ns.get())
                        + u128::from(read_bytes) * u128::from(t.read_byte_ns.get())
                        + u128::from(image.metadata_items.get())
                            * u128::from(t.metadata_item_ns.get())
                        + u128::from(image.payload_bytes.get()) * u128::from(t.hash_byte_ns.get());
                    if predicted >= u128::from(t.full_ns.get()) {
                        self.reason = "semantic cost model";
                        return None;
                    }
                }
                let _t = ctx.timer("micro_changed_parse");
                let parsing = std::time::Instant::now();
                let raw = crate::micro_link::RawObject::parse(source)?;
                metadata_ns = metadata_ns.saturating_add(parsing.elapsed().as_nanos() as u64);
                metadata_items = metadata_items.saturating_add(image.metadata_items.get());
                let mut updated = *image;
                updated.source_digest = if image.source_offset.get() != 0 {
                    *blake3::hash(source).as_bytes()
                } else {
                    [0; 32]
                };
                new_objects.push((object_begin + object_index, updated));
                parsed_bytes += source.len() as u64;
                if ctx.args.perf {
                    eprintln!(
                        "incremental: object_cost bytes={} sections={} symbols={} relocations={} merge_bytes={} payload_bytes={} compressed_bytes={}",
                        raw.cost.file_bytes,
                        raw.cost.sections,
                        raw.cost.symbols,
                        raw.cost.relocations,
                        raw.cost.merge_bytes,
                        raw.cost.payload_bytes,
                        raw.cost.compressed_bytes
                    );
                }
                changed_objects += 1;
                if changed_objects > 8 {
                    return None;
                }
                drop(_t);
                let _t = ctx.timer("micro_semantic_compare");
                let selective = raw.surfaces != image.surfaces;
                if selective {
                    if ctx.args.gc_sections
                        || ctx.args.icf
                        || raw.headers.iter().any(|h| h.sh_type.get() == SHT_GROUP)
                        || !raw.surfaces.selective_equivalent(image.surfaces)
                    {
                        self.reason = "semantic surfaces";
                        return None;
                    }
                    selective_objects += 1;
                    if raw.surfaces.merge_changed(image.surfaces) {
                        self.reason = "merge fragment proof";
                        let link = *view
                            .range::<InputObjects>(MERGE_OBJECTS, object_begin + object_index, 1)?
                            .first()?;
                        let next = raw.merge_contributions()?;
                        if link.count.get() as usize != next.len() {
                            return None;
                        }
                        let previous = view.range::<crate::merge_image::Contribution>(
                            MERGE_USES,
                            link.begin.get() as usize,
                            next.len(),
                        )?;
                        let mut occurrences: BTreeMap<u32, usize> = BTreeMap::new();
                        for (i, (old, new)) in previous.iter().zip(&next).enumerate() {
                            let occurrence = occurrences.entry(new.section.get()).or_default();
                            let bytes = raw.merge_fragment(new.section.get(), *occurrence)?;
                            *occurrence += 1;
                            if old.reserved.get() != 0 || old.section != new.section {
                                return None;
                            }
                            if old.key == new.key {
                                continue;
                            }
                            let (old_index, _) = view.merge_fragment(&old.key)?;
                            let (new_index, fragment) = view.merge_fragment(&new.key)?;
                            if fragment.length.get() as usize != bytes.len()
                                || fragment.offset.get().checked_add(bytes.len() as u64)?
                                    > view.header.output.size.get()
                            {
                                return None;
                            }
                            let mut existing = vec![0; bytes.len()];
                            #[cfg(unix)]
                            std::os::unix::fs::FileExt::read_exact_at(
                                &output,
                                &mut existing,
                                fragment.offset.get(),
                            )
                            .ok()?;
                            if existing != bytes {
                                return None;
                            }
                            *merge_delta.entry(old_index).or_default() -= 1;
                            *merge_delta.entry(new_index).or_default() += 1;
                            semantic_updates.push((
                                MERGE_USES,
                                link.begin.get() as usize + i,
                                new.as_bytes().to_vec(),
                            ));
                            changed_fragments += 1;
                        }
                    }
                    new_objects.last_mut()?.1.surfaces = raw.surfaces;
                }
                let section_start = image.sections_begin.get() as usize;
                let count = image.sections_count.get() as usize;
                if section_start.checked_add(count)?
                    > view.descriptor(SECTIONS)?.count.get() as usize
                    || view.descriptor(SECTIONS)?.count != view.descriptor(SECTION_REVISIONS)?.count
                {
                    return None;
                }
                let values = std::cell::RefCell::new(BTreeMap::new());
                let symbol_value = |index: u32, cell: crate::reloc_env::RelocCell| -> Option<u64> {
                    let key = (index, cell as u32);
                    if let Some(value) = values.borrow().get(&key) {
                        return Some(*value);
                    }
                    let value = view.resolved(image, index, cell as u32)?;
                    if raw.symbol_key(index)? != value.key {
                        return None;
                    }
                    values.borrow_mut().insert(key, value.value.get());
                    Some(value.value.get())
                };
                drop(_t);
                for section_index in 0..count {
                    let topology =
                        *view.record::<SectionTopology>(SECTIONS, section_start + section_index)?;
                    let revision = *view.record::<SectionRevision>(
                        SECTION_REVISIONS,
                        section_start + section_index,
                    )?;
                    let s = SectionImage::from_topology(topology, revision);
                    if s.writable.get() > 3 || s.domain.get() > 2 {
                        return None;
                    }
                    let header = raw.headers.get(s.shndx.get() as usize)?;
                    if header.sh_offset.get() != s.input_offset.get() {
                        return None;
                    }
                    let hashing = std::time::Instant::now();
                    let guard = raw.guard(s.shndx.get(), s.relsec.get())?;
                    let encoding = raw.encoding(s.relsec.get())?;
                    hash_ns = hash_ns.saturating_add(hashing.elapsed().as_nanos() as u64);
                    hash_bytes = hash_bytes
                        .saturating_add(s.size.get())
                        .saturating_add(raw.rels(s.relsec.get())?.as_bytes().len() as u64);
                    if guard == s.guard && encoding == s.encoding {
                        continue;
                    }
                    if s.writable.get() & 1 == 0
                        || (raw.surfaces.topology_changed(image.surfaces)
                            && encoding != s.encoding
                            && s.writable.get() & 2 == 0)
                    {
                        self.reason = "changed guarded section";
                        return None;
                    }
                    if raw.surfaces.topology_changed(image.surfaces)
                        && encoding != s.encoding
                        && raw.rels(s.relsec.get())?.iter().any(|r| {
                            r.r_type() != R_X86_64_PC32
                                || raw
                                    .symbol(r.r_sym())
                                    .is_none_or(|sym| sym.st_bind() == STB_LOCAL)
                        })
                    {
                        self.reason = "selective relocation dependency";
                        return None;
                    }
                    if raw.surfaces.topology_changed(image.surfaces) && encoding != s.encoding {
                        self.reason = "semantic dependency closure";
                        let section_id = section_start + section_index;
                        let link = *view
                            .range::<crate::semantic_cells::SectionCells>(
                                SECTION_CELLS,
                                section_id,
                                1,
                            )?
                            .first()?;
                        let previous = view.range::<U32>(
                            CELL_EDGES,
                            link.begin.get() as usize,
                            link.count.get() as usize,
                        )?;
                        let mut next = std::collections::BTreeSet::new();
                        for rel in raw.rels(s.relsec.get())? {
                            let (index, cell) = view.cell(
                                &raw.symbol_key(rel.r_sym())?,
                                crate::reloc_env::RelocCell::Symbol as u32,
                            )?;
                            if cell.properties.get() != 1 {
                                self.reason = "selective relocation requirement";
                                return None;
                            }
                            next.insert(index as u32);
                        }
                        if next.len() != previous.len()
                            || previous.windows(2).any(|w| w[0].get() >= w[1].get())
                        {
                            return None;
                        }
                        let old: std::collections::BTreeSet<_> =
                            previous.iter().map(|v| v.get()).collect();
                        for cell in old.difference(&next) {
                            consumer_changes
                                .entry(*cell as usize)
                                .or_default()
                                .0
                                .insert(section_id as u32);
                        }
                        for cell in next.difference(&old) {
                            consumer_changes
                                .entry(*cell as usize)
                                .or_default()
                                .1
                                .insert(section_id as u32);
                        }
                        for (i, cell) in next.into_iter().enumerate() {
                            if cell != previous[i].get() {
                                semantic_updates.push((
                                    CELL_EDGES,
                                    link.begin.get() as usize + i,
                                    U32::new(cell).as_bytes().to_vec(),
                                ));
                            }
                        }
                    }
                    affected_sections += 1;
                    let mut updated = s;
                    updated.guard = guard;
                    updated.encoding = encoding;
                    new_sections.push((section_start + section_index, updated));
                    let _t = ctx.timer("micro_section_relocate");
                    let mut candidate = raw.section(s.shndx.get())?.to_vec();
                    if candidate.len() as u64 != s.size.get() {
                        return None;
                    }
                    candidate_bytes += candidate.len() as u64;
                    for rel in raw.rels(s.relsec.get())? {
                        if rel.r_type() == R_NONE {
                            continue;
                        }
                        if matches!(
                            rel.r_type(),
                            R_X86_64_GOTPCRELX | R_X86_64_REX_GOTPCRELX | R_X86_64_CODE_4_GOTPCRELX
                        ) {
                            crate::arch::x86_64::apply_gotpcrelx(
                                &mut candidate,
                                rel,
                                symbol_value(rel.r_sym(), crate::reloc_env::RelocCell::Symbol)?,
                                symbol_value(rel.r_sym(), crate::reloc_env::RelocCell::Got)?,
                                s.address.get().checked_add(rel.r_offset())?,
                                symbol_value(
                                    rel.r_sym(),
                                    crate::reloc_env::RelocCell::RelaxPredicate,
                                )? == 1,
                            )?;
                            continue;
                        }
                        let cell = crate::arch::x86_64::relocation_cell(rel.r_type())?;
                        let sym = raw.symbol(rel.r_sym())?;
                        let value = if cell == crate::reloc_env::RelocCell::Symbol
                            && sym.st_bind() == STB_LOCAL
                            && sym.st_shndx() < SHN_LORESERVE
                        {
                            let placement = view.placement(image, sym.st_shndx())?;
                            placement.address.get().wrapping_add(sym.st_value())
                        } else {
                            symbol_value(rel.r_sym(), cell)?
                        };
                        let encoded = crate::arch::x86_64::cell_relocation(
                            rel.r_type(),
                            value,
                            rel.r_addend() as u64,
                            s.address.get().checked_add(rel.r_offset())?,
                        )?;
                        if !encoded.fits() {
                            return None;
                        }
                        encoded
                            .write(candidate.get_mut(usize::try_from(rel.r_offset()).ok()?..)?)?;
                    }
                    drop(_t);
                    let _t = ctx.timer("micro_output_compare");
                    let mut old = vec![0; candidate.len()];
                    let end = s.output_offset.get().checked_add(s.size.get())?;
                    if end > view.header.output.size.get() {
                        return None;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::FileExt::read_exact_at(
                        &output,
                        &mut old,
                        s.output_offset.get(),
                    )
                    .ok()?;
                    #[cfg(not(unix))]
                    return None;
                    let mut pos = 0;
                    while pos < candidate.len() {
                        if old[pos] == candidate[pos] {
                            pos += 1;
                            continue;
                        }
                        let start = pos;
                        pos += 1;
                        while pos < candidate.len() && old[pos] != candidate[pos] {
                            pos += 1;
                        }
                        patches.push(Patch {
                            offset: s.output_offset.get().checked_add(start as u64)?,
                            bytes: candidate[start..pos].to_vec(),
                        });
                        if patches.len() > 65536 {
                            return None;
                        }
                    }
                }
            }
            if Identity::from_metadata(&file.metadata().ok()?) != Some(*identity) {
                return None;
            }
        }
        for (index, (removed, added)) in consumer_changes {
            let cell = *view.range::<crate::semantic_cells::Cell>(CELLS, index, 1)?.first()?;
            let previous = view.range::<U32>(
                CELL_CONSUMERS,
                cell.begin.get() as usize,
                cell.capacity.get() as usize,
            )?;
            if cell.properties.get() > 1
                || (cell.kind.get() != 0 && cell.properties.get() != 0)
                || cell.capacity.get() == 0
                || previous.windows(2).any(|w| {
                    (w[0].get() == u32::MAX && w[1].get() != u32::MAX)
                        || (w[0].get() != u32::MAX && w[0].get() >= w[1].get())
                })
                || previous.iter().any(|v| {
                    v.get() != u32::MAX
                        && v.get() >= view.descriptor(SECTIONS).map_or(0, |t| t.count.get())
                })
            {
                return None;
            }
            let mut next: std::collections::BTreeSet<_> =
                previous.iter().map(|v| v.get()).take_while(|v| *v != u32::MAX).collect();
            for section in removed {
                if !next.remove(&section) {
                    return None;
                }
            }
            for section in added {
                if !next.insert(section) {
                    return None;
                }
            }
            if next.len() > previous.len() {
                self.reason = "semantic consumer capacity";
                return None;
            }
            for (i, value) in
                next.into_iter().chain(std::iter::repeat(u32::MAX)).take(previous.len()).enumerate()
            {
                if previous[i].get() != value {
                    semantic_updates.push((
                        CELL_CONSUMERS,
                        cell.begin.get() as usize + i,
                        U32::new(value).as_bytes().to_vec(),
                    ));
                }
            }
        }
        for (index, delta) in merge_delta {
            if delta == 0 {
                continue;
            }
            let refs = view.range::<U32>(MERGE_REFS, index, 1)?.first()?.get();
            let next = i64::from(refs).checked_add(delta)?;
            if next <= 0 || next > i64::from(u32::MAX) {
                self.reason = "merged unique set changed";
                return None;
            }
            semantic_updates.push((MERGE_REFS, index, U32::new(next as u32).as_bytes().to_vec()));
        }
        if Identity::path(&self.output) != Some(view.header.output) {
            return None;
        }
        if let Some(b) = build_id
            && (b.size.get() != ctx.args.build_id.size() as u64
                || b.offset.get().checked_add(b.size.get())? > view.header.output.size.get())
        {
            return None;
        }
        let mut subtrees = BTreeMap::new();
        if subtree_count != 0 && !patches.is_empty() {
            let mut ranges: Vec<_> =
                patches.iter().map(|p| p.offset..p.offset + p.bytes.len() as u64).collect();
            if let Some(b) = build_id {
                ranges.push(b.offset.get()..b.offset.get() + b.size.get());
            }
            let shards = coalesce(
                ranges
                    .iter()
                    .map(|r| r.start / SHARD as u64..r.end.div_ceil(SHARD as u64))
                    .collect(),
            );
            for range in shards {
                let start = range.start as usize * SHARD / crate::build_id_tree::granularity();
                let end = (range.end as usize * SHARD)
                    .min(view.header.output.size.get() as usize)
                    .div_ceil(crate::build_id_tree::granularity());
                for (i, value) in view
                    .range::<[u8; 32]>(SUBTREES, start, end.checked_sub(start)?)?
                    .into_iter()
                    .enumerate()
                {
                    subtrees.insert(start + i, value);
                }
            }
        }
        Some(MicroPlan {
            inputs,
            objects: new_objects,
            sections: std::mem::take(&mut new_sections),
            patches,
            leaves,
            dirty_leaves: std::collections::BTreeSet::new(),
            subtrees,
            subtree_count,
            dirty_subtrees: std::collections::BTreeSet::new(),
            build_id,
            candidate_bytes,
            parsed_bytes,
            changed_objects,
            selective_objects,
            affected_sections,
            semantic_updates,
            changed_fragments,
            validated_pages: view.validated.borrow().len(),
            read_bytes,
            read_ns,
            metadata_items,
            metadata_ns,
            hash_bytes,
            hash_ns,
        })
    }

    pub(crate) fn micro_link<E: Target>(&mut self, ctx: &Context<E>) -> bool {
        let before = IoCounters::read();
        let Some(mut plan) = self.micro_plan(ctx) else {
            return false;
        };
        let _t = ctx.timer("micro_link");
        crate::subprocess::install_signal_handler();
        crate::error::install_panic_hook();
        let size = View::load(self.state.as_deref().unwrap()).unwrap().header.output.size.get();
        let actual_bytes: u64 = plan.patches.iter().map(|p| p.bytes.len() as u64).sum();
        let mut ranges: Vec<_> =
            plan.patches.iter().map(|p| p.offset..p.offset + p.bytes.len() as u64).collect();
        if !plan.patches.is_empty()
            && matches!(ctx.args.build_id, BuildId::Hash(_))
            && let Some(id) = plan.build_id
        {
            ranges.push(id.offset.get()..id.offset.get() + id.size.get());
        }
        let pages = coalesce(ranges.iter().map(|r| r.start / 4096..r.end.div_ceil(4096)).collect())
            .iter()
            .map(|r| r.end - r.start)
            .sum::<u64>();
        let mut dirty = vec![false; (size as usize).div_ceil(SHARD)];
        for r in &ranges {
            dirty[r.start as usize / SHARD..(r.end as usize).div_ceil(SHARD)].fill(true);
        }
        let mut written = actual_bytes;
        let mut write_calls = plan.patches.len() as u64;
        if !plan.patches.is_empty() {
            let Some(mut output) =
                crate::output_file::OutputFile::reuse_existing(&ctx.args, size, 0o777)
            else {
                return false;
            };
            {
                let _t = ctx.timer("micro_output_patch");
                for patch in &plan.patches {
                    output
                        .write_at(patch.offset, &patch.bytes)
                        .unwrap_or_else(|e| crate::fatal!("incremental positioned write: {e}"));
                }
            }
            if let (BuildId::Hash(_), Some(id)) = (&ctx.args.build_id, plan.build_id) {
                let _t = ctx.timer("micro_build_id");
                let offset = id.offset.get() as usize;
                let len = id.size.get() as usize;
                let zeros = offset..offset + len;
                dirty[offset / SHARD..(offset + len).div_ceil(SHARD)].fill(true);
                let mut tree_dirty =
                    vec![false; (size as usize).div_ceil(crate::build_id_tree::granularity())];
                for range in ranges
                    .iter()
                    .cloned()
                    .chain(std::iter::once(offset as u64..(offset + len) as u64))
                {
                    tree_dirty[range.start as usize / crate::build_id_tree::granularity()
                        ..(range.end as usize).div_ceil(crate::build_id_tree::granularity())]
                        .fill(true);
                }
                for (i, is_dirty) in dirty.iter().copied().enumerate() {
                    if !is_dirty {
                        continue;
                    }
                    plan.dirty_leaves.insert(i);
                    let start = i * SHARD;
                    let end = (start + SHARD).min(size as usize);
                    if plan.subtrees.is_empty() {
                        plan.leaves[i] = crate::build_id_tree::virtual_zero_hash(
                            &output.buf()[start..end],
                            start,
                            zeros.clone(),
                        );
                        continue;
                    }
                    let first = start / crate::build_id_tree::granularity();
                    let last = end.div_ceil(crate::build_id_tree::granularity());
                    for (j, is_dirty) in tree_dirty.iter().enumerate().take(last).skip(first) {
                        if *is_dirty {
                            let substart = j * crate::build_id_tree::granularity();
                            let subend = (substart + crate::build_id_tree::granularity()).min(end);
                            plan.dirty_subtrees.insert(j);
                            plan.subtrees.insert(
                                j,
                                crate::build_id_tree::virtual_zero_subtree(
                                    &output.buf()[substart..subend],
                                    substart,
                                    zeros.clone(),
                                    substart - start,
                                ),
                            );
                        }
                    }
                    let values: Vec<_> = (first..last).map(|j| plan.subtrees[&j]).collect();
                    plan.leaves[i] = if end - start <= crate::build_id_tree::granularity() {
                        crate::build_id_tree::virtual_zero_hash(
                            &output.buf()[start..end],
                            start,
                            zeros.clone(),
                        )
                    } else {
                        crate::build_id_tree::root(&output.buf()[start..end], &values)
                    };
                }
                if ctx.args.perf {
                    eprintln!(
                        "incremental: build_id_subtrees={}/{}",
                        tree_dirty.iter().filter(|&&b| b).count(),
                        plan.subtree_count
                    );
                }
                let digest = blake3::hash(plan.leaves.as_flattened());
                output
                    .write_at(offset as u64, &digest.as_bytes()[..len])
                    .unwrap_or_else(|e| crate::fatal!("incremental build-id write: {e}"));
                written += len as u64;
                write_calls += 1;
            }
            if ctx.args.incremental_verify {
                verify(ctx, output.buf());
            }
            output.close();
        } else if ctx.args.incremental_verify {
            let file = File::open(&self.output).unwrap();
            let map = mapped_file::map_state(&file).unwrap();
            verify(ctx, &map);
        }
        self.update_micro_state(ctx, &plan);
        drop(_t);
        if ctx.args.perf {
            eprintln!(
                "incremental: {} objects={} parsed_bytes={} candidate_bytes={} actual_changed_bytes={} physical_write_bytes={} write_calls={} dirty_pages={} reuse={:.5}% build_id_shards={}/{}; global passes skipped",
                if plan.selective_objects == 0 { "MicroLink" } else { "SelectiveRelink" },
                plan.changed_objects,
                plan.parsed_bytes,
                plan.candidate_bytes,
                actual_bytes,
                written,
                write_calls,
                pages,
                100.0 * (1.0 - actual_bytes as f64 / size as f64),
                dirty.iter().filter(|&&b| b).count(),
                dirty.len()
            );
            if plan.changed_fragments != 0 {
                eprintln!(
                    "incremental: merge contribution_changes={} merged_bytes_written=0",
                    plan.changed_fragments
                );
            }
            if plan.selective_objects != 0 {
                eprintln!(
                    "incremental: dependency_edges_changed={} consumer_slots_changed={}",
                    plan.semantic_updates.iter().filter(|u| u.0 == CELL_EDGES).count(),
                    plan.semantic_updates.iter().filter(|u| u.0 == CELL_CONSUMERS).count()
                );
                eprintln!(
                    "incremental: semantic_closure objects={} sections={} generated_chunks=0 layout_changes=0",
                    plan.selective_objects, plan.affected_sections
                );
            }
            let after = IoCounters::read();
            eprintln!(
                "incremental: kernel read_bytes={} write_bytes={} syscw={} minor_faults={} major_faults={}",
                after.read.saturating_sub(before.read),
                after.write.saturating_sub(before.write),
                after.syscw.saturating_sub(before.syscw),
                after.minor.saturating_sub(before.minor),
                after.major.saturating_sub(before.major)
            );
            ctx.timers.print();
        }
        true
    }

    fn write_state(&self, data: &[u8]) -> Option<()> {
        if data.len() > ENTRY_LIMIT {
            return None;
        }
        let _quota = quota_lock(&self.cache)?;
        let pending = self.path.with_extension("pending");
        let _ = std::fs::remove_file(&pending);
        let mut entries: Vec<_> = std::fs::read_dir(&self.cache)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "state" || x == "pending"))
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let recent = std::fs::metadata(e.path().with_extension("keylock"))
                    .and_then(|m| m.modified())
                    .or_else(|_| metadata.modified())
                    .ok()?;
                Some((recent, e.path(), metadata.len()))
            })
            .collect();
        entries.sort_by_key(|e| e.0);
        let mut total = entries.iter().try_fold(0u64, |sum, e| sum.checked_add(e.2))?;
        for (_, path, length) in entries {
            if total.checked_add(data.len() as u64)? <= CACHE_LIMIT {
                break;
            }
            if path == self.path {
                continue;
            }
            let Ok(lock) =
                OpenOptions::new().read(true).write(true).open(path.with_extension("keylock"))
            else {
                continue;
            };
            if lock.try_lock().is_err() {
                continue;
            }
            std::fs::remove_file(path).ok()?;
            total = total.checked_sub(length)?;
        }
        if total.checked_add(data.len() as u64)? > CACHE_LIMIT {
            return None;
        }
        let mut file = OpenOptions::new().create_new(true).write(true).open(&pending).ok()?;
        let result = file.write_all(data).and_then(|_| std::fs::rename(&pending, &self.path));
        let _ = std::fs::remove_file(&pending);
        result.ok()?;
        Some(())
    }

    #[cfg(unix)]
    fn commit_records(
        &mut self,
        changes: Vec<(u32, usize, Vec<u8>)>,
        output: Identity,
    ) -> Option<(usize, usize, usize)> {
        use std::os::unix::fs::FileExt;
        let mapping = self.state.take()?;
        let view = View::load(&mapping)?;
        if changes.is_empty() && output == view.header.output {
            return Some((0, 0, 0));
        }
        let generation = view.generation.checked_add(1)?;
        let mut pages: BTreeMap<(u32, usize), Vec<u8>> = BTreeMap::new();
        for (kind, index, bytes) in changes {
            let t = view.descriptor(kind)?;
            let size = t.record_size.get() as usize;
            if bytes.len() != size || index >= t.count.get() as usize {
                return None;
            }
            let capacity = (state_page() - 64) / size;
            let page_index = index / capacity;
            let page = pages
                .entry((kind, page_index))
                .or_insert(view.page_data(kind, page_index)?.to_vec());
            let start = (index % capacity).checked_mul(size)?;
            page.get_mut(start..start.checked_add(size)?)?.copy_from_slice(&bytes);
        }
        let mut writes = Vec::new();
        let mut updates: BTreeMap<u32, BTreeMap<usize, [u8; 32]>> = BTreeMap::new();
        for ((kind, index), bytes) in pages {
            if view.page_data(kind, index)? == bytes {
                continue;
            }
            let t = view.descriptor(kind)?;
            let layout = TablePages::new(t.count.get() as usize, t.record_size.get() as usize)?;
            let offset = t.offset.get().checked_add(
                (layout.index.checked_add(index)? as u64).checked_mul(state_page() as u64)?,
            )?;
            let page = page_bytes(kind, index, generation, t.record_size.get() as usize, &bytes)?;
            updates
                .entry(kind)
                .or_default()
                .insert(index, <[u8; 32]>::read_from_bytes(&page[24..56]).ok()?);
            writes.push((offset, page));
        }
        let mut root = view.data.get(view.root..view.root.checked_add(ROOT_BYTES)?)?.to_vec();
        for (kind, mut children) in updates {
            let t = view.descriptor(kind)?;
            let layout = TablePages::new(t.count.get() as usize, t.record_size.get() as usize)?;
            let capacity = (state_page() - 64) / 32;
            for level in 0..layout.depth {
                let mut nodes: BTreeMap<usize, Vec<[u8; 32]>> = BTreeMap::new();
                for (index, digest) in children {
                    let node = nodes
                        .entry(index / capacity)
                        .or_insert(view.node(kind, level, index / capacity)?.to_vec());
                    *node.get_mut(index % capacity)? = digest;
                }
                children = BTreeMap::new();
                for (index, values) in nodes {
                    let flat = layout.flat(level, index)?;
                    let page = index_bytes(kind, flat, level, generation, &values)?;
                    children.insert(index, <[u8; 32]>::read_from_bytes(&page[24..56]).ok()?);
                    writes.push((
                        t.offset
                            .get()
                            .checked_add((flat as u64).checked_mul(state_page() as u64)?)?,
                        page,
                    ));
                }
            }
            let i = view.directory.iter().position(|t| t.kind.get() == kind)?;
            let mut descriptor = *t;
            descriptor.checksum = *children.get(&0)?;
            root[184 + i * 64..184 + (i + 1) * 64].copy_from_slice(descriptor.as_bytes());
        }
        let mut header = *view.header;
        header.output = output;
        root[..176].copy_from_slice(header.as_bytes());
        root[176..184].copy_from_slice(U64::new(generation).as_bytes());
        resign(&mut root)?;
        let file = OpenOptions::new().read(true).write(true).open(&self.path).ok()?;
        if file.metadata().ok()?.len() != view.data.len() as u64 {
            return None;
        }
        let next_root = if view.root == 0 { ROOT_BYTES as u64 } else { 0 };
        drop(view);
        drop(mapping);
        for (offset, bytes) in &writes {
            file.write_all_at(bytes, *offset).ok()?;
        }
        file.write_all_at(&root, next_root).ok()?;
        let data_pages = writes
            .iter()
            .filter(|(_, p)| U32::ref_from_bytes(&p[20..24]).is_ok_and(|level| level.get() == 0))
            .count();
        Some((data_pages, writes.len() - data_pages, writes.len() * state_page() + ROOT_BYTES))
    }

    #[cfg(not(unix))]
    fn commit_records(
        &self,
        _changes: Vec<(u32, usize, Vec<u8>)>,
        _output: Identity,
    ) -> Option<(usize, usize, usize)> {
        None
    }

    fn update_micro_state<E: Target>(&mut self, ctx: &Context<E>, plan: &MicroPlan) -> Option<()> {
        let _t = ctx.timer("micro_state_update");
        let view = View::load(self.state.as_deref()?)?;
        if self.changed.as_ref()?.iter().any(|(_, r, p)| Identity::path(p) != Some(r.identity)) {
            return None;
        }
        let mut changes = plan.semantic_updates.clone();
        for (index, record) in &plan.inputs {
            changes.push((INPUTS, *index, record.as_bytes().to_vec()));
        }
        for (index, record) in &plan.objects {
            if plan.selective_objects != 0 {
                changes.push((OBJECTS, *index, record.topology().as_bytes().to_vec()));
            }
            changes.push((OBJECT_REVISIONS, *index, record.source_digest.to_vec()));
        }
        for (index, record) in &plan.sections {
            changes.push((SECTION_REVISIONS, *index, record.revision().as_bytes().to_vec()));
        }
        for index in &plan.dirty_leaves {
            changes.push((LEAVES, *index, plan.leaves[*index].to_vec()));
        }
        for index in &plan.dirty_subtrees {
            changes.push((SUBTREES, *index, plan.subtrees.get(index)?.to_vec()));
        }
        let old = *view.records::<TelemetryDisk>(TELEMETRY)?.first()?;
        if plan.parsed_bytes != 0 && !ctx.args.incremental_verify {
            let elapsed = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            let ewma = |old: U64, value: u64| -> u64 {
                if old.get() == 0 {
                    value
                } else {
                    ((u128::from(old.get()) * 7 + u128::from(value)) / 8).min(u128::from(u64::MAX))
                        as u64
                }
            };
            let read_rate = ewma(old.read_byte_ns, plan.read_ns / plan.read_bytes.max(1));
            let metadata_rate =
                ewma(old.metadata_item_ns, plan.metadata_ns / plan.metadata_items.max(1));
            let hash_rate = ewma(old.hash_byte_ns, plan.hash_ns / plan.hash_bytes.max(1));
            let fixed = ewma(
                old.fixed_ns,
                elapsed
                    .saturating_sub(plan.read_ns)
                    .saturating_sub(plan.metadata_ns)
                    .saturating_sub(plan.hash_ns),
            );
            changes.push((
                TELEMETRY,
                0,
                Telemetry {
                    full_ns: old.full_ns.get(),
                    read_byte_ns: read_rate,
                    metadata_item_ns: metadata_rate,
                    hash_byte_ns: hash_rate,
                    fixed_ns: fixed,
                    samples: old.samples.get().saturating_add(1),
                    last_micro_ns: elapsed,
                }
                .disk()
                .as_bytes()
                .to_vec(),
            ));
        }
        drop(view);
        let (data, index, bytes) = self.commit_records(changes, Identity::path(&self.output)?)?;
        if ctx.args.perf {
            eprintln!(
                "incremental: state_pages={} state_write_bytes={bytes} data_pages={data} index_pages={index} root_pages={} plan_validated_pages={}",
                data + index,
                usize::from(bytes != 0),
                plan.validated_pages
            );
        }
        Some(())
    }
}

#[derive(Default)]
struct IoCounters {
    read: u64,
    write: u64,
    syscw: u64,
    minor: u64,
    major: u64,
}
impl IoCounters {
    fn read() -> Self {
        let mut counters = Self::default();
        if let Ok(io) = std::fs::read_to_string("/proc/self/io") {
            for line in io.lines() {
                if let Some((name, value)) = line.split_once(": ")
                    && let Ok(value) = value.parse()
                {
                    match name {
                        "read_bytes" => counters.read = value,
                        "write_bytes" => counters.write = value,
                        "syscw" => counters.syscw = value,
                        _ => {}
                    }
                }
            }
        }
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat")
            && let Some((_, fields)) = stat.rsplit_once(')')
        {
            let fields: Vec<_> = fields.split_whitespace().collect();
            counters.minor = fields.get(7).and_then(|v| v.parse().ok()).unwrap_or(0);
            counters.major = fields.get(9).and_then(|v| v.parse().ok()).unwrap_or(0);
        }
        counters
    }
}

fn boot() -> Option<[u8; 32]> {
    Some(*blake3::hash(&std::fs::read("/proc/sys/kernel/random/boot_id").ok()?).as_bytes())
}

fn recertify(view: &View<'_>, path: &Path) -> bool {
    let certify = || -> Option<()> {
        if view.inputs.iter().any(|r| Identity::path(view.path(r)) != Some(r.identity)) {
            return None;
        }
        let leaves = view.records::<[u8; 32]>(LEAVES)?;
        let ids = view.records::<BuildIdImage>(BUILD_ID)?;
        if ids.len() != 1
            || leaves.len() as u64 != view.header.output.size.get().div_ceil(SHARD as u64)
        {
            return None;
        }
        let id = ids[0];
        let length = usize::try_from(id.size.get()).ok()?;
        if length == 0
            || length > 32
            || id.offset.get().checked_add(id.size.get())? > view.header.output.size.get()
        {
            return None;
        }
        let mut file = File::open(path).ok()?;
        let identity = Identity::from_metadata(&file.metadata().ok()?)?;
        if identity != view.header.output {
            return None;
        }
        let digest = blake3::hash(leaves.as_flattened());
        let mut descriptor = vec![0; length];
        #[cfg(unix)]
        std::os::unix::fs::FileExt::read_exact_at(&file, &mut descriptor, id.offset.get()).ok()?;
        #[cfg(not(unix))]
        return None;
        if descriptor != digest.as_bytes()[..length] {
            return None;
        }
        let mut block = vec![0; SHARD];
        for (index, expected) in leaves.iter().enumerate() {
            let start = (index * SHARD) as u64;
            let end = (start + SHARD as u64).min(identity.size.get());
            let block = &mut block[..(end - start) as usize];
            file.read_exact(block).ok()?;
            let lo = start.max(id.offset.get());
            let hi = end.min(id.offset.get() + id.size.get());
            if lo < hi {
                block[(lo - start) as usize..(hi - start) as usize].fill(0);
            }
            if blake3::hash(block).as_bytes() != expected {
                return None;
            }
        }
        if Identity::from_metadata(&file.metadata().ok()?) != Some(identity)
            || Identity::path(path) != Some(identity)
        {
            return None;
        }
        Some(())
    };
    certify().is_some()
}

#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
#[repr(C)]
struct TelemetryDisk {
    full_ns: U64,
    read_byte_ns: U64,
    metadata_item_ns: U64,
    hash_byte_ns: U64,
    fixed_ns: U64,
    samples: U64,
    last_micro_ns: U64,
}
struct Telemetry {
    full_ns: u64,
    read_byte_ns: u64,
    metadata_item_ns: u64,
    hash_byte_ns: u64,
    fixed_ns: u64,
    samples: u64,
    last_micro_ns: u64,
}
impl Telemetry {
    fn disk(self) -> TelemetryDisk {
        TelemetryDisk {
            full_ns: self.full_ns.into(),
            read_byte_ns: self.read_byte_ns.into(),
            metadata_item_ns: self.metadata_item_ns.into(),
            hash_byte_ns: self.hash_byte_ns.into(),
            fixed_ns: self.fixed_ns.into(),
            samples: self.samples.into(),
            last_micro_ns: self.last_micro_ns.into(),
        }
    }
}
