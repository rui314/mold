//! This file creates the export trie, the table in __LINKEDIT through which
//! dyld looks up the symbols an image exports when it binds other images'
//! references to them. It does the job of .dynsym and the hash table
//! (.gnu.hash) on ELF. The trie is a prefix tree of the exported symbols'
//! names: each edge is labeled with a piece of a name, and the node where a
//! name ends gives the symbol's address, relative to the image, and flags,
//! such as whether it is a weak definition or thread-local, or, for a
//! symbol re-exported from another library, the library and the name the
//! symbol has there.

use mold_common::leb128::{uleb_size, write_uleb};
use rayon::prelude::*;

use crate::arch::Target;
use crate::chunks::ChunkHeader;
use crate::context::Context;
use crate::input_files::FileId;
use crate::macho::*;
use crate::symbol::SymbolId;

#[derive(Debug)]
pub struct ExportTrieSection {
    pub hdr: ChunkHeader,
    /// The trie, encoded once when its chunk is sized (every address is
    /// final by then) and reused when copied out.
    pub contents: Vec<u8>,
}

impl ExportTrieSection {
    pub fn new() -> Self {
        let mut hdr = ChunkHeader::linkedit();
        hdr.p2align = 3;
        Self { hdr, contents: Vec::new() }
    }
}

impl Default for ExportTrieSection {
    fn default() -> Self {
        Self::new()
    }
}

/// What an export trie terminal says about a symbol.
#[derive(Clone, Copy)]
enum Export {
    /// A symbol defined in this image: flags and image-relative address.
    Addr { flags: u32, addr: u64 },
    /// A symbol re-exported from a dylib under this name (an -alias of
    /// an imported symbol): the dylib's ordinal and the name it has
    /// there.
    Reexport { ordinal: u32, name: &'static [u8] },
}

impl Export {
    /// The terminal's payload after its size: flags, then either the
    /// address or the ordinal and the re-exported name (an empty name
    /// means the same name).
    fn terminal_size(self) -> usize {
        match self {
            Self::Addr { flags, addr } => uleb_size(flags as u64) + uleb_size(addr),
            Self::Reexport { ordinal, name } => {
                uleb_size(EXPORT_SYMBOL_FLAGS_REEXPORT as u64)
                    + uleb_size(ordinal as u64)
                    + name.len()
                    + 1
            }
        }
    }
}

/// A node of the export trie under construction.
#[derive(Default)]
struct TrieNode {
    /// The edges, labeled with slices of the symbol names.
    children: Vec<(&'static [u8], Self)>,
    /// The exported symbol ending here, if any.
    export: Option<Export>,
    /// Pre-order index, assigned by number_nodes; lets the later passes
    /// name a child by index without a pointer hash map.
    index: u32,
    /// Node count of this subtree (including this node), so that
    /// number_nodes can hand every subtree a disjoint slot range.
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
fn build_trie(names: &[(&'static [u8], Export)], depth: usize) -> TrieNode {
    let mut node = TrieNode::default();
    let mut rest = names;
    if let Some(&(name, export)) = rest.first()
        && name.len() == depth
    {
        node.export = Some(export);
        rest = &rest[1..];
    }
    let mut groups: Vec<&[(&'static [u8], Export)]> = Vec::new();
    while let Some(&(first, _)) = rest.first() {
        let b = first[depth];
        let n = rest.iter().take_while(|(n, _)| n[depth] == b).count();
        groups.push(&rest[..n]);
        rest = &rest[n..];
    }
    let build_child = |group: &&[(&'static [u8], Export)]| {
        let first = group[0].0;
        let last = group[group.len() - 1].0;
        let common =
            depth + first[depth..].iter().zip(&last[depth..]).take_while(|(a, b)| a == b).count();
        (&first[depth..common], build_trie(group, common))
    };
    node.children = if names.len() >= 1024 {
        groups.par_iter().map(build_child).collect()
    } else {
        groups.iter().map(build_child).collect()
    };
    node
}

/// Encodes the export trie: dyld's index of the image's exported
/// symbols. It is a radix tree; each node holds an optional terminal
/// payload (flags and the symbol's image-relative address, both ULEB128)
/// and edges labeled with NUL-terminated string fragments pointing at
/// child nodes by ULEB128 offset within the trie. The nodes are laid out
/// in ld-prime's order (see place_nodes), then the trie is padded to 8
/// bytes.
pub fn encode_export_trie<E: Target>(ctx: &Context<E>, sorted_globals: &[SymbolId]) -> Vec<u8> {
    // An image no dyld loads has no one to look its symbols up:
    // ld-prime writes no trie at all, even with the dyld info
    // -no_fixup_chains gives a -static one.
    if ctx.args.without_dyld() {
        return Vec::new();
    }
    let exports = exports(ctx, sorted_globals);
    // Nothing exported: an empty root node (terminal size 0, no
    // children), padded to 8 bytes as ld-prime writes it.
    if exports.is_empty() {
        return vec![0; 8];
    }

    let mut root = build_trie(&exports, 0);
    let nodes = number_nodes(&mut root);
    let placement = place_nodes(&nodes);
    let mut buf = write_nodes(&nodes, &placement);
    buf.resize(buf.len().next_multiple_of(8), 0);
    buf
}

/// The trie's entries, in name order. The caller hands over the
/// defined globals already sorted by name - the same list the symbol
/// table emits, with what the export lists leave out already made
/// private extern - so the trie never sorts.
fn exports<E: Target>(
    ctx: &Context<E>,
    sorted_globals: &[SymbolId],
) -> Vec<(&'static [u8], Export)> {
    let base = ctx.mach_header.hdr.addr;
    sorted_globals
        .par_iter()
        .filter_map(|&id| {
            let sym = &ctx.symbols[id];
            let target = ctx.indirect_aliases.iter().find_map(|&(a, t)| (a == id).then_some(t));
            if let Some(target) = target {
                let same_name = ctx.symbols[target].name() == sym.name();
                let Some(FileId::Dylib(_)) = ctx.symbols[target].file() else {
                    return None;
                };
                let ordinal = ctx.symbols[target].bind_ordinal(ctx) as u32;
                let name = if same_name { b"" } else { ctx.symbols[target].name() };
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
            if sym.is_tlv(ctx) {
                flags |= EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL;
            }
            let addr = if sym.is_absolute(ctx) {
                flags |= EXPORT_SYMBOL_FLAGS_KIND_ABSOLUTE;
                sym.addr(ctx)
            } else {
                sym.addr(ctx) - base
            };
            Some((sym.name(), Export::Addr { flags, addr }))
        })
        .collect()
}

/// Numbers the trie's nodes in pre-order (TrieNode::index), sorting each
/// node's children by label, and returns them in that order. Two
/// parallel passes in mold's prefix-sum shape: count every subtree, then
/// each subtree stores its pre-order run into its own disjoint slot
/// range of one preallocated array - no appending or copying, and the
/// pre-order index is simply the slot number. Fan-out happens at nodes
/// with many children (the second level: every Mach-O name starts with
/// '_', so the root has one child).
fn number_nodes(root: &mut TrieNode) -> Vec<&TrieNode> {
    const FANOUT: usize = 8;
    fn count(node: &mut TrieNode) -> u32 {
        node.children.sort_by(|a, b| a.0.cmp(b.0));
        let below: u32 = if node.children.len() >= FANOUT {
            node.children.par_iter_mut().map(|(_, c)| count(c)).sum()
        } else {
            node.children.iter_mut().map(|(_, c)| count(c)).sum()
        };
        node.size = 1 + below;
        node.size
    }
    // The nodes as raw pointers until every index is stamped, which
    // borrows them mutably.
    struct Slots(*mut *const TrieNode);
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
    let total = count(root) as usize;
    let mut nodes: Vec<*const TrieNode> = vec![std::ptr::null(); total];
    fill(root, 0, &Slots(nodes.as_mut_ptr()));
    // SAFETY: fill stored a pointer to every node of `root`, which
    // stays borrowed, and unchanged, as long as the references live.
    nodes.into_iter().map(|p| unsafe { &*p }).collect()
}

/// Where place_nodes puts the nodes: each one's offset and size, by
/// pre-order index, the nodes in file order, and the trie's size.
struct Placement {
    offs: Vec<u32>,
    sizes: Vec<u32>,
    order: Vec<u32>,
    total: u32,
}

/// Places the trie's nodes, given in pre-order, as ld-prime lays them
/// out. The root comes first, with room for each child offset at its
/// widest (5 bytes, a u32's ULEB128) since it is written before its
/// children are placed; the unused bytes stay zero after its last edge.
/// The other nodes follow in post-order, a node after its subtrees, so
/// its children's offsets and thus its own size are known when it is
/// placed.
fn place_nodes(nodes: &[&TrieNode]) -> Placement {
    // Each node's size apart from its child-offset ULEBs, and the
    // pre-order indices of its children: a pure per-node map.
    let (fixed, kids): (Vec<usize>, Vec<Vec<u32>>) = nodes
        .par_iter()
        .map(|node| {
            let terminal_size = node.export.map_or(0, Export::terminal_size);
            let mut f = uleb_size(terminal_size as u64) + terminal_size + 1;
            let mut k = Vec::with_capacity(node.children.len());
            for (label, child) in &node.children {
                f += label.len() + 1;
                k.push(child.index);
            }
            (f, k)
        })
        .unzip();

    let mut offs = vec![0u32; nodes.len()];
    let mut sizes = vec![0u32; nodes.len()];
    let mut order = Vec::with_capacity(nodes.len());
    sizes[0] = (fixed[0] + 5 * kids[0].len()) as u32;
    order.push(0);
    let mut off = sizes[0];
    // An explicit stack of (node, next child to visit): tries of long
    // mangled names nest deeply.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for &top in &kids[0] {
        stack.push((top as usize, 0));
        while let Some(&(node, next)) = stack.last() {
            if let Some(&child) = kids[node].get(next) {
                stack.last_mut().unwrap().1 += 1;
                stack.push((child as usize, 0));
                continue;
            }
            stack.pop();
            let edges: usize = kids[node].iter().map(|&c| uleb_size(offs[c as usize] as u64)).sum();
            offs[node] = off;
            sizes[node] = (fixed[node] + edges) as u32;
            order.push(node as u32);
            off += sizes[node];
        }
    }
    Placement { offs, sizes, order, total: off }
}

/// Writes the placed nodes, each on a core of its own into its slice of
/// the trie: on a big Rust debug link the trie is tens of MB.
fn write_nodes(nodes: &[&TrieNode], placement: &Placement) -> Vec<u8> {
    let mut buf = vec![0u8; placement.total as usize];
    // The nodes' slices, cut off one after another in file order.
    let mut rest = buf.as_mut_slice();
    let slices: Vec<(&TrieNode, &mut [u8])> = (placement.order.iter())
        .map(|&i| {
            let size = placement.sizes[i as usize] as usize;
            (nodes[i as usize], rest.split_off_mut(..size).unwrap())
        })
        .collect();
    slices.into_par_iter().for_each(|(node, dst)| write_node(node, &placement.offs, dst));
    buf
}

/// Writes a node: its terminal - its size, then the export's flags and
/// address, or a re-export's flags, ordinal and name - or a 0, then the
/// count of its edges, each a NUL-terminated label and the child's
/// offset in `offs`.
fn write_node(node: &TrieNode, offs: &[u32], dst: &mut [u8]) {
    let mut p = 0;
    match node.export {
        Some(export @ Export::Addr { flags, addr }) => {
            p += write_uleb(&mut dst[p..], export.terminal_size() as u64);
            p += write_uleb(&mut dst[p..], flags as u64);
            p += write_uleb(&mut dst[p..], addr);
        }
        Some(export @ Export::Reexport { ordinal, name }) => {
            p += write_uleb(&mut dst[p..], export.terminal_size() as u64);
            p += write_uleb(&mut dst[p..], EXPORT_SYMBOL_FLAGS_REEXPORT as u64);
            p += write_uleb(&mut dst[p..], ordinal as u64);
            dst[p..p + name.len()].copy_from_slice(name);
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
        p += write_uleb(&mut dst[p..], offs[child.index as usize] as u64);
    }
    // The root's unused reserved offset bytes stay zero.
    debug_assert!(if node.index == 0 { p <= dst.len() } else { p == dst.len() });
}
