//! The export trie in __LINKEDIT: dyld's index of exported symbols.

use crate::macho::arch::Arch;
use crate::macho::context::Context;
use crate::macho::format::*;
use crate::macho::input_files::FileId;
use crate::macho::output_chunks::ChunkHeader;
use crate::macho::symbol::SymbolId;

#[derive(Debug)]
pub struct ExportTrieSection {
    pub hdr: ChunkHeader,
    /// The trie, encoded once when its chunk is sized (every address is
    /// final by then) and reused when copied out.
    pub contents: Vec<u8>,
}

impl ExportTrieSection {
    pub fn new() -> ExportTrieSection {
        ExportTrieSection { hdr: ChunkHeader::linkedit(), contents: Vec::new() }
    }
}

pub fn copy_buf<E: Arch>(ctx: &Context<E>, buf: &mut [u8]) {
    let data = &ctx.export_trie.contents;
    buf[..data.len()].copy_from_slice(data);
}

/// What an export trie terminal says about a symbol.
#[derive(Clone, Copy)]
enum Export {
    /// A symbol defined in this image: flags and image-relative address.
    Addr { flags: u32, addr: u64 },
    /// A symbol re-exported from a dylib under this name (an -alias of
    /// an imported symbol): the dylib's ordinal and the name it has
    /// there.
    Reexport { ordinal: u32, name: &'static str },
}

impl Export {
    /// The terminal's payload after its size: flags, then either the
    /// address or the ordinal and the re-exported name (an empty name
    /// means the same name).
    fn terminal_size(self) -> usize {
        match self {
            Export::Addr { flags, addr } => uleb_len(flags as u64) + uleb_len(addr),
            Export::Reexport { ordinal, name } => {
                uleb_len(EXPORT_SYMBOL_FLAGS_REEXPORT as u64)
                    + uleb_len(ordinal as u64)
                    + name.len()
                    + 1
            }
        }
    }
}

/// A node of the export trie under construction.
#[derive(Default)]
struct TrieNode {
    /// Edges can split UTF-8 code points, so labels borrow bytes rather
    /// than strings from the symbol names.
    children: Vec<(&'static [u8], TrieNode)>,
    /// The exported symbol ending here, if any.
    export: Option<Export>,
    offset: usize,
    /// Pre-order index, assigned by flatten; lets the sizing pass name
    /// a child by index without a pointer hash map.
    index: u32,
    /// Node count of this subtree (including this node), so that
    /// flatten can hand every subtree a disjoint slot range.
    size: u32,
}

/// Builds the subtrie for a sorted run of names that all share their
/// first `depth` bytes. The run splits into children by the byte at
/// `depth`, and each child's edge label is its group's remaining
/// common prefix (for a sorted group, the common prefix of its first
/// and last name). Sibling subtries build in parallel, so
/// construction parallelizes at every branching level - splitting on
/// leading bytes alone is useless when every Mach-O symbol starts
/// with '_'. Construction stays linear in the total name length.
fn build_trie(names: &[(&'static str, Export)], depth: usize) -> TrieNode {
    use rayon::prelude::*;
    let mut node = TrieNode::default();
    let mut rest = names;
    if let Some(&(name, export)) = rest.first() {
        if name.len() == depth {
            node.export = Some(export);
            rest = &rest[1..];
        }
    }
    let mut groups: Vec<&[(&'static str, Export)]> = Vec::new();
    while let Some(&(first, _)) = rest.first() {
        let b = first.as_bytes()[depth];
        let n = rest.iter().take_while(|(n, _)| n.as_bytes()[depth] == b).count();
        groups.push(&rest[..n]);
        rest = &rest[n..];
    }
    let build_child = |group: &&[(&'static str, Export)]| {
        let first = group[0].0;
        let last = group[group.len() - 1].0;
        let common = depth
            + first
                .bytes()
                .skip(depth)
                .zip(last.bytes().skip(depth))
                .take_while(|(a, b)| a == b)
                .count();
        (&first.as_bytes()[depth..common], build_trie(group, common))
    };
    node.children = if names.len() >= 1024 {
        groups.par_iter().map(build_child).collect()
    } else {
        groups.iter().map(build_child).collect()
    };
    node
}

fn uleb_len(mut val: u64) -> usize {
    let mut len = 1;
    while val >= 0x80 {
        val >>= 7;
        len += 1;
    }
    len
}

/// Encodes the export trie: dyld's index of the image's exported
/// symbols. It is a radix tree; each node holds an optional terminal
/// payload (flags and the symbol's image-relative address, both ULEB128)
/// and edges labeled with NUL-terminated string fragments pointing at
/// child nodes by ULEB128 offset within the trie. Since offsets are
/// variable-length, sizing iterates to a fixed point.
pub fn encode_export_trie<E: Arch>(ctx: &Context<E>, sorted_globals: &[SymbolId]) -> Vec<u8> {
    use rayon::prelude::*;
    let base = ctx.args.pagezero_size;

    // The caller hands over the defined globals already sorted by
    // name - the same list the symbol table emits - so the trie only
    // filters the explicit export/unexport lists (order-preserving)
    // and never sorts.
    let exports: Vec<(&'static str, Export)> = sorted_globals
        .par_iter()
        .filter_map(|&id| {
            let sym = &ctx.symbols[id];
            let target = ctx.indirect_aliases.iter().find_map(|&(a, t)| (a == id).then_some(t));
            let same_name = target.is_some_and(|t| ctx.symbols[t].name() == sym.name());
            // Explicit reexports survive restrictions on local exports.
            if !same_name {
                if let Some(exported) = &ctx.args.exported_symbols {
                    if !exported.iter().any(|pat| pat == sym.name()) {
                        return None;
                    }
                }
                if ctx.args.unexported_symbols.iter().any(|pat| pat == sym.name()) {
                    return None;
                }
            }
            if let Some(target) = target {
                let Some(FileId::Dylib(dylib)) = ctx.symbols[target].file() else {
                    return None;
                };
                let ordinal = ctx.bind_ordinal(dylib) as u32;
                let name = if same_name { "" } else { ctx.symbols[target].name() };
                return Some((sym.name(), Export::Reexport { ordinal, name }));
            }
            // The kind bits tell a client linker (and dyld) that the
            // export is a TLV descriptor; ld64 sets them, and a
            // linker reading a stripped dylib's trie has nothing else
            // to go by.
            let mut flags = 0;
            if sym.is_weak_def() {
                flags |= EXPORT_SYMBOL_FLAGS_WEAK_DEFINITION;
            }
            if crate::macho::passes::is_thread_local_sym(ctx, id) {
                flags |= EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL;
            }
            let addr = if ctx.is_absolute_symbol(id) {
                flags |= EXPORT_SYMBOL_FLAGS_KIND_ABSOLUTE;
                ctx.sym_addr(id)
            } else {
                ctx.sym_addr(id) - base
            };
            Some((sym.name(), Export::Addr { flags, addr }))
        })
        .collect();
    if exports.is_empty() {
        return Vec::new();
    }

    let mut root = build_trie(&exports, 0);

    // Nodes in pre-order, as raw pointers to sidestep the borrow of the
    // recursive structure. Two parallel passes in mold's prefix-sum
    // shape: count every subtree, then each subtree writes its
    // pre-order run into its own disjoint slot range of one
    // preallocated array - no appending or copying, and the pre-order
    // index is simply the slot number. Fan-out happens at nodes with
    // many children (the second level: every Mach-O name starts with
    // '_', so the root has one child).
    const FANOUT: usize = 8;
    fn count(node: &mut TrieNode) -> u32 {
        node.children.sort_by(|a, b| a.0.cmp(&b.0));
        let below: u32 = if node.children.len() >= FANOUT {
            node.children.par_iter_mut().map(|(_, c)| count(c)).sum()
        } else {
            node.children.iter_mut().map(|(_, c)| count(c)).sum()
        };
        node.size = 1 + below;
        node.size
    }
    struct Slots(*mut *mut TrieNode);
    unsafe impl Sync for Slots {}
    fn fill(node: &mut TrieNode, base: u32, slots: &Slots) {
        node.index = base;
        // SAFETY: every subtree owns [base, base+size), the ranges are
        // disjoint by construction of the prefix sums below, and the
        // array holds exactly root.size slots.
        unsafe { *slots.0.add(base as usize) = node };
        if node.children.len() >= FANOUT {
            let mut b = base + 1;
            let bases: Vec<u32> = node
                .children
                .iter()
                .map(|(_, c)| {
                    let x = b;
                    b += c.size;
                    x
                })
                .collect();
            node.children.par_iter_mut().zip(bases).for_each(|((_, c), cb)| fill(c, cb, slots));
        } else {
            let mut b = base + 1;
            for (_, c) in &mut node.children {
                fill(c, b, slots);
                b += c.size;
            }
        }
    }
    let total = count(&mut root) as usize;
    let mut nodes: Vec<*mut TrieNode> = vec![std::ptr::null_mut(); total];
    fill(&mut root, 0, &Slots(nodes.as_mut_ptr()));
    debug_assert!(nodes.iter().all(|p| !p.is_null()));

    // Assign node offsets until they stop moving. Everything except
    // the width of the child-offset ULEBs is invariant, so the
    // fixpoint (a couple of passes: offsets only grow as their ULEBs
    // widen) runs over precomputed per-node fixed sizes and child
    // index lists, no pointer chasing.
    // Each node's fixed size and the pre-order indices of its children.
    // flatten stamped every node's index, so a child names itself by
    // index with no pointer hash map, and the whole pass is a pure
    // per-node map that runs in parallel.
    struct NodePtr(*mut TrieNode);
    unsafe impl Sync for NodePtr {}
    let node_ptrs: Vec<NodePtr> = nodes.iter().map(|&p| NodePtr(p)).collect();
    let (fixed, kids): (Vec<usize>, Vec<Vec<u32>>) = node_ptrs
        .par_iter()
        .map(|np| {
            // SAFETY: nodes live in `root`, which outlives this function.
            let node = unsafe { &*np.0 };
            let terminal_size = match node.export {
                Some(export) => export.terminal_size(),
                None => 0,
            };
            let mut f = uleb_len(terminal_size as u64) + terminal_size + 1;
            let mut k = Vec::with_capacity(node.children.len());
            for (label, child) in &node.children {
                f += label.len() + 1;
                k.push(child.index);
            }
            (f, k)
        })
        .unzip();
    let mut offs = vec![0u32; nodes.len()];
    // Total encoded size, set on every pass (the loop always runs).
    let mut total;
    loop {
        let mut changed = false;
        let mut off = 0u32;
        for i in 0..nodes.len() {
            if offs[i] != off {
                offs[i] = off;
                changed = true;
            }
            off += fixed[i] as u32;
            for &c in &kids[i] {
                off += uleb_len(offs[c as usize] as u64) as u32;
            }
        }
        total = off;
        if !changed {
            break;
        }
    }
    for (i, &node) in nodes.iter().enumerate() {
        // SAFETY: as above; each node written once.
        unsafe { (*node).offset = offs[i] as usize };
    }

    // Emit every node into its final slot in parallel. Node i owns the
    // byte range [offs[i], offs[i+1]) (the last runs to `total`), the
    // ranges are disjoint and cover the buffer, and each node reads
    // only its children's offsets (already final) - so all writes are
    // independent. On a big Rust debug link the trie is tens of MB, so
    // this is the difference between a serial and a parallel memcpy.
    fn write_uleb_at(dst: &mut [u8], mut pos: usize, mut val: u64) -> usize {
        let start = pos;
        loop {
            let mut b = (val & 0x7f) as u8;
            val >>= 7;
            if val != 0 {
                b |= 0x80;
            }
            dst[pos] = b;
            pos += 1;
            if val == 0 {
                break;
            }
        }
        pos - start
    }
    let mut buf = vec![0u8; total as usize];
    {
        struct BufPtr(*mut u8);
        unsafe impl Sync for BufPtr {}
        let bp = BufPtr(buf.as_mut_ptr());
        let bp = &bp;
        let n = nodes.len();
        node_ptrs.par_iter().enumerate().for_each(|(i, np)| {
            let node = unsafe { &*np.0 };
            let start = offs[i] as usize;
            let end = if i + 1 < n { offs[i + 1] as usize } else { total as usize };
            // SAFETY: the [start, end) ranges are disjoint across nodes
            // and lie within the allocation of length `total`.
            let dst = unsafe { std::slice::from_raw_parts_mut(bp.0.add(start), end - start) };
            let mut p = 0;
            match node.export {
                Some(export @ Export::Addr { flags, addr }) => {
                    p += write_uleb_at(dst, p, export.terminal_size() as u64);
                    p += write_uleb_at(dst, p, flags as u64);
                    p += write_uleb_at(dst, p, addr);
                }
                Some(export @ Export::Reexport { ordinal, name }) => {
                    p += write_uleb_at(dst, p, export.terminal_size() as u64);
                    p += write_uleb_at(dst, p, EXPORT_SYMBOL_FLAGS_REEXPORT as u64);
                    p += write_uleb_at(dst, p, ordinal as u64);
                    dst[p..p + name.len()].copy_from_slice(name.as_bytes());
                    p += name.len();
                    dst[p] = 0;
                    p += 1;
                }
                None => {
                    dst[p] = 0;
                    p += 1;
                }
            }
            dst[p] = node.children.len() as u8;
            p += 1;
            for (label, child) in &node.children {
                dst[p..p + label.len()].copy_from_slice(label);
                p += label.len();
                dst[p] = 0;
                p += 1;
                p += write_uleb_at(dst, p, child.offset as u64);
            }
            debug_assert_eq!(p, end - start);
        });
    }
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    buf
}
