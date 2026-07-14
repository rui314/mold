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

#include <cstdio>
#include <fstream>
#include <tbb/concurrent_unordered_map.h>
#include <tbb/concurrent_vector.h>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

namespace mold {

struct Digest {
  bool operator==(const Digest &) const = default;
  u64 hi;
  u64 lo;
};

static u8 hmac_key[16];

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
static bool is_leaf(Context<E> &ctx, InputSection<E> &isec) {
  if (!isec.get_rels(ctx).empty())
    return false;

  for (FdeRecord<E> &fde : isec.get_fdes())
    if (fde.get_rels(isec.file).size() > 1)
      return false;

  return true;
}

template <typename E>
static Digest compute_digest(Context<E> &ctx, InputSection<E> &isec) {
  SipHash13_128 hasher(hmac_key);

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
    } else if (isec->leader) {
      hash('4');
      hash((u64)isec->leader);
    } else if (isec->icf_eligible) {
      hash('5');
    } else {
      hash('6');
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

  for (i64 i = 0; i < isec.get_rels(ctx).size(); i++) {
    const ElfRel<E> &rel = isec.get_rels(ctx)[i];
    hash(rel.r_offset);
    hash(rel.r_type);
    hash(get_addend(isec, rel));
    hash_symbol(*isec.file.symbols[rel.r_sym]);
  }

  Digest digest;
  hasher.finish(&digest);
  return digest;
}

// A concurrent hash map from digest to section. We use it to merge
// leaf sections, to count the number of distinct digests, and to elect
// the leader section of each digest's equivalence class.
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
    u64 done = (round << 49) | (digest.hi >> 16);
    for (i64 i = digest.hi & mask;; i = (i + 1) & mask)
      if (slots[i].hi == done && slots[i].lo == digest.lo)
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

// Early merge of leaf nodes, which can be processed without constructing the
// entire graph. This reduces the vertex count and improves memory efficiency.
//
// A leaf's digest doesn't depend on any other section's digest, so unlike
// the sections that participate in the propagation rounds, its final digest
// is known upfront, and sections that agree on it can be merged right away.
template <typename E>
static void merge_leaf_nodes(Context<E> &ctx) {
  Timer t(ctx, "merge_leaf_nodes");

  static Counter eligible("icf_eligibles");
  static Counter non_eligible("icf_non_eligibles");
  static Counter leaf("icf_leaf_nodes");

  tbb::enumerable_thread_specific<i64> num_leaves;

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections) {
      if (!isec || !isec->is_alive)
        continue;

      if (!is_eligible(ctx, *isec)) {
        non_eligible++;
        continue;
      }

      if (is_leaf(ctx, *isec)) {
        leaf++;
        isec->icf_leaf = true;
        num_leaves.local()++;
      } else {
        eligible++;
        isec->icf_eligible = true;
      }
    }
  });

  DigestMap<E> map(num_leaves.combine(std::plus()));

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections)
      if (isec && isec->is_alive && isec->icf_leaf)
        map.insert(compute_digest(ctx, *isec), isec.get());
  });

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections)
      if (isec && isec->is_alive && isec->icf_leaf)
        isec->leader = map.find(compute_digest(ctx, *isec));
  });
}

template <typename E>
static std::vector<InputSection<E> *> gather_sections(Context<E> &ctx) {
  Timer t(ctx, "gather_sections");

  // Count the number of input sections for each input file.
  std::vector<i64> num_sections(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections)
      if (isec && isec->is_alive && isec->icf_eligible)
        num_sections[i]++;
  });

  std::vector<i64> section_indices(ctx.objs.size());
  for (i64 i = 0; i < ctx.objs.size() - 1; i++)
    section_indices[i + 1] = section_indices[i] + num_sections[i];

  std::vector<InputSection<E> *> sections(
    section_indices.back() + num_sections.back());

  // Fill `sections` contents.
  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    i64 idx = section_indices[i];
    for (std::unique_ptr<InputSection<E>> &isec : ctx.objs[i]->sections)
      if (isec && isec->is_alive && isec->icf_eligible)
        sections[idx++] = isec.get();
  });

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    sections[i]->icf_idx = i;
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
template <typename E>
static void gather_edges(Context<E> &ctx,
                         std::span<InputSection<E> *> sections,
                         std::vector<u32> &edges,
                         std::vector<u32> &edge_indices) {
  Timer t(ctx, "gather_edges");

  if (sections.empty())
    return;

  std::vector<i64> num_edges(sections.size());
  edge_indices.resize(sections.size());

  tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
    InputSection<E> &isec = *sections[i];
    assert(isec.icf_eligible);

    for (i64 j = 0; j < isec.get_rels(ctx).size(); j++) {
      const ElfRel<E> &rel = isec.get_rels(ctx)[j];
      Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
      if (!sym.get_frag())
        if (InputSection<E> *isec = sym.get_input_section())
          if (isec->icf_eligible)
            num_edges[i]++;
    }
  });

  for (i64 i = 0; i < num_edges.size() - 1; i++)
    edge_indices[i + 1] = edge_indices[i] + num_edges[i];

  edges.resize(edge_indices.back() + num_edges.back());

  tbb::parallel_for((i64)0, (i64)num_edges.size(), [&](i64 i) {
    InputSection<E> &isec = *sections[i];
    i64 idx = edge_indices[i];

    for (ElfRel<E> &rel : isec.get_rels(ctx)) {
      Symbol<E> &sym = *isec.file.symbols[rel.r_sym];
      if (InputSection<E> *isec = sym.get_input_section())
        if (isec->icf_eligible)
          edges[idx++] = isec->icf_idx;
  }
  });
}

template <typename E>
static void propagate(std::span<std::vector<Digest>> digests,
                      std::span<u32> edges, std::span<u32> edge_indices,
                      bool slot, std::span<u8> converged,
                      tbb::affinity_partitioner &ap) {
  i64 num_digests = digests[0].size();

  tbb::parallel_for((i64)0, num_digests, [&](i64 i) {
    if (converged[i])
      return;

    SipHash13_128 hasher(hmac_key);
    hasher.update(&digests[2][i], sizeof(Digest));

    i64 begin = edge_indices[i];
    i64 end = (i + 1 == num_digests) ? edges.size() : edge_indices[i + 1];

    for (i64 j : edges.subspan(begin, end - begin))
      hasher.update(&digests[slot][j], sizeof(Digest));

    hasher.finish(&digests[!slot][i]);

    // If this node has converged, skip further iterations as it will
    // yield the same hash.
    if (digests[slot][i] == digests[!slot][i])
      converged[i] = true;
  }, ap);

  static Counter counter("icf_round");
  counter++;
}

template <typename E>
static i64 count_num_classes(std::span<Digest> digests,
                             std::span<InputSection<E> *> sections,
                             DigestMap<E> &map, tbb::affinity_partitioner &ap) {
  map.next_round();

  tbb::enumerable_thread_specific<i64> num_classes;
  tbb::parallel_for((i64)0, (i64)digests.size(), [&](i64 i) {
    if (map.insert(digests[i], sections[i]))
      num_classes.local()++;
  }, ap);
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

  get_random_bytes(hmac_key, sizeof(hmac_key));

  uniquify_cies(ctx);
  merge_leaf_nodes(ctx);

  // Prepare for the propagation rounds.
  std::vector<InputSection<E> *> sections = gather_sections(ctx);

  // We allocate 3 arrays to store hashes for each vertex.
  //
  // Index 0 and 1 are used for tree hashes from the previous
  // iteration and the current iteration. They switch roles every
  // iteration. See `slot` below.
  //
  // Index 2 stores the initial, single-vertex hash. This is combined
  // with hashes from the connected vertices to form the tree hash
  // described above.
  std::vector<std::vector<Digest>> digests(3);
  digests[0] = compute_digests<E>(ctx, sections);
  digests[1].resize(digests[0].size());
  digests[2] = digests[0];

  std::vector<u32> edges;
  std::vector<u32> edge_indices;
  gather_edges<E>(ctx, sections, edges, edge_indices);

  std::vector<u8> converged(digests[0].size());
  bool slot = 0;

  // The digest map is used to count the number of distinct digests in
  // the loop below. As a side effect, it records the lowest-priority
  // section for each digest, which the grouping step after the loop
  // uses as the leader of each equivalence class.
  DigestMap<E> map(digests[0].size());

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
    tbb::affinity_partitioner ap;
    i64 num_classes = -1;

    for (;;) {
      propagate<E>(digests, edges, edge_indices, slot, converged, ap);
      slot = !slot;
      i64 m = count_num_classes<E>(digests[slot], sections, map, ap);
      if (m == num_classes)
        break;
      num_classes = m;
    }
  }

  // Group sections by digest. The final counting round has already
  // elected a leader for each digest; look it up.
  {
    Timer t(ctx, "group");
    std::span<Digest> digest = digests[slot];

    tbb::parallel_for((i64)0, (i64)sections.size(), [&](i64 i) {
      sections[i]->leader = map.find(digest[i]);
    });
  }

  if (!ctx.arg.print_icf_sections.empty())
    print_icf_sections(ctx);

  // Update alignment of leaders.
  {
    Timer t(ctx, "update_alignment");
    tbb::parallel_for_each(ctx.objs, [](ObjectFile<E> *file) {
      for (std::unique_ptr<InputSection<E>> &isec : file->sections) {
        if (isec && isec->is_alive && isec->leader && isec->leader != isec.get()) {
          update_maximum(isec->leader->p2align, isec->p2align);
        }
      }
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
