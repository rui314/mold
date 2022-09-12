#include "mold.h"
#include "../archive-file.h"
#include "../output-file.h"
#include "../sha.h"

#include <cstdlib>
#include <fcntl.h>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sys/stat.h>
#include <sys/types.h>
#include <tbb/concurrent_vector.h>
#include <tbb/global_control.h>
#include <tbb/parallel_for_each.h>

#ifndef _WIN32
# include <sys/mman.h>
# include <sys/time.h>
#endif

namespace mold::macho {

static std::pair<std::string_view, std::string_view>
split_string(std::string_view str, char sep) {
  size_t pos = str.find(sep);
  if (pos == str.npos)
    return {str, ""};
  return {str.substr(0, pos), str.substr(pos + 1)};
}

template <typename E>
static bool has_lto_obj(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    if (file->lto_module)
      return true;
  return false;
}

template <typename E>
static void resolve_symbols(Context<E> &ctx) {
  Timer t(ctx, "resolve_symbols");

  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dylibs);

  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    file->resolve_symbols(ctx);
  });

  if (has_lto_obj(ctx))
    do_lto(ctx);

  for (std::string_view name : ctx.arg.u)
    if (InputFile<E> *file = get_symbol(ctx, name)->file)
      file->is_alive = true;

  if (InputFile<E> *file = ctx.arg.entry->file)
    file->is_alive = true;

  std::vector<ObjectFile<E> *> live_objs;
  for (ObjectFile<E> *file : ctx.objs)
    if (file->is_alive)
      live_objs.push_back(file);

  for (i64 i = 0; i < live_objs.size(); i++) {
    live_objs[i]->mark_live_objects(ctx, [&](ObjectFile<E> *file) {
      live_objs.push_back(file);
    });
  }

  // Remove symbols of eliminated files.
  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    if (!file->is_alive)
      file->clear_symbols();
  });

  // Redo symbol resolution because extracting object files from archives
  // may raise the priority of symbols defined by the object file.
  tbb::parallel_for_each(files, [&](InputFile<E> *file) {
    if (file->is_alive)
      file->resolve_symbols(ctx);
  });

  std::erase_if(ctx.objs, [](InputFile<E> *file) { return !file->is_alive; });
  std::erase_if(ctx.dylibs, [](InputFile<E> *file) { return !file->is_alive; });
}

template <typename E>
static void handle_exported_symbols_list(Context<E> &ctx) {
  Timer t(ctx, "handle_exported_symbols_list");
  if (ctx.arg.exported_symbols_list.empty())
    return;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->syms)
      if (sym && sym->file == file)
        if (sym->scope == SCOPE_EXTERN || sym->scope == SCOPE_PRIVATE_EXTERN)
          sym->scope = ctx.arg.exported_symbols_list.find(sym->name)
            ? SCOPE_EXTERN : SCOPE_PRIVATE_EXTERN;
  });
}

template <typename E>
static void handle_unexported_symbols_list(Context<E> &ctx) {
  Timer t(ctx, "handle_unexported_symbols_list");
  if (ctx.arg.unexported_symbols_list.empty())
    return;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Symbol<E> *sym : file->syms)
      if (sym && sym->file == file)
        if (sym->scope == SCOPE_EXTERN &&
            ctx.arg.unexported_symbols_list.find(sym->name))
          sym->scope = SCOPE_PRIVATE_EXTERN;
  });
}

template <typename E>
static void create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
  obj->is_alive = true;
  obj->mach_syms = obj->mach_syms2;
  ctx.obj_pool.emplace_back(obj);
  ctx.objs.push_back(obj);
  ctx.internal_obj = obj;

  auto add = [&](std::string_view name) {
    Symbol<E> *sym = get_symbol(ctx, name);
    sym->file = obj;
    obj->syms.push_back(sym);
    return sym;
  };

  add("__dyld_private");

  switch (ctx.output_type) {
  case MH_EXECUTE: {
    Symbol<E> *sym = add("__mh_execute_header");
    sym->scope = SCOPE_EXTERN;
    sym->referenced_dynamically = true;
    sym->value = ctx.arg.pagezero_size;
    break;
  }
  case MH_DYLIB:
    add("__mh_dylib_header");
    break;
  case MH_BUNDLE:
    add("__mh_bundle_header");
    break;
  default:
    unreachable();
  }

  add("___dso_handle");

  // Add start stop symbols.
  std::set<std::string_view> start_stop_symbols;
  std::mutex mu;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::set<std::string_view> set;
    for (Symbol<E> *sym : file->syms)
      if (!sym->file)
        if (sym->name.starts_with("segment$start$") ||
            sym->name.starts_with("segment$end$") ||
            sym->name.starts_with("section$start$") ||
            sym->name.starts_with("section$end$"))
          set.insert(sym->name);

    std::scoped_lock lock(mu);
    start_stop_symbols.merge(set);
  });

  for (std::string_view name : start_stop_symbols)
    add(name);
}

// Remove unreferenced subsections to eliminate code and data
// referenced by duplicated weakdef symbols.
template <typename E>
static void remove_unreferenced_subsections(Context<E> &ctx) {
  Timer t(ctx, "remove_unreferenced_subsections");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = 0; i < file->mach_syms.size(); i++) {
      MachSym &msym = file->mach_syms[i];
      Symbol<E> &sym = *file->syms[i];
      if (sym.file != file && (msym.type == N_SECT) && (msym.desc & N_WEAK_DEF))
        file->sym_to_subsec[i]->is_alive = false;
    }
  });

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::erase_if(file->subsections, [](Subsection<E> *subsec) {
      return !subsec->is_alive;
    });
  });
}

template <typename E>
static bool compare_segments(const std::unique_ptr<OutputSegment<E>> &a,
                             const std::unique_ptr<OutputSegment<E>> &b) {
  // We want to sort output segments in the following order:
  // __TEXT, __DATA_CONST, __DATA, <other segments>, __LINKEDIT
  auto get_rank = [](std::string_view name) {
    if (name == "__TEXT")
      return 0;
    if (name == "__DATA_CONST")
      return 1;
    if (name == "__DATA")
      return 2;
    if (name == "__LINKEDIT")
      return 4;
    return 3;
  };

  std::string_view x = a->cmd.get_segname();
  std::string_view y = b->cmd.get_segname();
  return std::tuple{get_rank(x), x} < std::tuple{get_rank(y), y};
}

template <typename E>
static bool compare_chunks(const Chunk<E> *a, const Chunk<E> *b) {
  assert(a->hdr.get_segname() == b->hdr.get_segname());

  auto is_bss = [](const Chunk<E> *x) {
    return x->hdr.type == S_ZEROFILL || x->hdr.type == S_THREAD_LOCAL_ZEROFILL;
  };

  if (is_bss(a) != is_bss(b))
    return !is_bss(a);

  static const std::string_view rank[] = {
    // __TEXT
    "__mach_header",
    "__text",
    "__stubs",
    "__stub_helper",
    "__gcc_except_tab",
    "__cstring",
    "__unwind_info",
    // __DATA_CONST
    "__got",
    "__const",
    // __DATA
    "__mod_init_func",
    "__la_symbol_ptr",
    "__thread_ptrs",
    "__data",
    "__thread_ptr",
    "__thread_data",
    "__thread_vars",
    "__thread_bss",
    "__common",
    "__bss",
    // __LINKEDIT
    "__rebase",
    "__binding",
    "__weak_binding",
    "__lazy_binding",
    "__export",
    "__func_starts",
    "__data_in_code",
    "__symbol_table",
    "__ind_sym_tab",
    "__string_table",
    "__code_signature",
  };

  auto get_rank = [](std::string_view name) {
    i64 i = 0;
    for (; i < sizeof(rank) / sizeof(rank[0]); i++)
      if (name == rank[i])
        return i;
    return i;
  };

  std::string_view x = a->hdr.get_sectname();
  std::string_view y = b->hdr.get_sectname();
  return std::tuple{get_rank(x), x} < std::tuple{get_rank(y), y};
}

template <typename E>
static void claim_unresolved_symbols(Context<E> &ctx) {
  Timer t(ctx, "claim_unresolved_symbols");

  for (std::string_view name : ctx.arg.U) {
    Symbol<E> *sym = get_symbol(ctx, name);
    if (!sym->file) {
      sym->is_imported = true;
      sym->is_weak = true;
    }
  }

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (i64 i = 0; i < file->mach_syms.size(); i++) {
      MachSym &msym = file->mach_syms[i];
      if (!msym.is_extern || !msym.is_undef())
        continue;

      Symbol<E> &sym = *file->syms[i];
      std::scoped_lock lock(sym.mu);

      if (sym.is_imported) {
        if (!sym.file ||
            (!sym.file->is_dylib && file->priority < sym.file->priority)) {
          sym.file = file;
          sym.scope = SCOPE_PRIVATE_EXTERN;
          sym.is_imported = true;
          sym.subsec = nullptr;
          sym.value = 0;
          sym.is_common = false;
        }
      }
    }
  });
}

template <typename E>
static Chunk<E> *find_section(Context<E> &ctx, std::string_view segname,
                              std::string_view sectname) {
  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->hdr.match(segname, sectname))
      return chunk;
  return nullptr;
}

template <typename E>
static void create_synthetic_chunks(Context<E> &ctx) {
  Timer t(ctx, "create_synthetic_chunks");

  // Create a __DATA,__objc_imageinfo section.
  ctx.image_info = ObjcImageInfoSection<E>::create(ctx);

  // Create a __LINKEDIT,__func_starts section.
  if (ctx.arg.function_starts)
    ctx.function_starts.reset(new FunctionStartsSection(ctx));

  // Handle -sectcreate
  for (SectCreateOption arg : ctx.arg.sectcreate) {
    MappedFile<Context<E>> *mf =
      MappedFile<Context<E>>::must_open(ctx, std::string(arg.filename));
    new SectCreateSection<E>(ctx, arg.segname, arg.sectname, mf->get_contents());
  }

  // We add subsections specified by -order_file to output sections.
  for (std::string_view name : ctx.arg.order_file)
    if (Symbol<E> *sym = get_symbol(ctx, name); sym->file)
      if (Subsection<E> *subsec = sym->subsec)
        if (!subsec->added_to_osec)
          subsec->isec.osec.add_subsec(subsec);

  // Add remaining subsections to output sections.
  for (ObjectFile<E> *file : ctx.objs)
    for (Subsection<E> *subsec : file->subsections)
      if (!subsec->added_to_osec)
        subsec->isec.osec.add_subsec(subsec);

  // Add output sections to segments.
  for (Chunk<E> *chunk : ctx.chunks) {
    if (chunk != ctx.data && chunk->is_output_section &&
        ((OutputSection<E> *)chunk)->members.empty())
      continue;

    OutputSegment<E> *seg =
      OutputSegment<E>::get_instance(ctx, chunk->hdr.get_segname());
    seg->chunks.push_back(chunk);
  }

  // Handle -add_empty_section
  for (AddEmptySectionOption &opt : ctx.arg.add_empty_section) {
    if (!find_section(ctx, opt.segname, opt.sectname)) {
      OutputSegment<E> *seg = OutputSegment<E>::get_instance(ctx, opt.segname);
      Chunk<E> *sec = new SectCreateSection<E>(ctx, opt.segname, opt.sectname, {});
      seg->chunks.push_back(sec);
    }
  }

  // Sort segments and output sections.
  sort(ctx.segments, compare_segments<E>);

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    sort(seg->chunks, compare_chunks<E>);
}

template <typename E>
static void uniquify_cstrings(Context<E> &ctx, OutputSection<E> &osec) {
  Timer t(ctx, "uniquify_cstrings");

  struct Entry {
    Entry(Subsection<E> *subsec) : owner(subsec) {}

    Entry(const Entry &other) :
      owner(other.owner.load()), p2align(other.p2align.load()) {}

    std::atomic<Subsection<E> *> owner = nullptr;
    std::atomic_uint8_t p2align = 0;
  };

  struct SubsecRef {
    Subsection<E> *subsec = nullptr;
    u64 hash = 0;
    Entry *ent = nullptr;
  };

  std::vector<SubsecRef> vec(osec.members.size());

  // Estimate the number of unique strings.
  tbb::enumerable_thread_specific<HyperLogLog> estimators;

  tbb::parallel_for((i64)0, (i64)osec.members.size(), [&](i64 i) {
    Subsection<E> *subsec = osec.members[i];
    if (subsec->is_cstring) {
      u64 h = hash_string(subsec->get_contents());
      vec[i].subsec = subsec;
      vec[i].hash = h;
      estimators.local().insert(h);
    }
  });

  HyperLogLog estimator;
  for (HyperLogLog &e : estimators)
    estimator.merge(e);

  // Create a hash map large enough to hold all strings.
  ConcurrentMap<Entry> map(estimator.get_cardinality() * 3 / 2);

  // Insert all strings into the hash table.
  tbb::parallel_for_each(vec, [&](SubsecRef &ref) {
    if (ref.subsec) {
      std::string_view s = ref.subsec->get_contents();
      ref.ent = map.insert(s, ref.hash, {ref.subsec}).first;

      Subsection<E> *existing = ref.ent->owner;
      while (existing->isec.file.priority < ref.subsec->isec.file.priority &&
             !ref.ent->owner.compare_exchange_weak(existing, ref.subsec));

      update_maximum(ref.ent->p2align, ref.subsec->p2align.load());
    }
  });

  // Decide who will become the owner for each subsection.
  tbb::parallel_for_each(vec, [&](SubsecRef &ref) {
    if (ref.subsec && ref.subsec != ref.ent->owner) {
      ref.subsec->is_coalesced = true;
      ref.subsec->replacer = ref.ent->owner;
    }
  });

  static Counter counter("num_merged_strings");
  counter += std::erase_if(osec.members, [](Subsection<E> *subsec) {
    return subsec->is_coalesced;
  });
}

template <typename E>
static void merge_cstring_sections(Context<E> &ctx) {
  Timer t(ctx, "merge_cstring_sections");

  for (Chunk<E> *chunk : ctx.chunks)
    if (chunk->is_output_section && chunk->hdr.type == S_CSTRING_LITERALS)
      uniquify_cstrings(ctx, *(OutputSection<E> *)chunk);

  // Rewrite relocations and symbols.
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (isec)
        for (Relocation<E> &r : isec->rels)
          if (r.subsec && r.subsec->is_coalesced)
            r.subsec = r.subsec->replacer;
  });

  std::vector<InputFile<E> *> files;
  append(files, ctx.objs);
  append(files, ctx.dylibs);

  for (InputFile<E> *file: files)
    for (Symbol<E> *sym : file->syms)
      if (sym && sym->subsec && sym->subsec->is_coalesced)
        sym->subsec = sym->subsec->replacer;

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    std::erase_if(file->subsections, [](Subsection<E> *subsec) {
      return subsec->is_coalesced;
    });
  });
}

template <typename E>
static void scan_unwind_info(Context<E> &ctx) {
  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    for (Subsection<E> *subsec : file->subsections)
      for (UnwindRecord<E> &rec : subsec->get_unwind_records())
        if (rec.personality)
          rec.personality->flags |= NEEDS_GOT;
  });
}

template <typename E>
static void export_symbols(Context<E> &ctx) {
  Timer t(ctx, "export_symbols");

  ctx.got.add(ctx, get_symbol(ctx, "dyld_stub_binder"));

  for (ObjectFile<E> *file : ctx.objs) {
    for (Symbol<E> *sym : file->syms) {
      if (sym && sym->file == file) {
        if (sym->flags & NEEDS_GOT)
          ctx.got.add(ctx, sym);
        if (sym->flags & NEEDS_THREAD_PTR)
          ctx.thread_ptrs.add(ctx, sym);
      } else if (sym && !sym->file) {
        if (sym->flags & NEEDS_STUB)
          ctx.stubs.add(ctx, sym);
      }
    }
  }

  for (DylibFile<E> *file : ctx.dylibs) {
    for (Symbol<E> *sym : file->syms) {
      if (sym && sym->file == file) {
        if (sym->flags & NEEDS_STUB)
          ctx.stubs.add(ctx, sym);
        if (sym->flags & NEEDS_GOT)
          ctx.got.add(ctx, sym);
        if (sym->flags & NEEDS_THREAD_PTR)
          ctx.thread_ptrs.add(ctx, sym);
      }
    }
  }
}

template <typename E>
static i64 assign_offsets(Context<E> &ctx) {
  Timer t(ctx, "assign_offsets");

  i64 sect_idx = 1;
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (!chunk->is_hidden)
        chunk->sect_idx = sect_idx++;

  i64 fileoff = 0;
  i64 vmaddr = ctx.arg.pagezero_size;

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments) {
    seg->set_offset(ctx, fileoff, vmaddr);
    fileoff += seg->cmd.filesize;
    vmaddr += seg->cmd.vmsize;
  }
  return fileoff;
}

// An address of a symbol of type S_THREAD_LOCAL_VARIABLES is computed
// as a relative address to the beginning of the first thread-local
// section. This function finds the beginnning address.
template <typename E>
static u64 get_tls_begin(Context<E> &ctx) {
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->hdr.type == S_THREAD_LOCAL_REGULAR ||
          chunk->hdr.type == S_THREAD_LOCAL_ZEROFILL)
        return chunk->hdr.addr;
  return 0;
}

template <typename E>
static void fix_synthetic_symbol_values(Context<E> &ctx) {
  get_symbol(ctx, "__dyld_private")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_dylib_header")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "__mh_bundle_header")->value = ctx.data->hdr.addr;
  get_symbol(ctx, "___dso_handle")->value = ctx.data->hdr.addr;

  auto find_segment = [&](std::string_view name) -> SegmentCommand * {
    for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
      if (seg->cmd.get_segname() == name)
        return &seg->cmd;
    return nullptr;
  };

  auto find_section = [&](std::string_view name) -> MachSection * {
    size_t pos = name.find('$');
    if (pos == name.npos)
      return nullptr;

    std::string_view segname = name.substr(0, pos);
    std::string_view sectname = name.substr(pos + 1);

    for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
      for (Chunk<E> *chunk : seg->chunks)
        if (chunk->hdr.match(segname, sectname))
          return &chunk->hdr;
    return nullptr;
  };

  for (Symbol<E> *sym : ctx.internal_obj->syms) {
    std::string_view name = sym->name;

    if (remove_prefix(name, "segment$start$")) {
      sym->value = ctx.text->hdr.addr;
      if (SegmentCommand *cmd = find_segment(name))
        sym->value = cmd->vmaddr;
      continue;
    }

    if (remove_prefix(name, "segment$end$")) {
      sym->value = ctx.text->hdr.addr;
      if (SegmentCommand *cmd = find_segment(name))
        sym->value = cmd->vmaddr + cmd->vmsize;
      continue;
    }

    if (remove_prefix(name, "section$start$")) {
      sym->value = ctx.text->hdr.addr;
      if (MachSection *hdr = find_section(name))
        sym->value = hdr->addr;
      continue;
    }

    if (remove_prefix(name, "section$end$")) {
      sym->value = ctx.text->hdr.addr;
      if (MachSection *hdr = find_section(name))
        sym->value = hdr->addr + hdr->size;
    }
  }
}

template <typename E>
static void copy_sections_to_output_file(Context<E> &ctx) {
  Timer t(ctx, "copy_sections_to_output_file");

  tbb::parallel_for_each(ctx.segments,
                         [&](std::unique_ptr<OutputSegment<E>> &seg) {
    Timer t2(ctx, std::string(seg->cmd.get_segname()), &t);

    // Fill text segment paddings with single-byte NOP instructions so
    // that otool wouldn't out-of-sync when disassembling an output file.
    // Do this only for x86-64 because ARM64 instructions are always 4
    // bytes long.
    if constexpr (std::is_same_v<E, X86_64>)
      if (seg->cmd.get_segname() == "__TEXT")
        memset(ctx.buf + seg->cmd.fileoff, 0x90, seg->cmd.filesize);

    tbb::parallel_for_each(seg->chunks, [&](Chunk<E> *sec) {
      if (sec->hdr.type != S_ZEROFILL) {
        Timer t3(ctx, std::string(sec->hdr.get_sectname()), &t2);
        sec->copy_buf(ctx);
      }
    });
  });
}

template <typename E>
static void compute_uuid(Context<E> &ctx) {
  Timer t(ctx, "copy_sections_to_output_file");

  // Compute a markle tree of height two.
  i64 filesize = ctx.output_file->filesize;
  i64 shard_size = 4096 * 1024;
  i64 num_shards = align_to(filesize, shard_size) / shard_size;
  std::vector<u8> shards(num_shards * SHA256_SIZE);

  tbb::parallel_for((i64)0, num_shards, [&](i64 i) {
    u8 *begin = ctx.buf + shard_size * i;
    u8 *end = (i == num_shards - 1) ? ctx.buf + filesize : begin + shard_size;
    sha256_hash(begin, end - begin, shards.data() + i * SHA256_SIZE);
  });

  u8 buf[SHA256_SIZE];
  sha256_hash(shards.data(), shards.size(), buf);
  memcpy(ctx.uuid, buf, 16);
  ctx.mach_hdr.copy_buf(ctx);
}

template <typename E>
static MappedFile<Context<E>> *
strip_universal_header(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  FatHeader &hdr = *(FatHeader *)mf->data;
  assert(hdr.magic == FAT_MAGIC);

  FatArch *arch = (FatArch *)(mf->data + sizeof(hdr));
  for (i64 i = 0; i < hdr.nfat_arch; i++)
    if (arch[i].cputype == E::cputype)
      return mf->slice(ctx, mf->name, arch[i].offset, arch[i].size);
  Fatal(ctx) << mf->name << ": fat file contains no matching file";
}

template <typename E>
static void read_file(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  if (get_file_type(mf) == FileType::MACH_UNIVERSAL)
    mf = strip_universal_header(ctx, mf);

  switch (get_file_type(mf)) {
  case FileType::TAPI:
  case FileType::MACH_DYLIB:
  case FileType::MACH_EXE:
    ctx.dylibs.push_back(DylibFile<E>::create(ctx, mf));
    break;
  case FileType::MACH_OBJ:
  case FileType::LLVM_BITCODE:
    ctx.objs.push_back(ObjectFile<E>::create(ctx, mf, ""));
    break;
  case FileType::AR:
    for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf))
      if (get_file_type(child) == FileType::MACH_OBJ)
        ctx.objs.push_back(ObjectFile<E>::create(ctx, child, mf->name));
    break;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
    break;
  }
}

template <typename E>
static std::vector<std::string>
read_filelist(Context<E> &ctx, std::string arg) {
  std::string path;
  std::string dir;

  if (size_t pos = arg.find(','); pos != arg.npos) {
    path = arg.substr(0, pos);
    dir = arg.substr(pos + 1) + "/";
  } else {
    path = arg;
  }

  MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
  if (!mf)
    Fatal(ctx) << "-filepath: cannot open " << path;

  std::vector<std::string> vec;
  for (std::string_view str = mf->get_contents(); !str.empty();) {
    std::string_view path;
    std::tie(path, str) = split_string(str, '\n');
    vec.push_back(path_clean(dir + std::string(path)));
  }
  return vec;
}

template <typename E>
static bool has_dylib(Context<E> &ctx, std::string_view path) {
  for (DylibFile<E> *file : ctx.dylibs)
    if (file->install_name == path)
      return true;
  return false;
}

template <typename E>
static void read_input_files(Context<E> &ctx, std::span<std::string> args) {
  Timer t(ctx, "read_input_files");

  std::unordered_set<std::string> libs;
  std::unordered_set<std::string> frameworks;

  auto search = [&](std::vector<std::string> names)
      -> MappedFile<Context<E>> * {
    for (std::string dir : ctx.arg.library_paths) {
      for (std::string name : names) {
        std::string path = dir + "/lib" + name;
        if (MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path))
          return mf;
        ctx.missing_files.insert(path);
      }
    }
    return nullptr;
  };

  auto find_library = [&](std::string name) -> MappedFile<Context<E>> * {
    // -search_paths_first
    if (ctx.arg.search_paths_first)
      return search({name + ".tbd", name + ".dylib", name + ".a"});

    // -search_dylibs_first
    if (MappedFile<Context<E>> *mf = search({name + ".tbd", name + ".dylib"}))
      return mf;
    return search({name + ".a"});
  };

  auto read_library = [&](std::string name) {
    if (!libs.insert(name).second)
      return;

    MappedFile<Context<E>> *mf = find_library(name);
    if (!mf)
      Fatal(ctx) << "library not found: -l" << name;
    read_file(ctx, mf);
  };

  auto find_framework = [&](std::string name) -> MappedFile<Context<E>> * {
    std::string suffix;
    std::tie(name, suffix) = split_string(name, ',');

    for (std::string path : ctx.arg.framework_paths) {
      path = get_realpath(path + "/" + name + ".framework/" + name);

      if (!suffix.empty())
        if (auto *mf = MappedFile<Context<E>>::open(ctx, path + suffix))
          return mf;

      if (auto *mf = MappedFile<Context<E>>::open(ctx, path + ".tbd"))
        return mf;

      if (auto *mf = MappedFile<Context<E>>::open(ctx, path))
        return mf;
    }
    return nullptr;
  };

  auto read_framework = [&](std::string name) {
    if (!frameworks.insert(name).second)
      return;

    MappedFile<Context<E>> *mf = find_framework(name);
    if (!mf)
      Fatal(ctx) << "-framework not found: " << name;
    read_file(ctx, mf);
  };

  while (!args.empty()) {
    const std::string &opt = args[0];
    args = args.subspan(1);

    if (!opt.starts_with('-')) {
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, opt));
      continue;
    }

    if (opt == "-all_load") {
      ctx.all_load = true;
      continue;
    }

    if (opt == "-noall_load") {
      ctx.all_load = false;
      continue;
    }

    if (args.empty())
      Fatal(ctx) << opt << ": missing argument";

    const std::string &arg = args[0];
    args = args.subspan(1);

    if (opt == "-filelist") {
      for (std::string &path : read_filelist(ctx, arg)) {
        MappedFile<Context<E>> *mf = MappedFile<Context<E>>::open(ctx, path);
        if (!mf)
          Fatal(ctx) << "-filepath " << arg << ": cannot open file: " << path;
        read_file(ctx, mf);
      }
    } else if (opt == "-force_load") {
      bool orig = ctx.all_load;
      ctx.all_load = true;
      read_file(ctx, MappedFile<Context<E>>::must_open(ctx, arg));
      ctx.all_load = orig;
    } else if (opt == "-framework") {
      read_framework(arg);
    } else if (opt == "-needed_framework") {
      ctx.needed_l = true;
      read_framework(arg);
    } else if (opt == "-weak_framework") {
      ctx.weak_l = true;
      read_framework(arg);
    } else if (opt == "-l") {
      read_library(arg);
    } else if (opt == "-needed-l") {
      ctx.needed_l = true;
      read_library(arg);
    } else if (opt == "-hidden-l") {
      ctx.hidden_l = true;
      read_library(arg);
    } else if (opt == "-weak-l") {
      ctx.weak_l = true;
      read_library(arg);
    } else if (opt == "-reexport-l") {
      ctx.reexport_l = true;
      read_library(arg);
    } else {
      unreachable();
    }

    ctx.needed_l = false;
    ctx.hidden_l = false;
    ctx.weak_l = false;
    ctx.reexport_l = false;
  }

  // With -bundle_loader, we can import symbols from a main executable.
  if (!ctx.arg.bundle_loader.empty())
    read_library(ctx.arg.bundle_loader);

  // An object file can contain linker directives to load other object
  // files or libraries, so process them if any.
  for (i64 i = 0; i < ctx.objs.size(); i++) {
    ObjectFile<E> *file = ctx.objs[i];
    std::vector<std::string> opts = file->get_linker_options(ctx);

    for (i64 j = 0; j < opts.size();) {
      if (opts[j] == "-framework") {
        if (frameworks.insert(opts[j + 1]).second)
          if (MappedFile<Context<E>> *mf = find_framework(opts[j + 1]))
            read_file(ctx, mf);
        j += 2;
      } else if (opts[j].starts_with("-l")) {
        std::string name = opts[j].substr(2);
        if (libs.insert(name).second)
          if (MappedFile<Context<E>> *mf = find_library(name))
            read_file(ctx, mf);
        j++;
      } else {
        Fatal(ctx) << *file << ": unknown LC_LINKER_OPTION command: " << opts[j];
      }
    }
  }

  if (ctx.objs.empty())
    Fatal(ctx) << "no input files";

  for (ObjectFile<E> *file : ctx.objs)
    file->priority = ctx.file_priority++;
  for (DylibFile<E> *dylib : ctx.dylibs)
    dylib->priority = ctx.file_priority++;

  for (i64 i = 1; DylibFile<E> *file : ctx.dylibs)
    if (file->dylib_idx != BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE)
      file->dylib_idx = i++;
}

template <typename E>
static void parse_object_files(Context<E> &ctx) {
  Timer t(ctx, "parse_object_files");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    file->parse(ctx);
  });
}

template <typename E>
static void write_dependency_info(Context<E> &ctx) {
  static constexpr u8 LINKER_VERSION = 0;
  static constexpr u8 INPUT_FILE = 0x10;
  static constexpr u8 NOT_FOUND_FILE = 0x11;
  static constexpr u8 OUTPUT_FILE = 0x40;

  std::ofstream out;
  out.open(std::string(ctx.arg.dependency_info).c_str());
  if (!out.is_open())
    Fatal(ctx) << "cannot open " << ctx.arg.dependency_info
               << ": " << errno_string();

  out << LINKER_VERSION << mold_version << '\0';

  std::set<std::string_view> input_files;
  for (std::unique_ptr<MappedFile<Context<E>>> &mf : ctx.mf_pool)
    if (!mf->parent)
      input_files.insert(mf->name);

  for (std::string_view s : input_files)
    out << INPUT_FILE << s << '\0';

  for (std::string_view s : ctx.missing_files)
    out << NOT_FOUND_FILE << s << '\0';

  out << OUTPUT_FILE << ctx.arg.output << '\0';
  out.close();
}

template <typename E>
static void print_stats(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs) {
    static Counter subsections("num_subsections");
    subsections += file->subsections.size();

    static Counter syms("num_syms");
    syms += file->syms.size();

    static Counter rels("num_rels");
    for (std::unique_ptr<InputSection<E>> &isec : file->sections)
      if (isec)
        rels += isec->rels.size();
  }

  Counter::print();
}

template <typename E>
static int do_main(int argc, char **argv) {
  Context<E> ctx;

  for (i64 i = 0; i < argc; i++)
    ctx.cmdline_args.push_back(argv[i]);

  std::vector<std::string> file_args = parse_nonpositional_args(ctx);

  if (ctx.arg.arch != E::cputype) {
#ifndef MOLD_DEBUG_X86_64_ONLY
    switch (ctx.arg.arch) {
    case CPU_TYPE_X86_64:
      return do_main<X86_64>(argc, argv);
    case CPU_TYPE_ARM64:
      return do_main<X86_64>(argc, argv);
    }
#endif
    Fatal(ctx) << "unknown cputype: " << ctx.arg.arch;
  }

  Timer t(ctx, "all");

  tbb::global_control tbb_cont(tbb::global_control::max_allowed_parallelism,
                               ctx.arg.thread_count);

  if (ctx.arg.adhoc_codesign)
    ctx.code_sig.reset(new CodeSignatureSection<E>(ctx));

  read_input_files(ctx, file_args);
  parse_object_files(ctx);

  if (ctx.arg.ObjC)
    for (ObjectFile<E> *file : ctx.objs)
      if (!file->archive_name.empty() && file->is_objc_object(ctx))
        file->is_alive = true;

  resolve_symbols(ctx);
  remove_unreferenced_subsections(ctx);

  if (ctx.output_type == MH_EXECUTE && !ctx.arg.entry->file)
    Error(ctx) << "undefined entry point symbol: " << *ctx.arg.entry;

  // Handle -exported_symbol and -exported_symbols_list
  handle_exported_symbols_list(ctx);

  // Handle -unexported_symbol and -unexported_symbols_list
  handle_unexported_symbols_list(ctx);

  create_internal_file(ctx);

  if (ctx.arg.trace) {
    for (ObjectFile<E> *file : ctx.objs)
      SyncOut(ctx) << *file;
    for (DylibFile<E> *file : ctx.dylibs)
      SyncOut(ctx) << *file;
  }

  for (ObjectFile<E> *file : ctx.objs)
    file->convert_common_symbols(ctx);

  claim_unresolved_symbols(ctx);

  if (ctx.arg.dead_strip)
    dead_strip(ctx);

  create_synthetic_chunks(ctx);
  merge_cstring_sections(ctx);

  for (ObjectFile<E> *file : ctx.objs)
    file->check_duplicate_symbols(ctx);

  for (SectAlignOption &opt : ctx.arg.sectalign)
    if (Chunk<E> *chunk = find_section(ctx, opt.segname, opt.sectname))
      chunk->hdr.p2align = opt.p2align;

  bool has_pagezero_seg = ctx.arg.pagezero_size;
  for (i64 i = 0; i < ctx.segments.size(); i++)
    ctx.segments[i]->seg_idx = (has_pagezero_seg ? i + 1 : i);

  for (ObjectFile<E> *file : ctx.objs)
    for (Subsection<E> *subsec : file->subsections)
      subsec->scan_relocations(ctx);

  scan_unwind_info(ctx);
  export_symbols(ctx);

  i64 output_size = assign_offsets(ctx);
  ctx.tls_begin = get_tls_begin(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file =
    OutputFile<Context<E>>::open(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  copy_sections_to_output_file(ctx);

  if constexpr (std::is_same_v<E, ARM64>)
    if (!ctx.arg.ignore_optimization_hints)
      apply_linker_optimization_hints(ctx);

  if (ctx.code_sig)
    ctx.code_sig->write_signature(ctx);
  else if (ctx.arg.uuid == UUID_HASH)
    compute_uuid(ctx);

  ctx.output_file->close(ctx);

  if (!ctx.arg.dependency_info.empty())
    write_dependency_info(ctx);

  ctx.checkpoint();
  t.stop();

  if (ctx.arg.perf)
    print_timer_records(ctx.timer_records);

  if (ctx.arg.stats)
    print_stats(ctx);

  if (!ctx.arg.map.empty())
    print_map(ctx);

  if (ctx.arg.quick_exit) {
    std::cout << std::flush;
    std::cerr << std::flush;
    _exit(0);
  }

  return 0;
}

int main(int argc, char **argv) {
#ifdef MOLD_DEBUG_X86_64_ONLY
  return do_main<X86_64>(argc, argv);
#else
  return do_main<ARM64>(argc, argv);
#endif
}

}
