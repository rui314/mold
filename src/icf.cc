// This file implements the Identical Code Folding feature which can
// reduce the output file size of a typical program by a few percent.
// ICF identifies read-only input sections that happen to be identical
// and thus can be used interchangeably. ICF leaves one of them and discards
// the others.
//
// ICF is usually used in combination with -ffunction-sections and
// -fdata-sections compiler options, so that object files have one section
// for each function or variable instead of having one large .text or .data.
// The unit of ICF merging is section.
//
// Two sections are considered identical by ICF if they have the exact
// same contents, metadata such as section flags, exception handling
// records, and relocations. The last one is interesting because two
// relocations are considered identical if they point to the _same_
// section in terms of ICF.
//
// To see what that means, consider two sections, A and B, which are
// identical except for one pair of relocations. Say, A has a relocation to
// section C, and B has a relocation to D. In this case, A and B are
// considered identical if C and D are considered identical. C and D can be
// either really the same section or two different sections that are
// considered identical by ICF. Below is an example of such inputs, A, B, C
// and D:
//
//   void A() { C(); }
//   void B() { D(); }
//   void C() { A(); }
//   void D() { B(); }
//
// If we assume A and B are mergeable, we can merge C and D, which makes A
// and B mergeable. There's no contradiction in our assumption, so we can
// conclude that A and B as well as C and D are mergeable.
//
// This problem boils down to one in graph theory. Input to ICF can be
// considered as a directed graph in which vertices are sections and edges
// are relocations. Vertices have labels (section contents, etc.), and so
// are edges (relocation offsets, etc.). Two vertices are considered
// identical if and only if the (possibly infinite) their unfoldings into
// regular trees are equal. Given this formulation, we want to find as
// many identical vertices as possible.
//
// Just like a lot of problems with graph, this problem doesn't have a
// straightforward "optimal" solution, and we need to resort to heuristics.
//
// mold approaches this problem by hashing program trees with increasing depth
// on each iteration.
// For example, when we start, we only hash individual functions with
// their call into other functions omitted. From the second iteration, we
// put the function they call into the hash by appending the hash of those
// functions from the previous iteration. This means that the nth iteration
// hashes call chain up to (n-1) levels deep.
// We use a cryptographic hash function, so the unique number of hashes will
// only monotonically increase as we take into account of deeper trees with
// iterations (otherwise, that means we have found a hash collision). We stop
// when the unique number of hashes stop increasing; this is based on the fact
// that once we observe an iteration with the same amount of unique hashes as
// the previous iteration, it will remain unchanged for further iterations.
// This is provable, but here we omit the proof for brevity.
//
// When compared to other approaches, mold's approach has a relatively cheaper
// cost per iteration, and as a bonus, is highly parallelizable.
// For Chromium, mold's ICF finishes in less than 1 second with 20 threads,
// whereas lld takes 5 seconds and gold takes 50 seconds under the same
// conditions.

#include "mold.h"
#include "../lib/siphash.h"

#include <fstream>
#include <tbb/concurrent_unordered_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

namespace mold {

struct Digest {
  u64 hi;
  u64 lo;
};

static u8 siphash_key[16];

template <typename E>
static void uniquify_cies(Context<E> &ctx) {
  Timer t(ctx, "uniquify_cies");
  std::vector<CieRecord<E> *> cies;

  auto find = [&](CieRecord<E> &cie) -> i64 {
    for (i64 i = 0; i < cies.size(); i++)
      if (cie_equals(cie, *cies[i]))
        return i;
    return -1;
  };

  for (ObjectFile<E> *file : ctx.objs) {
    for (CieRecord<E> &cie : file->cies) {
      if (i64 idx = find(cie); idx != -1) {
        cie.icf_idx = idx;
      } else {
        cie.icf_idx = cies.size();
        cies.push_back(&cie);
      }
    }
  }
}

template <typename E>
static bool is_eligible(Context<E> &ctx, InputSection<E> &isec) {
  const ElfShdr<E> &shdr = isec.shdr();

  if (shdr.sh_size == 0 || !(shdr.sh_flags & SHF_ALLOC) ||
      shdr.sh_type == SHT_NOBITS || is_c_identifier(isec.name))
    return false;

  if (shdr.sh_flags & SHF_EXECINSTR)
    return (ctx.arg.icf_all || !isec.address_taken) &&
           isec.name != ".init" && isec.name != ".fini";

  // .gcc_except_table contains a compiler-generated table. Pointer
  // equality for the section is not significant because only the C++
  // exception handling code will use the table at runtime.
  if (isec.name == ".gcc_except_table" ||
      isec.name.starts_with(".gcc_except_table."))
    return true;

  bool is_readonly = !(shdr.sh_flags & SHF_WRITE);
  bool is_relro = isec.name.starts_with(".data.rel.ro");
  return (ctx.arg.ignore_data_address_equality || !isec.address_taken) &&
         (is_readonly || is_relro);
}

template <typename E>
static Digest compute_digest(Context<E> &ctx, InputSection<E> &isec) {
  SipHash13_128 hasher(siphash_key);

  auto hash = [&](auto val) {
    hasher.update((u8 *)&val, sizeof(val));
  };

  auto hash_string = [&](std::string_view str) {
    hash(str.size());
    hasher.update((u8 *)str.data(), str.size());
  };

  auto hash_symbol = [&](Symbol<E> &sym) {
    InputSection<E> *isec = sym.get_input_section();

    if (!sym.file || sym.is_imported) {
      hash('1');
      hash((u64)&sym);
    } else if (SectionFragment<E> *frag = sym.get_frag()) {
      hash('2');
      hash((u64)frag);
    } else if (!isec) {
      hash('3');
    } else if (isec->icf_eligible) {
      hash('4');
    } else {
      hash('5');
      hash((u64)isec);
    }
    hash(sym.value);
  };

  hash_string(isec.contents);
  hash(isec.shdr().sh_flags);
  hash(isec.get_fdes().size());
  hash(isec.get_rels(ctx).size());

  for (FdeRecord<E> &fde : isec.get_fdes()) {
    hash(isec.file.cies[fde.cie_idx].icf_idx);

    // Bytes 0 to 4 contain the length of this record, and
    // bytes 4 to 8 contain an offset to CIE.
    hash_string(fde.get_contents(isec.file).substr(8));

    hash(fde.get_rels(isec.file).size());

    for (const ElfRel<E> &rel : fde.get_rels(isec.file).subspan(1)) {
      hash_symbol(*isec.file.symbols[rel.r_sym]);
      hash(rel.r_type);
      hash(rel.r_offset - fde.input_offset);
      hash(get_addend(isec.file.cies[fde.cie_idx].input_section, rel));
    }
  }

  for (const ElfRel<E> &rel : isec.get_rels(ctx)) {
    hash(rel.r_offset);
    hash(rel.r_type);
    hash(get_addend(isec, rel));
    hash_symbol(*isec.file.symbols[rel.r_sym]);
  }

  Digest digest;
  hasher.finish(&digest);
  return digest;
}

// A concurrent hash map from digest to section. We use it to count the
// number of distinct digests and to elect the leader section of each
// digest's equivalence class.
//
// The number of distinct digests cannot exceed the number of digests,
// so we can allocate a large enough table upfront and never need to
// grow it.
//
// The map is reused across propagation rounds. Instead of clearing the
// table at the start of each round, we stamp each slot with the round
// number in which it was written; a slot stamped with an earlier round
// is treated as vacant.
//
// Each slot consists of two 64-bit words and a section pointer. The
// first word packs the round number, a busy bit, and 48 bits of the
// digest; the second word holds another 64 bits. An inserter claims a
// vacant slot by installing the first word with compare-and-swap with
// the busy bit set, writes the second word and the pointer, and then
// rewrites the first word with the busy bit cleared to publish the
// slot. Since the slot index is derived from digest bits that the
// first word doesn't contain, a successful match effectively compares
// an entire 128-bit digest, so the map is exact under the same
// hash-collision assumption the surrounding algorithm is built on.
//
// Of all sections inserted with the same digest in the same round, the
// slot ends up pointing to the one with the lowest priority, which ICF
// uses as the leader of the digest's equivalence class.
template <typename E>
class DigestMap {
public:
  DigestMap(i64 n) :
    mask(bit_ceil(n * 2) - 1),
    slots(mask + 1) {}

  void next_round() {
    if (++round == 1 << 15) {
      // The round number wrapped around, making stale slot stamps
      // ambiguous, so reset the table. In practice, ICF converges long
      // before this point.
      for (Slot &slot : slots)
        slot.hi = 0;
      round = 1;
    }
  }

  // Returns true if the digest was not in the table.
  bool insert(const Digest &digest, InputSection<E> *isec) {
    constexpr u64 busy_bit = 1LL << 48;
    u64 tag = digest.hi >> 16;
    u64 value = (round << 49) | tag;

    for (i64 i = digest.hi & mask;; i = (i + 1) & mask) {
      Slot &slot = slots[i];
      u64 x = slot.hi.load(std::memory_order_acquire);

      // If the slot was last written in an earlier round, it's vacant;
      // try to claim it.
      while ((x >> 49) != round) {
        if (slot.hi.compare_exchange_weak(x, value | busy_bit,
                                          std::memory_order_acquire)) {
          slot.lo = digest.lo;
          slot.leader = isec;
          slot.hi.store(value, std::memory_order_release);
          return true;
        }
      }

      // The slot is occupied. If it holds a different digest, try the
      // next slot.
      if ((x & 0xffff'ffff'ffff) != tag)
        continue;

      // The tags match; compare the digest bits in the second word,
      // waiting for the writer to publish them if the slot is still
      // being claimed.
      while (x & busy_bit)
        x = slot.hi.load(std::memory_order_acquire);
      if (slot.lo != digest.lo)
        continue;

      // The digest is already in the table; keep the slot pointing to
      // the lowest-priority section.
      InputSection<E> *cur = slot.leader;
      while (isec->get_priority() < cur->get_priority() &&
             !slot.leader.compare_exchange_strong(cur, isec));
      return false;
    }
  }

  // Returns the section associated with a digest. The digest must have
  // been inserted in the current round, and no insertion may be running
  // concurrently.
  InputSection<E> *find(const Digest &digest) {
    u64 value = (round << 49) | (digest.hi >> 16);
    for (i64 i = digest.hi & mask;; i = (i + 1) & mask)
      if (slots[i].hi == value && slots[i].lo == digest.lo)
        return slots[i].leader;
  }

private:
  struct Slot {
    Atomic<u64> hi;
    Atomic<u64> lo;
    Atomic<InputSection<E> *> leader;
  };

  // Time begins in round 1 so that all-zero slots, the initial state
  // of the table, read as vacant.
  u64 round = 1;
  u64 mask;
  std::vector<Slot> slots;
};

template <typename E>
static std::vector<InputSection<E> *> gather_sections(Context<E> &ctx) {
  Timer t(ctx, "gather_sections");

  static Counter eligible("icf_eligibles");
  static Counter non_eligible("icf_non_eligibles");

  // Count the number of eligible input sections for each input file
  // and turn the counts into starting indices with a prefix sum.
  std::vector<i64> indices(ctx.objs.size() + 1);

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections) {
      if (!isec || !isec->is_alive)
        continue;

      if (is_eligible(ctx, *isec)) {
        eligible++;
        isec->icf_eligible = true;
        indices[i + 1]++;
      } else {
        non_eligible++;
      }
    }
  });

  for (i64 i = 1; i < indices.size(); i++)
    indices[i] += indices[i - 1];

  std::vector<InputSection<E> *> sections(indices.back());

  // Fill `sections` contents.
  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    i64 idx = indices[i];
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections) {
      if (isec && isec->is_alive && isec->icf_eligible) {
        isec->icf_idx = idx;
        sections[idx++] = isec.get();
      }
    }
  });

  return sections;
}

template <typename E>
static std::vector<Digest>
compute_digests(Context<E> &ctx, std::span<InputSection<E> *> sections) {
  Timer t(ctx, "compute_digests");

  std::vector<Digest> digests(sections.size());
  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    digests[i] = compute_digest(ctx, *sections[i]);
  });
  return digests;
}

// Build a graph, treating every function as a vertex and every function call
// as an edge. See the description at the top for a more detailed formulation.
// We use u32 indices here to improve cache locality.
//
// Relocations in a section's FDEs are edges too, because compute_digest
// hashes eligible relocation targets without identity, and every such
// target must be represented as an edge to remain distinguishable. In
// particular, an FDE's reference to an LSDA is an edge; without it, two
// identical functions whose exception tables catch different types would
// be folded into one.
template <typename E>
static void gather_edges(Context<E> &ctx,
                         std::span<InputSection<E> *> sections,
                         std::vector<u32> &edges,
                         std::vector<u32> &edge_indices) {
  Timer t(ctx, "gather_edges");

  // Count the number of outgoing edges for each vertex and turn the
  // counts into starting indices with a prefix sum. The extra entry at
  // the end makes edge_indices[i + 1] valid for every vertex, so that
  // vertex i's edges are edge_indices[i] to edge_indices[i + 1].
  edge_indices.resize(sections.size() + 1);

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    InputSection<E> &isec = *sections[i];
    assert(isec.icf_eligible);

    for (FdeRecord<E> &fde : isec.get_fdes())
      for (const ElfRel<E> &rel : fde.get_rels(isec.file).subspan(1))
        if (Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
            InputSection<E> *isec = sym.get_input_section())
          if (isec->icf_eligible)
            edge_indices[i + 1]++;

    for (const ElfRel<E> &rel : isec.get_rels(ctx))
      if (Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
          InputSection<E> *isec = sym.get_input_section())
        if (isec->icf_eligible)
          edge_indices[i + 1]++;
  });

  for (i64 i = 1; i < edge_indices.size(); i++)
    edge_indices[i] += edge_indices[i - 1];

  edges.resize(edge_indices.back());

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    InputSection<E> &isec = *sections[i];
    i64 idx = edge_indices[i];

    for (FdeRecord<E> &fde : isec.get_fdes())
      for (const ElfRel<E> &rel : fde.get_rels(isec.file).subspan(1))
        if (Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
            InputSection<E> *isec = sym.get_input_section())
          if (isec->icf_eligible)
            edges[idx++] = isec->icf_idx;

    for (const ElfRel<E> &rel : isec.get_rels(ctx))
      if (Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
          InputSection<E> *isec = sym.get_input_section())
        if (isec->icf_eligible)
          edges[idx++] = isec->icf_idx;
  });
}

// Compute the next-round digest of each vertex by hashing its current
// digest and the current digests of the vertices it refers to. A
// vertex's digest after the nth round is therefore a hash of its
// unfolding into a tree of depth n.
static void propagate(std::span<Digest> cur, std::span<Digest> next,
                      std::span<u32> edges, std::span<u32> edge_indices) {
  tbb::parallel_for((i64)0, (i64)cur.size(), [&](i64 i) {
    SipHash13_128 hasher(siphash_key);
    hasher.update(&cur[i], sizeof(Digest));

    i64 begin = edge_indices[i];
    i64 end = edge_indices[i + 1];
    for (i64 j : edges.subspan(begin, end - begin))
      hasher.update(&cur[j], sizeof(Digest));

    hasher.finish(&next[i]);
  });

  static Counter counter("icf_round");
  counter++;
}

template <typename E>
static i64 count_num_classes(std::span<Digest> digests,
                             std::span<InputSection<E> *> sections,
                             DigestMap<E> &map) {
  map.next_round();

  tbb::enumerable_thread_specific<i64> num_classes;
  tbb::parallel_for((i64)0, (i64)digests.size(), [&](i64 i) {
    if (map.insert(digests[i], sections[i]))
      num_classes.local()++;
  });
  return num_classes.combine(std::plus());
}

template <typename E>
static void print_icf_sections(Context<E> &ctx) {
  tbb::concurrent_vector<InputSection<E> *> leaders;
  tbb::concurrent_unordered_multimap<InputSection<E> *, InputSection<E> *> map;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
      if (isec && isec->is_alive && isec->leader) {
        if (isec.get() == isec->leader)
          leaders.push_back(isec.get());
        else
          map.insert({isec->leader, isec.get()});
      }
    }
  });

  tbb::parallel_sort(leaders.begin(), leaders.end(),
                     [](InputSection<E> *a, InputSection<E> *b) {
                       return a->get_priority() < b->get_priority();
                     });

  std::ostream *out = &std::cout;
  std::ofstream file;

  std::string &path = ctx.arg.print_icf_sections;
  if (path != "-") {
    file.open(path);
    if (file.fail())
      Fatal(ctx) << "--print-icf-sections: cannot open " << path << ": "
                 << errno_string();
    out = &file;
  }

  i64 saved_bytes = 0;

  for (InputSection<E> *leader : leaders) {
    auto [begin, end] = map.equal_range(leader);
    if (begin != end) {
      *out << "selected section " << *leader << '\n';
      for (auto it = begin; it != end; it++) {
        *out << "  removing identical section " << *it->second << '\n';
        saved_bytes += leader->contents.size();
      }
    }
  }

  *out << "ICF saved " << saved_bytes << " bytes\n";
}

template <typename E>
void icf_sections(Context<E> &ctx) {
  Timer t(ctx, "icf");
  if (ctx.objs.empty())
    return;

  get_random_bytes(siphash_key, sizeof(siphash_key));

  uniquify_cies(ctx);

  // Prepare for the propagation rounds.
  std::vector<InputSection<E> *> sections = gather_sections(ctx);

  // `digests` holds the current digest of each vertex; `scratch` is
  // where a propagation round writes the next digests before the two
  // vectors swap roles.
  std::vector<Digest> digests = compute_digests<E>(ctx, sections);
  std::vector<Digest> scratch(digests.size());

  std::vector<u32> edges;
  std::vector<u32> edge_indices;
  gather_edges<E>(ctx, sections, edges, edge_indices);

  // The digest map is used to count the number of distinct digests in
  // the loop below. As a side effect, it records the lowest-priority
  // section for each digest, which the grouping step after the loop
  // uses as the leader of each equivalence class.
  DigestMap<E> map(digests.size());

  // Execute the propagation rounds until convergence is obtained.
  //
  // The number of distinct digests can only monotonically increase as
  // the rounds hash ever-deeper trees, so once two consecutive rounds
  // yield the same count, the partition of sections into equivalence
  // classes has stopped changing and will remain unchanged for further
  // iterations (proof omitted for brevity). Note that individual
  // digests may well still be changing at that point; sections that
  // have a cycle in downstream (i.e. recursive functions and functions
  // that call them) never settle on a digest. That doesn't matter
  // because sections in the same class change their digests in
  // lockstep, keeping the partition intact.
  {
    Timer t(ctx, "propagate");
    i64 num_classes = -1;

    for (;;) {
      propagate(digests, scratch, edges, edge_indices);
      std::swap(digests, scratch);
      i64 m = count_num_classes<E>(digests, sections, map);
      if (m == num_classes)
        break;
      num_classes = m;
    }
  }

  // Group sections by digest. The final counting round has already
  // elected a leader for each digest; look it up.
  {
    Timer t(ctx, "group");
    tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
      sections[i]->leader = map.find(digests[i]);
    });
  }

  if (!ctx.arg.print_icf_sections.empty())
    print_icf_sections(ctx);

  // Update alignment of leaders.
  {
    Timer t(ctx, "update_alignment");
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      for (std::unique_ptr<InputSection<E>> &isec : file->sections)
        if (isec && isec->is_alive && isec->icf_removed())
          update_maximum(isec->leader->p2align, isec->p2align);
    });
  }

  // Eliminate duplicate sections.
  // Symbols pointing to eliminated sections will be redirected on the fly when
  // exporting to the symtab.
  {
    Timer t(ctx, "sweep");
    static Counter eliminated("icf_eliminated");
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
        if (isec && isec->is_alive && isec->icf_removed()) {
          isec->kill();
          eliminated++;
        }
      }
    });
  }
}

using E = MOLD_TARGET;

template void icf_sections(Context<E> &ctx);

} // namespace mold
