#include "mold.h"
#include "../sha.h"

#include <shared_mutex>
#include <tbb/enumerable_thread_specific.h>
#include <tbb/parallel_for.h>
#include <tbb/parallel_for_each.h>
#include <tbb/parallel_sort.h>

#ifndef _WIN32
# include <sys/mman.h>
#endif

namespace mold::macho {

template <typename E>
std::ostream &operator<<(std::ostream &out, const Chunk<E> &chunk) {
  out << chunk.hdr.get_segname() << "," << chunk.hdr.get_sectname();
  return out;
}

template <typename E>
static std::vector<u8> create_pagezero_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(SegmentCommand));
  SegmentCommand &cmd = *(SegmentCommand *)buf.data();

  cmd.cmd = LC_SEGMENT_64;
  cmd.cmdsize = buf.size();
  strcpy(cmd.segname, "__PAGEZERO");
  cmd.vmsize = ctx.arg.pagezero_size;
  return buf;
}

template <typename E>
static std::vector<u8> create_dyld_info_only_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(DyldInfoCommand));
  DyldInfoCommand &cmd = *(DyldInfoCommand *)buf.data();

  cmd.cmd = LC_DYLD_INFO_ONLY;
  cmd.cmdsize = buf.size();

  cmd.rebase_off = ctx.rebase.hdr.offset;
  cmd.rebase_size = ctx.rebase.hdr.size;

  cmd.bind_off = ctx.bind.hdr.offset;
  cmd.bind_size = ctx.bind.hdr.size;

  cmd.lazy_bind_off = ctx.lazy_bind.hdr.offset;
  cmd.lazy_bind_size = ctx.lazy_bind.hdr.size;

  cmd.export_off = ctx.export_.hdr.offset;
  cmd.export_size = ctx.export_.hdr.size;
  return buf;
}

template <typename E>
static std::vector<u8> create_symtab_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(SymtabCommand));
  SymtabCommand &cmd = *(SymtabCommand *)buf.data();

  cmd.cmd = LC_SYMTAB;
  cmd.cmdsize = buf.size();
  cmd.symoff = ctx.symtab.hdr.offset;
  cmd.nsyms = ctx.symtab.hdr.size / sizeof(MachSym);
  cmd.stroff = ctx.strtab.hdr.offset;
  cmd.strsize = ctx.strtab.hdr.size;
  return buf;
}

template <typename E>
static std::vector<u8> create_dysymtab_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(DysymtabCommand));
  DysymtabCommand &cmd = *(DysymtabCommand *)buf.data();

  cmd.cmd = LC_DYSYMTAB;
  cmd.cmdsize = buf.size();

  cmd.ilocalsym = 0;
  cmd.nlocalsym = ctx.symtab.num_locals;
  cmd.iextdefsym = ctx.symtab.num_locals;
  cmd.nextdefsym = ctx.symtab.num_globals;
  cmd.iundefsym = ctx.symtab.num_locals + ctx.symtab.num_globals;
  cmd.nundefsym = ctx.symtab.num_undefs;
  return buf;
}

template <typename E>
static std::vector<u8> create_dylinker_cmd(Context<E> &ctx) {
  static constexpr char path[] = "/usr/lib/dyld";

  std::vector<u8> buf(align_to(sizeof(DylinkerCommand) + sizeof(path), 8));
  DylinkerCommand &cmd = *(DylinkerCommand *)buf.data();

  cmd.cmd = LC_LOAD_DYLINKER;
  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  memcpy(buf.data() + sizeof(cmd), path, sizeof(path));
  return buf;
}

template <typename E>
static std::vector<u8> create_uuid_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(UUIDCommand));
  UUIDCommand &cmd = *(UUIDCommand *)buf.data();

  cmd.cmd = LC_UUID;
  cmd.cmdsize = buf.size();

  assert(sizeof(cmd.uuid) == sizeof(ctx.uuid));
  memcpy(cmd.uuid, ctx.uuid, sizeof(cmd.uuid));
  return buf;
}

template <typename E>
static std::vector<u8> create_build_version_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(BuildVersionCommand) + sizeof(BuildToolVersion));
  BuildVersionCommand &cmd = *(BuildVersionCommand *)buf.data();

  cmd.cmd = LC_BUILD_VERSION;
  cmd.cmdsize = buf.size();
  cmd.platform = ctx.arg.platform;
  cmd.minos = ctx.arg.platform_min_version;
  cmd.sdk = ctx.arg.platform_sdk_version;
  cmd.ntools = 1;

  BuildToolVersion &tool = *(BuildToolVersion *)(buf.data() + sizeof(cmd));
  tool.tool = TOOL_MOLD;
  tool.version = parse_version(ctx, mold_version_string);
  return buf;
}

template <typename E>
static std::vector<u8> create_source_version_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(SourceVersionCommand));
  SourceVersionCommand &cmd = *(SourceVersionCommand *)buf.data();

  cmd.cmd = LC_SOURCE_VERSION;
  cmd.cmdsize = buf.size();
  return buf;
}

template <typename E>
static std::vector<u8> create_main_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(EntryPointCommand));
  EntryPointCommand &cmd = *(EntryPointCommand *)buf.data();

  cmd.cmd = LC_MAIN;
  cmd.cmdsize = buf.size();
  cmd.entryoff = ctx.arg.entry->get_addr(ctx) - ctx.arg.pagezero_size;
  cmd.stacksize = ctx.arg.stack_size;
  return buf;
}

template <typename E>
static std::vector<u8>
create_load_dylib_cmd(Context<E> &ctx, DylibFile<E> &dylib) {
  i64 size = sizeof(DylibCommand) + dylib.install_name.size() + 1; // +1 for NUL
  std::vector<u8> buf(align_to(size, 8));
  DylibCommand &cmd = *(DylibCommand *)buf.data();

  if (dylib.is_reexported)
    cmd.cmd = LC_REEXPORT_DYLIB;
  else if (dylib.is_weak)
    cmd.cmd = LC_LOAD_WEAK_DYLIB;
  else
    cmd.cmd = LC_LOAD_DYLIB;

  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  cmd.timestamp = 2;
  cmd.current_version = ctx.arg.current_version;
  cmd.compatibility_version = ctx.arg.compatibility_version;
  write_string(buf.data() + sizeof(cmd), dylib.install_name);
  return buf;
}

template <typename E>
static std::vector<u8> create_rpath_cmd(Context<E> &ctx, std::string_view name) {
  i64 size = sizeof(RpathCommand) + name.size() + 1; // +1 for NUL
  std::vector<u8> buf(align_to(size, 8));
  RpathCommand &cmd = *(RpathCommand *)buf.data();

  cmd.cmd = LC_RPATH;
  cmd.cmdsize = buf.size();
  cmd.path_off = sizeof(cmd);
  write_string(buf.data() + sizeof(cmd), name);
  return buf;
}

template <typename E>
static std::vector<u8> create_function_starts_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(LinkEditDataCommand));
  LinkEditDataCommand &cmd = *(LinkEditDataCommand *)buf.data();

  cmd.cmd = LC_FUNCTION_STARTS;
  cmd.cmdsize = buf.size();
  cmd.dataoff = ctx.function_starts->hdr.offset;
  cmd.datasize = ctx.function_starts->hdr.size;
  return buf;
}

template <typename E>
static std::vector<u8> create_data_in_code_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(LinkEditDataCommand));
  LinkEditDataCommand &cmd = *(LinkEditDataCommand *)buf.data();

  cmd.cmd = LC_DATA_IN_CODE;
  cmd.cmdsize = buf.size();
  cmd.dataoff = ctx.data_in_code.hdr.offset;
  cmd.datasize = ctx.data_in_code.hdr.size;
  return buf;
}

template <typename E>
static std::vector<u8> create_id_dylib_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(DylibCommand) +
                      align_to(ctx.arg.final_output.size() + 1, 8));
  DylibCommand &cmd = *(DylibCommand *)buf.data();

  cmd.cmd = LC_ID_DYLIB;
  cmd.cmdsize = buf.size();
  cmd.nameoff = sizeof(cmd);
  write_string(buf.data() + sizeof(cmd), ctx.arg.final_output);
  return buf;
}

template <typename E>
static std::vector<u8> create_code_signature_cmd(Context<E> &ctx) {
  std::vector<u8> buf(sizeof(LinkEditDataCommand));
  LinkEditDataCommand &cmd = *(LinkEditDataCommand *)buf.data();

  cmd.cmd = LC_CODE_SIGNATURE;
  cmd.cmdsize = buf.size();
  cmd.dataoff = ctx.code_sig->hdr.offset;
  cmd.datasize = ctx.code_sig->hdr.size;
  return buf;
}

template <typename E>
static std::vector<std::vector<u8>> create_load_commands(Context<E> &ctx) {
  std::vector<std::vector<u8>> vec;

  if (ctx.arg.pagezero_size)
    vec.push_back(create_pagezero_cmd(ctx));

  auto append = [&](std::vector<u8> &buf, auto x) {
    i64 off = buf.size();
    buf.resize(buf.size() + sizeof(x));
    memcpy(buf.data() + off, &x, sizeof(x));
  };

  // Add LC_SEGMENT_64 comamnds
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments) {
    std::vector<u8> &buf = vec.emplace_back();

    i64 nsects = 0;
    for (Chunk<E> *sec : seg->chunks)
      if (!sec->is_hidden)
        nsects++;

    SegmentCommand cmd = seg->cmd;
    cmd.cmdsize = sizeof(SegmentCommand) + sizeof(MachSection) * nsects;
    cmd.nsects = nsects;
    append(buf, cmd);

    for (Chunk<E> *sec : seg->chunks) {
      if (!sec->is_hidden) {
        sec->hdr.set_segname(cmd.segname);
        append(buf, sec->hdr);
      }
    }
  }

  vec.push_back(create_dyld_info_only_cmd(ctx));
  vec.push_back(create_symtab_cmd(ctx));
  vec.push_back(create_dysymtab_cmd(ctx));
  if (ctx.arg.uuid != UUID_NONE)
    vec.push_back(create_uuid_cmd(ctx));
  vec.push_back(create_build_version_cmd(ctx));
  vec.push_back(create_source_version_cmd(ctx));
  if (ctx.arg.function_starts)
    vec.push_back(create_function_starts_cmd(ctx));

  for (DylibFile<E> *file : ctx.dylibs)
    if (file->dylib_idx != BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE)
      vec.push_back(create_load_dylib_cmd(ctx, *file));

  for (std::string_view rpath : ctx.arg.rpath)
    vec.push_back(create_rpath_cmd(ctx, rpath));

  if (!ctx.data_in_code.contents.empty())
    vec.push_back(create_data_in_code_cmd(ctx));

  switch (ctx.output_type) {
  case MH_EXECUTE:
    vec.push_back(create_dylinker_cmd(ctx));
    vec.push_back(create_main_cmd(ctx));
    break;
  case MH_DYLIB:
    vec.push_back(create_id_dylib_cmd(ctx));
    break;
  case MH_BUNDLE:
    break;
  default:
    unreachable();
  }

  if (ctx.code_sig)
    vec.push_back(create_code_signature_cmd(ctx));
  return vec;
}

template <typename E>
void OutputMachHeader<E>::compute_size(Context<E> &ctx) {
  std::vector<std::vector<u8>> cmds = create_load_commands(ctx);
  this->hdr.size = sizeof(MachHeader) + flatten(cmds).size() + ctx.arg.headerpad;
}

template <typename E>
static bool has_tlv(Context<E> &ctx) {
  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->hdr.type == S_THREAD_LOCAL_VARIABLES)
        return true;
  return false;
}

template <typename E>
static bool has_reexported_lib(Context<E> &ctx) {
  for (DylibFile<E> *file : ctx.dylibs)
    if (file->is_reexported)
      return true;
  return false;
}

template <typename E>
void OutputMachHeader<E>::copy_buf(Context<E> &ctx) {
  u8 *buf = ctx.buf + this->hdr.offset;

  std::vector<std::vector<u8>> cmds = create_load_commands(ctx);

  MachHeader &mhdr = *(MachHeader *)buf;
  mhdr.magic = 0xfeedfacf;
  mhdr.cputype = E::cputype;
  mhdr.cpusubtype = E::cpusubtype;
  mhdr.filetype = ctx.output_type;
  mhdr.ncmds = cmds.size();
  mhdr.sizeofcmds = flatten(cmds).size();
  mhdr.flags = MH_TWOLEVEL | MH_NOUNDEFS | MH_DYLDLINK | MH_PIE;

  if (has_tlv(ctx))
    mhdr.flags |= MH_HAS_TLV_DESCRIPTORS;

  if (ctx.output_type == MH_DYLIB && !has_reexported_lib(ctx))
    mhdr.flags |= MH_NO_REEXPORTED_DYLIBS;

  if (ctx.arg.mark_dead_strippable_dylib)
    mhdr.flags |= MH_DEAD_STRIPPABLE_DYLIB;

  write_vector(buf + sizeof(mhdr), flatten(cmds));
}

template <typename E>
OutputSection<E> *
OutputSection<E>::get_instance(Context<E> &ctx, std::string_view segname,
                               std::string_view sectname) {
  static std::shared_mutex mu;

  auto find = [&]() -> OutputSection<E> * {
    for (Chunk<E> *chunk : ctx.chunks) {
      if (chunk->hdr.match(segname, sectname)) {
        if (!chunk->is_output_section)
          Fatal(ctx) << "reserved name is used: " << segname << "," << sectname;
        return (OutputSection<E> *)chunk;
      }
    }
    return nullptr;
  };

  {
    std::shared_lock lock(mu);
    if (OutputSection<E> *osec = find())
      return osec;
  }

  std::unique_lock lock(mu);
  if (OutputSection<E> *osec = find())
    return osec;

  OutputSection<E> *osec = new OutputSection<E>(ctx, segname, sectname);
  ctx.chunk_pool.emplace_back(osec);
  return osec;
}

template <typename E>
void OutputSection<E>::compute_size(Context<E> &ctx) {
  if constexpr (std::is_same_v<E, ARM64>) {
    if (this->hdr.attr & S_ATTR_SOME_INSTRUCTIONS ||
        this->hdr.attr & S_ATTR_PURE_INSTRUCTIONS) {
      create_range_extension_thunks(ctx, *this);
      return;
    }
  }

  u64 offset = 0;

  if (this == ctx.data) {
    // As a special case, we need a word-size padding at the beginning
    // of __data for dyld. It is located by __dyld_private symbol.
    offset += 8;
  }

  for (Subsection<E> *subsec : members) {
    offset = align_to(offset, 1 << subsec->p2align);
    subsec->output_offset = offset;
    offset += subsec->input_size;
  }
  this->hdr.size = offset;
}

template <typename E>
void OutputSection<E>::copy_buf(Context<E> &ctx) {
  assert(this->hdr.type != S_ZEROFILL);

  tbb::parallel_for_each(members, [&](Subsection<E> *subsec) {
    std::string_view data = subsec->get_contents();
    u8 *loc = ctx.buf + this->hdr.offset + subsec->output_offset;
    memcpy(loc, data.data(), data.size());
    subsec->apply_reloc(ctx, loc);
  });

  if constexpr (std::is_same_v<E, ARM64>) {
    tbb::parallel_for_each(thunks,
                           [&](std::unique_ptr<RangeExtensionThunk<E>> &thunk) {
      thunk->copy_buf(ctx);
    });
  }
}

template <typename E>
OutputSegment<E> *
OutputSegment<E>::get_instance(Context<E> &ctx, std::string_view name) {
  static std::shared_mutex mu;

  auto find = [&]() -> OutputSegment<E> *{
    for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
      if (seg->cmd.get_segname() == name)
        return seg.get();
    return nullptr;
  };

  {
    std::shared_lock lock(mu);
    if (OutputSegment<E> *seg = find())
      return seg;
  }

  std::unique_lock lock(mu);
  if (OutputSegment<E> *seg = find())
    return seg;

  OutputSegment<E> *seg = new OutputSegment<E>(name);
  ctx.segments.emplace_back(seg);
  return seg;
}

template <typename E>
OutputSegment<E>::OutputSegment(std::string_view name) {
  cmd.cmd = LC_SEGMENT_64;
  memcpy(cmd.segname, name.data(), name.size());

  if (name == "__PAGEZERO")
    cmd.initprot = cmd.maxprot = 0;
  else if (name == "__TEXT")
    cmd.initprot = cmd.maxprot = VM_PROT_READ | VM_PROT_EXECUTE;
  else if (name == "__LINKEDIT")
    cmd.initprot = cmd.maxprot = VM_PROT_READ;
  else
    cmd.initprot = cmd.maxprot = VM_PROT_READ | VM_PROT_WRITE;

  if (name == "__DATA_CONST")
    cmd.flags = SG_READ_ONLY;
}

template <typename E>
void OutputSegment<E>::set_offset(Context<E> &ctx, i64 fileoff, u64 vmaddr) {
  cmd.fileoff = fileoff;
  cmd.vmaddr = vmaddr;

  if (cmd.get_segname() == "__LINKEDIT")
    set_offset_linkedit(ctx, fileoff, vmaddr);
  else
    set_offset_regular(ctx, fileoff, vmaddr);
}

template <typename E>
void OutputSegment<E>::set_offset_regular(Context<E> &ctx, i64 fileoff,
                                          u64 vmaddr) {
  Timer t(ctx, std::string(cmd.get_segname()));
  i64 i = 0;

  auto is_bss = [](Chunk<E> &x) {
    return x.hdr.type == S_ZEROFILL || x.hdr.type == S_THREAD_LOCAL_ZEROFILL;
  };

  auto get_alignment = [](Chunk<E> &chunk) {
    switch (chunk.hdr.type) {
    case S_THREAD_LOCAL_REGULAR:
    case S_THREAD_LOCAL_ZEROFILL:
    case S_THREAD_LOCAL_VARIABLES:
      return 16;
    default:
      return 1 << chunk.hdr.p2align;
    }
  };

  // Assign offsets to non-BSS sections
  while (i < chunks.size() && !is_bss(*chunks[i])) {
    Timer t2(ctx, std::string(chunks[i]->hdr.get_sectname()), &t);
    Chunk<E> &sec = *chunks[i++];

    fileoff = align_to(fileoff, get_alignment(sec));
    vmaddr = align_to(vmaddr, get_alignment(sec));

    sec.hdr.offset = fileoff;
    sec.hdr.addr = vmaddr;

    sec.compute_size(ctx);
    fileoff += sec.hdr.size;
    vmaddr += sec.hdr.size;
  }

  // Assign offsets to BSS sections
  while (i < chunks.size()) {
    Chunk<E> &sec = *chunks[i++];
    assert(is_bss(sec));

    vmaddr = align_to(vmaddr, get_alignment(sec));
    sec.hdr.addr = vmaddr;
    sec.compute_size(ctx);
    vmaddr += sec.hdr.size;
  }

  cmd.vmsize = align_to(vmaddr - cmd.vmaddr, COMMON_PAGE_SIZE);
  cmd.filesize = align_to(fileoff - cmd.fileoff, COMMON_PAGE_SIZE);
}

template <typename E>
void OutputSegment<E>::set_offset_linkedit(Context<E> &ctx, i64 fileoff,
                                           u64 vmaddr) {
  Timer t(ctx, "__LINKEDIT");

  // Unlike regular segments, __LINKEDIT member sizes can be computed in
  // parallel except __string_table and __code_signature sections.
  auto skip = [&](Chunk<E> *c) {
    return c == &ctx.strtab || c == ctx.code_sig.get();
  };

  tbb::parallel_for_each(chunks, [&](Chunk<E> *chunk) {
    if (!skip(chunk)) {
      Timer t2(ctx, std::string(chunk->hdr.get_sectname()), &t);
      chunk->compute_size(ctx);
    }
  });

  for (Chunk<E> *chunk : chunks) {
    fileoff = align_to(fileoff, 1 << chunk->hdr.p2align);
    vmaddr = align_to(vmaddr, 1 << chunk->hdr.p2align);

    chunk->hdr.offset = fileoff;
    chunk->hdr.addr = vmaddr;

    if (skip(chunk)) {
      Timer t2(ctx, std::string(chunk->hdr.get_sectname()), &t);
      chunk->compute_size(ctx);
    }

    fileoff += chunk->hdr.size;
    vmaddr += chunk->hdr.size;
  }

  cmd.vmsize = align_to(vmaddr - cmd.vmaddr, COMMON_PAGE_SIZE);
  cmd.filesize = fileoff - cmd.fileoff;
}

inline RebaseEncoder::RebaseEncoder() {
  buf.push_back(REBASE_OPCODE_SET_TYPE_IMM | REBASE_TYPE_POINTER);
}

inline void RebaseEncoder::add(i64 seg_idx, i64 offset) {
  assert(seg_idx < 16);

  // Accumulate consecutive base relocations
  if (seg_idx == cur_seg && offset == cur_off) {
    cur_off += 8;
    times++;
    return;
  }

  // Flush the accumulated base relocations
  flush();

  // Advance the cursor
  if (seg_idx != cur_seg || offset < cur_off) {
    buf.push_back(REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);
    encode_uleb(buf, offset);
  } else {
    i64 dist = offset - cur_off;
    assert(dist >= 0);

    if (dist % 8 == 0 && dist < 128) {
      buf.push_back(REBASE_OPCODE_ADD_ADDR_IMM_SCALED | (dist >> 3));
    } else {
      buf.push_back(REBASE_OPCODE_ADD_ADDR_ULEB);
      encode_uleb(buf, dist);
    }
  }

  cur_seg = seg_idx;
  cur_off = offset + 8;
  times = 1;
}

inline void RebaseEncoder::flush() {
  if (times == 0)
    return;

  if (times < 16) {
    buf.push_back(REBASE_OPCODE_DO_REBASE_IMM_TIMES | times);
  } else {
    buf.push_back(REBASE_OPCODE_DO_REBASE_ULEB_TIMES);
    encode_uleb(buf, times);
  }

  times = 0;
}

inline void RebaseEncoder::finish() {
  flush();
  buf.push_back(REBASE_OPCODE_DONE);
  buf.resize(align_to(buf.size(), 8));
}

template <typename E>
inline void RebaseSection<E>::compute_size(Context<E> &ctx) {
  RebaseEncoder enc;

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++)
    enc.add(ctx.data_seg->seg_idx,
            ctx.lazy_symbol_ptr.hdr.addr + i * word_size -
            ctx.data_seg->cmd.vmaddr);

  for (Symbol<E> *sym : ctx.got.syms)
    if (!sym->is_imported)
      enc.add(ctx.data_const_seg->seg_idx,
              sym->get_got_addr(ctx) - ctx.data_const_seg->cmd.vmaddr);

  for (Symbol<E> *sym : ctx.thread_ptrs.syms)
    if (!sym->is_imported)
      enc.add(ctx.data_seg->seg_idx,
              sym->get_tlv_addr(ctx) - ctx.data_seg->cmd.vmaddr);

  auto refers_tls = [](Symbol<E> *sym) {
    if (sym && sym->subsec) {
      auto ty = sym->subsec->isec.osec.hdr.type;
      return ty == S_THREAD_LOCAL_REGULAR || ty == S_THREAD_LOCAL_ZEROFILL ||
             ty == S_THREAD_LOCAL_VARIABLES;
    }
    return false;
  };

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->is_output_section)
        for (Subsection<E> *subsec : ((OutputSection<E> *)chunk)->members)
          for (Relocation<E> &rel : subsec->get_rels())
            if (!rel.is_pcrel && !rel.is_subtracted && rel.type == E::abs_rel &&
                !refers_tls(rel.sym))
              enc.add(seg->seg_idx,
                      subsec->get_addr(ctx) + rel.offset - seg->cmd.vmaddr);

  enc.finish();
  contents = std::move(enc.buf);
  this->hdr.size = contents.size();
}

template <typename E>
inline void RebaseSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->hdr.offset, contents);
}

inline BindEncoder::BindEncoder() {
  buf.push_back(BIND_OPCODE_SET_TYPE_IMM | BIND_TYPE_POINTER);
}

template <typename E>
static i32 get_dylib_idx(InputFile<E> *file) {
  if (file->is_dylib)
    return ((DylibFile<E> *)file)->dylib_idx;
  return BIND_SPECIAL_DYLIB_FLAT_LOOKUP;
}

template <typename E>
inline void BindEncoder::add(Symbol<E> &sym, i64 seg_idx, i64 offset, i64 addend) {
  i64 dylib_idx = get_dylib_idx(sym.file);
  i64 flags = (sym.is_weak ? BIND_SYMBOL_FLAGS_WEAK_IMPORT : 0);

  if (last_dylib != dylib_idx) {
    if (dylib_idx < 0) {
      buf.push_back(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM |
                    (dylib_idx & BIND_IMMEDIATE_MASK));
    } else if (dylib_idx < 16) {
      buf.push_back(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | dylib_idx);
    } else {
      buf.push_back(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
      encode_uleb(buf, dylib_idx);
    }
  }

  if (last_name != sym.name || last_flags != flags) {
    assert(flags < 16);
    buf.push_back(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
    buf.insert(buf.end(), (u8 *)sym.name.data(),
               (u8 *)(sym.name.data() + sym.name.size()));
    buf.push_back('\0');
  }

  if (last_seg != seg_idx || last_offset != offset) {
    assert(seg_idx < 16);
    buf.push_back(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);
    encode_uleb(buf, offset);
  }

  if (last_addend != addend) {
    buf.push_back(BIND_OPCODE_SET_ADDEND_SLEB);
    encode_sleb(buf, addend);
  }

  buf.push_back(BIND_OPCODE_DO_BIND);

  last_dylib = dylib_idx;
  last_name = sym.name;
  last_flags = flags;
  last_seg = seg_idx;
  last_offset = offset;
  last_addend = addend;
}

inline void BindEncoder::finish() {
  buf.push_back(BIND_OPCODE_DONE);
  buf.resize(align_to(buf.size(), 8));
}

template <typename E>
void BindSection<E>::compute_size(Context<E> &ctx) {
  BindEncoder enc;

  for (Symbol<E> *sym : ctx.got.syms)
    if (sym->is_imported)
      enc.add(*sym, ctx.data_const_seg->seg_idx,
              sym->get_got_addr(ctx) - ctx.data_const_seg->cmd.vmaddr, 0);

  for (Symbol<E> *sym : ctx.thread_ptrs.syms)
    if (sym->is_imported)
      enc.add(*sym, ctx.data_seg->seg_idx,
              sym->get_tlv_addr(ctx) - ctx.data_seg->cmd.vmaddr, 0);

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->is_output_section)
        for (Subsection<E> *subsec : ((OutputSection<E> *)chunk)->members)
          for (Relocation<E> &r : subsec->get_rels())
            if (r.needs_dynrel)
              enc.add(*r.sym, seg->seg_idx,
                      subsec->get_addr(ctx) + r.offset - seg->cmd.vmaddr,
                      r.addend);

  enc.finish();
  contents = std::move(enc.buf);
  this->hdr.size = contents.size();
}

template <typename E>
void BindSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->hdr.offset, contents);
}

template <typename E>
void LazyBindSection<E>::add(Context<E> &ctx, Symbol<E> &sym) {
  auto emit = [&](u8 byte) {
    contents.push_back(byte);
  };

  i64 dylib_idx = get_dylib_idx(sym.file);

  if (dylib_idx < 0) {
    emit(BIND_OPCODE_SET_DYLIB_SPECIAL_IMM | (dylib_idx & BIND_IMMEDIATE_MASK));
  } else if (dylib_idx < 16) {
    emit(BIND_OPCODE_SET_DYLIB_ORDINAL_IMM | dylib_idx);
  } else {
    emit(BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB);
    encode_uleb(contents, dylib_idx);
  }

  i64 flags = (sym.is_weak ? BIND_SYMBOL_FLAGS_WEAK_IMPORT : 0);
  assert(flags < 16);

  emit(BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM | flags);
  contents.insert(contents.end(), (u8 *)sym.name.data(),
                  (u8 *)(sym.name.data() + sym.name.size()));
  emit('\0');

  i64 seg_idx = ctx.data_seg->seg_idx;
  emit(BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB | seg_idx);

  i64 offset = ctx.lazy_symbol_ptr.hdr.addr + sym.stub_idx * word_size -
               ctx.data_seg->cmd.vmaddr;
  encode_uleb(contents, offset);

  emit(BIND_OPCODE_DO_BIND);
  emit(BIND_OPCODE_DONE);
}

template <typename E>
void LazyBindSection<E>::compute_size(Context<E> &ctx) {
  ctx.stubs.bind_offsets.clear();

  for (Symbol<E> *sym : ctx.stubs.syms) {
    ctx.stubs.bind_offsets.push_back(contents.size());
    add(ctx, *sym);
  }

  contents.resize(align_to(contents.size(), 1 << this->hdr.p2align));
  this->hdr.size = contents.size();
}

template <typename E>
void LazyBindSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->hdr.offset, contents);
}

inline i64 ExportEncoder::finish() {
  tbb::parallel_sort(entries, [](const Entry &a, const Entry &b) {
    return a.name < b.name;
  });

  // Construct a trie
  TrieNode node;
  tbb::task_group tg;
  construct_trie(node, entries, 0, &tg, entries.size() / 32, true);
  tg.wait();

  if (node.prefix.empty())
    root = std::move(node);
  else
    root.children.emplace_back(new TrieNode(std::move(node)));

  // Set output offsets to trie nodes. Since a serialized trie node
  // contains output offsets of other nodes in the variable-length
  // ULEB format, it unfortunately needs more than one iteration.
  // We need to repeat until the total size of the serialized trie
  // converges to obtain the optimized output. However, in reality,
  // repeating this step twice is enough. Size reduction on third and
  // further iterations is negligible.
  set_offset(root, 0);
  return set_offset(root, 0);
}

static i64 common_prefix_len(std::string_view x, std::string_view y) {
  i64 i = 0;
  while (i < x.size() && i < y.size() && x[i] == y[i])
    i++;
  return i;
}

void
inline ExportEncoder::construct_trie(TrieNode &node, std::span<Entry> entries,
                                     i64 len, tbb::task_group *tg,
                                     i64 grain_size, bool divide) {
  i64 new_len = common_prefix_len(entries[0].name, entries.back().name);

  if (new_len > len) {
    node.prefix = entries[0].name.substr(len, new_len - len);
    if (entries[0].name.size() == new_len) {
      node.is_leaf = true;
      node.flags = entries[0].flags;
      node.addr = entries[0].addr;
      entries = entries.subspan(1);
    }
  }

  for (i64 i = 0; i < entries.size();) {
    auto it = std::partition_point(entries.begin() + i + 1, entries.end(),
                                   [&](const Entry &ent) {
      return entries[i].name[new_len] == ent.name[new_len];
    });
    i64 j = it - entries.begin();

    TrieNode *child = new TrieNode;
    std::span<Entry> subspan = entries.subspan(i, j - i);

    if (divide && j - i < grain_size) {
      tg->run([=, this] {
        construct_trie(*child, subspan, new_len, tg, grain_size, false);
      });
    } else {
      construct_trie(*child, subspan, new_len, tg, grain_size, divide);
    }

    node.children.emplace_back(child);
    i = j;
  }
}

inline i64 ExportEncoder::set_offset(TrieNode &node, i64 offset) {
  node.offset = offset;

  i64 size = 0;
  if (node.is_leaf) {
    size = uleb_size(node.flags) + uleb_size(node.addr);
    size += uleb_size(size);
  } else {
    size = 1;
  }

  size++; // # of children

  for (std::unique_ptr<TrieNode> &child : node.children) {
    // +1 for NUL byte
    size += child->prefix.size() + 1 + uleb_size(child->offset);
  }

  for (std::unique_ptr<TrieNode> &child : node.children)
    size += set_offset(*child, offset + size);
  return size;
}

inline void ExportEncoder::write_trie(u8 *start, TrieNode &node) {
  u8 *buf = start + node.offset;

  if (node.is_leaf) {
    buf += write_uleb(buf, uleb_size(node.flags) + uleb_size(node.addr));
    buf += write_uleb(buf, node.flags);
    buf += write_uleb(buf, node.addr);
  } else {
    *buf++ = 0;
  }

  *buf++ = node.children.size();

  for (std::unique_ptr<TrieNode> &child : node.children) {
    buf += write_string(buf, child->prefix);
    buf += write_uleb(buf, child->offset);
  }

  for (std::unique_ptr<TrieNode> &child : node.children)
    write_trie(start, *child);
}

template <typename E>
void ExportSection<E>::compute_size(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs)
    for (Symbol<E> *sym : file->syms)
      if (sym && sym->file == file && sym->scope == SCOPE_EXTERN)
        enc.entries.push_back({
            sym->name,
            sym->is_weak ? EXPORT_SYMBOL_FLAGS_WEAK_DEFINITION : 0,
            sym->get_addr(ctx) - ctx.arg.pagezero_size});

  if (enc.entries.empty())
    return;

  this->hdr.size = align_to(enc.finish(), 8);
}

template <typename E>
void ExportSection<E>::copy_buf(Context<E> &ctx) {
  if (this->hdr.size == 0)
    return;

  u8 *buf = ctx.buf + this->hdr.offset;
  memset(buf, 0, this->hdr.size);
  enc.write_trie(buf, enc.root);
}

// LC_FUNCTION_STARTS contains function start addresses encoded in
// ULEB128. I don't know what tools consume this table, but we create
// it anyway by default for the sake of compatibility.
template <typename E>
void FunctionStartsSection<E>::compute_size(Context<E> &ctx) {
  std::vector<std::vector<u64>> vec(ctx.objs.size());

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    ObjectFile<E> &file = *ctx.objs[i];
    for (Symbol<E> *sym : file.syms)
      if (sym && sym->file == &file && sym->subsec && sym->subsec->is_alive &&
          &sym->subsec->isec.osec == ctx.text)
        vec[i].push_back(sym->get_addr(ctx));
  });

  std::vector<u64> addrs = flatten(vec);
  tbb::parallel_sort(addrs.begin(), addrs.end());

  contents.resize(addrs.size() * 5);

  u8 *p = contents.data();
  u64 last = ctx.arg.pagezero_size;

  for (u64 val : addrs) {
    p += write_uleb(p, val - last);
    last = val;
  }

  this->hdr.size = p - contents.data();
  contents.resize(this->hdr.size);
}

template <typename E>
void FunctionStartsSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->hdr.offset, contents);
}

template <typename E>
void SymtabSection<E>::compute_size(Context<E> &ctx) {
  symtab_offsets.clear();
  symtab_offsets.resize(ctx.objs.size() + ctx.dylibs.size() + 1);

  strtab_offsets.clear();
  strtab_offsets.resize(ctx.objs.size() + ctx.dylibs.size() + 1);
  strtab_offsets[0] = 1;

  tbb::enumerable_thread_specific<i64> locals;
  tbb::enumerable_thread_specific<i64> globals;
  tbb::enumerable_thread_specific<i64> undefs;

  // Calculate the sizes for -add_ast_path symbols.
  locals.local() += ctx.arg.add_ast_path.size();
  symtab_offsets[0] += ctx.arg.add_ast_path.size();
  for (std::string_view s : ctx.arg.add_ast_path)
    strtab_offsets[0] += s.size() + 1;

  // Calculate the sizes required for symbols in input files.
  auto count = [&](Symbol<E> *sym) {
    if (sym->is_imported)
      undefs.local() += 1;
    else if (sym->scope == SCOPE_EXTERN)
      globals.local() += 1;
    else
      locals.local() += 1;
  };

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    ObjectFile<E> &file = *ctx.objs[i];
    for (Symbol<E> *sym : file.syms) {
      if (sym && sym->file == &file && (!sym->subsec || sym->subsec->is_alive)) {
        symtab_offsets[i + 1]++;
        strtab_offsets[i + 1] += sym->name.size() + 1;
        count(sym);
      }
    }
  });

  tbb::parallel_for((i64)0, (i64)ctx.dylibs.size(), [&](i64 i) {
    DylibFile<E> &file = *ctx.dylibs[i];
    for (Symbol<E> *sym : file.syms) {
      if (sym && sym->file == &file &&
          (sym->stub_idx != -1 || sym->got_idx != -1)) {
        symtab_offsets[i + 1 + ctx.objs.size()]++;
        strtab_offsets[i + 1 + ctx.objs.size()] += sym->name.size() + 1;
        count(sym);
      }
    }
  });

  num_locals = locals.combine(std::plus());
  num_globals = globals.combine(std::plus());
  num_undefs = undefs.combine(std::plus());

  for (i64 i = 1; i < symtab_offsets.size(); i++)
    symtab_offsets[i] += symtab_offsets[i - 1];

  for (i64 i = 1; i < strtab_offsets.size(); i++)
    strtab_offsets[i] += strtab_offsets[i - 1];

  this->hdr.size = symtab_offsets.back() * sizeof(MachSym);
  ctx.strtab.hdr.size = strtab_offsets.back();
}

template <typename E>
void SymtabSection<E>::copy_buf(Context<E> &ctx) {
  MachSym *buf = (MachSym *)(ctx.buf + this->hdr.offset);

  u8 *strtab = ctx.buf + ctx.strtab.hdr.offset;
  strtab[0] = '\0';

  // Create symbols for -add_ast_path
  {
    i64 symoff = 0;
    i64 stroff = 1;
    for (std::string_view s : ctx.arg.add_ast_path) {
      MachSym &msym = buf[symoff++];
      msym.stroff = stroff;
      msym.n_type = N_AST;

      write_string(strtab + stroff, s);
      stroff += s.size() + 1;
    }
  }

  // Copy symbols from input files to an output file
  Symbol<E> *mh_execute_header = get_symbol(ctx, "__mh_execute_header");
  Symbol<E> *dyld_private = get_symbol(ctx, "__dyld_private");
  Symbol<E> *mh_dylib_header = get_symbol(ctx, "__mh_dylib_header");
  Symbol<E> *mh_bundle_header = get_symbol(ctx, "__mh_bundle_header");
  Symbol<E> *dso_handle = get_symbol(ctx, "___dso_handle");

  auto write = [&](Symbol<E> &sym, i64 symoff, i64 stroff) {
    MachSym &msym = buf[symoff];

    msym.stroff = stroff;
    write_string(strtab + stroff, sym.name);

    msym.is_extern = (sym.is_imported || sym.scope == SCOPE_EXTERN);
    msym.type = (sym.is_imported ? N_UNDF : N_SECT);

    if (sym.is_imported)
      msym.sect = N_UNDF;
    else if (sym.subsec)
      msym.sect = sym.subsec->isec.osec.sect_idx;
    else if (&sym == mh_execute_header)
      msym.sect = ctx.text->sect_idx;
    else if (&sym == dyld_private || &sym == mh_dylib_header ||
             &sym == mh_bundle_header || &sym == dso_handle)
      msym.sect = ctx.data->sect_idx;
    else
      msym.sect = N_ABS;

    if (sym.file->is_dylib)
      msym.desc = ((DylibFile<E> *)sym.file)->dylib_idx << 8;
    else if (sym.is_imported)
      msym.desc = DYNAMIC_LOOKUP_ORDINAL << 8;
    else if (sym.referenced_dynamically)
      msym.desc = REFERENCED_DYNAMICALLY;

    if (!sym.is_imported && (!sym.subsec || sym.subsec->is_alive))
      msym.value = sym.get_addr(ctx);
  };

  tbb::parallel_for((i64)0, (i64)ctx.objs.size(), [&](i64 i) {
    ObjectFile<E> &file = *ctx.objs[i];
    i64 symoff = symtab_offsets[i];
    i64 stroff = strtab_offsets[i];

    for (Symbol<E> *sym : file.syms) {
      if (sym && sym->file == &file && (!sym->subsec || sym->subsec->is_alive)) {
        write(*sym, symoff, stroff);
        symoff++;
        stroff += sym->name.size() + 1;
      }
    }
  });

  tbb::parallel_for((i64)0, (i64)ctx.dylibs.size(), [&](i64 i) {
    DylibFile<E> &file = *ctx.dylibs[i];
    i64 symoff = symtab_offsets[i + ctx.objs.size()];
    i64 stroff = strtab_offsets[i + ctx.objs.size()];

    for (Symbol<E> *sym : file.syms) {
      if (sym && sym->file == &file &&
          (sym->stub_idx != -1 || sym->got_idx != -1)) {
        write(*sym, symoff, stroff);
        symoff++;
        stroff += sym->name.size() + 1;
      }
    }
  });

  auto get_rank = [](const MachSym &msym) {
    if (msym.sect == N_UNDF)
      return 2;
    if (msym.is_extern)
      return 1;
    return 0;
  };

  std::stable_sort(buf, buf + this->hdr.size / sizeof(MachSym),
       [&](const MachSym &a, const MachSym &b) {
     return std::tuple{get_rank(a), a.value} < std::tuple{get_rank(b), b.value};
  });
}

template <typename E>
static bool has_objc_image_info_section(Context<E> &ctx) {
  return false;
}

// Create __DATA,__objc_imageinfo section contents by merging input
// __objc_imageinfo sections.
template <typename E>
std::unique_ptr<ObjcImageInfoSection<E>>
ObjcImageInfoSection<E>::create(Context<E> &ctx) {
  ObjcImageInfo *first = nullptr;

  for (ObjectFile<E> *file : ctx.objs) {
    if (file->objc_image_info) {
      first = file->objc_image_info;
      break;
    }
  }

  if (!first)
    return nullptr;

  ObjcImageInfo info;
  info.flags = (first->flags & OBJC_IMAGE_HAS_CATEGORY_CLASS_PROPERTIES);

  for (ObjectFile<E> *file : ctx.objs) {
    if (!file->objc_image_info)
      continue;

    ObjcImageInfo &info2 = *file->objc_image_info;

    // Make sure that all object files have the same flag.
    if ((info.flags & OBJC_IMAGE_HAS_CATEGORY_CLASS_PROPERTIES) !=
        (info2.flags & OBJC_IMAGE_HAS_CATEGORY_CLASS_PROPERTIES))
      Error(ctx) << *file << ": incompatible __objc_imageinfo flag";

    // Make sure that all object files have the same Swift version.
    if (info.swift_version == 0)
      info.swift_version = info2.swift_version;

    if (info.swift_version != info2.swift_version && info2.swift_version != 0)
      Error(ctx) << *file << ": incompatible __objc_imageinfo swift version"
                 << (u32)info.swift_version << " " << (u32)info2.swift_version;

    // swift_lang_version is set to the newest.
    info.swift_lang_version =
      std::max<u32>(info.swift_lang_version, info2.swift_lang_version);
  }

  return std::make_unique<ObjcImageInfoSection<E>>(ctx, info);
}

template <typename E>
void ObjcImageInfoSection<E>::copy_buf(Context<E> &ctx) {
  memcpy(ctx.buf + this->hdr.offset, &contents, sizeof(contents));
}

template <typename E>
void CodeSignatureSection<E>::compute_size(Context<E> &ctx) {
  std::string filename = filepath(ctx.arg.final_output).filename().string();
  i64 filename_size = align_to(filename.size() + 1, 16);
  i64 num_blocks = align_to(this->hdr.offset, E::page_size) / E::page_size;
  this->hdr.size = sizeof(CodeSignatureHeader) + sizeof(CodeSignatureBlobIndex) +
                   sizeof(CodeSignatureDirectory) + filename_size +
                   num_blocks * SHA256_SIZE;
}

// A __code_signature section is optional for x86 macOS but mandatory
// for ARM macOS. The section contains a cryptographic hash for each
// memory page of an executable or a dylib file. The program loader
// verifies the hash values on the initial execution of a binary and
// will reject it if a hash value does not match.
template <typename E>
void CodeSignatureSection<E>::write_signature(Context<E> &ctx) {
  Timer t(ctx, "write_signature");

  u8 *buf = ctx.buf + this->hdr.offset;
  memset(buf, 0, this->hdr.size);

  std::string filename = filepath(ctx.arg.final_output).filename().string();
  i64 filename_size = align_to(filename.size() + 1, 16);
  i64 num_blocks = align_to(this->hdr.offset, E::page_size) / E::page_size;

  // Fill code-sign header fields
  CodeSignatureHeader &sighdr = *(CodeSignatureHeader *)buf;
  buf += sizeof(sighdr);

  sighdr.magic = CSMAGIC_EMBEDDED_SIGNATURE;
  sighdr.length = this->hdr.size;
  sighdr.count = 1;

  CodeSignatureBlobIndex &idx = *(CodeSignatureBlobIndex *)buf;
  buf += sizeof(idx);

  idx.type = CSSLOT_CODEDIRECTORY;
  idx.offset = sizeof(sighdr) + sizeof(idx);

  CodeSignatureDirectory &dir = *(CodeSignatureDirectory *)buf;
  buf += sizeof(dir);

  dir.magic = CSMAGIC_CODEDIRECTORY;
  dir.length = sizeof(dir) + filename_size + num_blocks * SHA256_SIZE;
  dir.version = CS_SUPPORTSEXECSEG;
  dir.flags = CS_ADHOC | CS_LINKER_SIGNED;
  dir.hash_offset = sizeof(dir) + filename_size;
  dir.ident_offset = sizeof(dir);
  dir.n_code_slots = num_blocks;
  dir.code_limit = this->hdr.offset;
  dir.hash_size = SHA256_SIZE;
  dir.hash_type = CS_HASHTYPE_SHA256;
  dir.page_size = std::countr_zero(E::page_size);
  dir.exec_seg_base = ctx.text_seg->cmd.fileoff;
  dir.exec_seg_limit = ctx.text_seg->cmd.filesize;
  if (ctx.output_type == MH_EXECUTE)
    dir.exec_seg_flags = CS_EXECSEG_MAIN_BINARY;

  memcpy(buf, filename.data(), filename.size());
  buf += filename_size;

  // Compute a hash value for each block.
  auto compute_hash = [&](i64 i) {
    u8 *start = ctx.buf + i * E::page_size;
    u8 *end = ctx.buf + std::min<i64>((i + 1) * E::page_size, this->hdr.offset);
    sha256_hash(start, end - start, buf + i * SHA256_SIZE);
  };

  for (i64 i = 0; i < num_blocks; i += 1024) {
    i64 j = std::min(num_blocks, i + 1024);
    tbb::parallel_for(i, j, compute_hash);

#if __APPLE__
    // Calling msync() with MS_ASYNC speeds up the following msync()
    // with MS_INVALIDATE.
    msync(ctx.buf + i * E::page_size, 1024 * E::page_size, MS_ASYNC);
#endif
  }

  // A LC_UUID load command may also contain a crypto hash of the
  // entire file. We compute its value as a tree hash.
  if (ctx.arg.uuid == UUID_HASH) {
    u8 uuid[SHA256_SIZE];
    sha256_hash(ctx.buf + this->hdr.offset, this->hdr.size, uuid);

    // Indicate that this is UUIDv4 as defined by RFC4122.
    uuid[6] = (uuid[6] & 0b00001111) | 0b01010000;
    uuid[8] = (uuid[8] & 0b00111111) | 0b10000000;

    memcpy(ctx.uuid, uuid, 16);

    // Rewrite the load commands to write the updated UUID and
    // recompute code signatures for the updated blocks.
    ctx.mach_hdr.copy_buf(ctx);

    for (i64 i = 0; i * E::page_size < ctx.mach_hdr.hdr.size; i++)
      compute_hash(i);
  }

#if __APPLE__
  // If an executable's pages have been created via an mmap(), the output
  // file will fail for the code signature verification because the macOS
  // kernel wrongly assume that the pages may be mutable after the code
  // verification, though it is actually impossible after munmap().
  //
  // In order to workaround the issue, we call msync() to invalidate all
  // mmapped pages.
  //
  // https://openradar.appspot.com/FB8914231
  Timer t2(ctx, "msync", &t);
  msync(ctx.buf, ctx.output_file->filesize, MS_INVALIDATE);
#endif
}

template <typename E>
void DataInCodeSection<E>::compute_size(Context<E> &ctx) {
  assert(contents.empty());

  for (ObjectFile<E> *file : ctx.objs) {
    std::span<DataInCodeEntry> entries = file->data_in_code_entries;

    for (Subsection<E> *subsec : file->subsections) {
      if (entries.empty())
        break;

      DataInCodeEntry &ent = entries[0];
      if (subsec->input_addr + subsec->input_size < ent.offset)
        continue;

      if (ent.offset < subsec->input_addr + subsec->input_size) {
        u32 offset = subsec->get_addr(ctx) + subsec->input_addr - ent.offset -
                     ctx.text_seg->cmd.vmaddr;
        contents.push_back({offset, ent.length, ent.kind});
      }

      entries = entries.subspan(1);
    }
  }

  this->hdr.size = contents.size() * sizeof(contents[0]);
}

template <typename E>
void DataInCodeSection<E>::copy_buf(Context<E> &ctx) {
  write_vector(ctx.buf + this->hdr.offset, contents);
}

template <typename E>
void StubsSection<E>::add(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->stub_idx == -1);
  sym->stub_idx = syms.size();

  syms.push_back(sym);

  i64 nsyms = syms.size();
  this->hdr.size = nsyms * E::stub_size;

  ctx.stub_helper.hdr.size =
    E::stub_helper_hdr_size + nsyms * E::stub_helper_size;
  ctx.lazy_symbol_ptr.hdr.size = nsyms * word_size;
}

template <typename E>
class UnwindEncoder {
public:
  std::vector<u8> encode(Context<E> &ctx, std::span<UnwindRecord<E>> records);
  u32 encode_personality(Context<E> &ctx, u64 addr);

  std::vector<std::span<UnwindRecord<E>>>
  split_records(Context<E> &ctx, std::span<UnwindRecord<E>> records);

  std::vector<u64> personalities;
};

template <typename E>
std::vector<u8>
UnwindEncoder<E>::encode(Context<E> &ctx, std::span<UnwindRecord<E>> records) {
  i64 num_lsda = 0;

  for (UnwindRecord<E> &rec : records) {
    if (rec.personality)
      rec.encoding |= encode_personality(ctx, rec.personality->get_got_addr(ctx));
    if (rec.lsda)
      num_lsda++;
  }

  std::vector<std::span<UnwindRecord<E>>> pages = split_records(ctx, records);

  // Compute the size of the buffer.
  i64 size = sizeof(UnwindSectionHeader) +
             personalities.size() * 4 +
             sizeof(UnwindFirstLevelPage) * (pages.size() + 1) +
             sizeof(UnwindSecondLevelPage) * pages.size() +
             (sizeof(UnwindPageEntry) + 4) * records.size() +
             sizeof(UnwindLsdaEntry) * num_lsda;

  // Allocate an output buffer.
  std::vector<u8> buf(size);

  // Write the section header.
  UnwindSectionHeader &uhdr = *(UnwindSectionHeader *)buf.data();
  uhdr.version = UNWIND_SECTION_VERSION;
  uhdr.encoding_offset = sizeof(uhdr);
  uhdr.encoding_count = 0;
  uhdr.personality_offset = sizeof(uhdr);
  uhdr.personality_count = personalities.size();
  uhdr.page_offset = sizeof(uhdr) + personalities.size() * 4;
  uhdr.page_count = pages.size() + 1;

  // Write the personalities
  u32 *per = (u32 *)(buf.data() + sizeof(uhdr));
  for (u64 addr : personalities)
    *per++ = addr;

  // Write first level pages, LSDA and second level pages
  UnwindFirstLevelPage *page1 = (UnwindFirstLevelPage *)per;
  UnwindLsdaEntry *lsda = (UnwindLsdaEntry *)(page1 + (pages.size() + 1));
  UnwindSecondLevelPage *page2 = (UnwindSecondLevelPage *)(lsda + num_lsda);

  for (std::span<UnwindRecord<E>> span : pages) {
    page1->func_addr = span[0].get_func_addr(ctx);
    page1->page_offset = (u8 *)page2 - buf.data();
    page1->lsda_offset = (u8 *)lsda - buf.data();

    for (UnwindRecord<E> &rec : span) {
      if (rec.lsda) {
        lsda->func_addr = rec.get_func_addr(ctx);
        lsda->lsda_addr = rec.lsda->get_addr(ctx) + rec.lsda_offset;
        lsda++;
      }
    }

    std::unordered_map<u32, u32> map;
    for (UnwindRecord<E> &rec : span)
      map.insert({rec.encoding, map.size()});

    page2->kind = UNWIND_SECOND_LEVEL_COMPRESSED;
    page2->page_offset = sizeof(UnwindSecondLevelPage);
    page2->page_count = span.size();

    UnwindPageEntry *entry = (UnwindPageEntry *)(page2 + 1);
    for (UnwindRecord<E> &rec : span) {
      entry->func_addr = rec.get_func_addr(ctx) - page1->func_addr;
      entry->encoding = map[rec.encoding];
      entry++;
    }

    page2->encoding_offset = (u8 *)entry - (u8 *)page2;
    page2->encoding_count = map.size();

    u32 *encoding = (u32 *)entry;
    for (std::pair<u32, u32> kv : map)
      encoding[kv.second] = kv.first;

    page1++;
    page2 = (UnwindSecondLevelPage *)(encoding + map.size());
  }

  // Write a terminator
  UnwindRecord<E> &last = records[records.size() - 1];
  page1->func_addr = last.subsec->get_addr(ctx) + last.subsec->input_size + 1;
  page1->page_offset = 0;
  page1->lsda_offset = (u8 *)lsda - buf.data();

  buf.resize((u8 *)page2 - buf.data());
  return buf;
}

template <typename E>
u32 UnwindEncoder<E>::encode_personality(Context<E> &ctx, u64 addr) {
  for (i64 i = 0; i < personalities.size(); i++)
    if (personalities[i] == addr)
      return (i + 1) << std::countr_zero(UNWIND_PERSONALITY_MASK);

  if (personalities.size() == 3)
    Fatal(ctx) << "too many personality functions";

  personalities.push_back(addr);
  return personalities.size() << std::countr_zero(UNWIND_PERSONALITY_MASK);
}

template <typename E>
std::vector<std::span<UnwindRecord<E>>>
UnwindEncoder<E>::split_records(Context<E> &ctx,
                                std::span<UnwindRecord<E>> records) {
  constexpr i64 max_group_size = 4096;

  sort(records, [&](const UnwindRecord<E> &a, const UnwindRecord<E> &b) {
    return a.get_func_addr(ctx) < b.get_func_addr(ctx);
  });

  std::vector<std::span<UnwindRecord<E>>> vec;

  while (!records.empty()) {
    u64 end_addr = records[0].get_func_addr(ctx) + (1 << 24);
    i64 i = 1;
    while (i < records.size() && i < max_group_size &&
           records[i].get_func_addr(ctx) < end_addr)
      i++;
    vec.push_back(records.subspan(0, i));
    records = records.subspan(i);
  }
  return vec;
}

template <typename E>
static std::vector<u8> construct_unwind_info(Context<E> &ctx) {
  std::vector<UnwindRecord<E>> records;

  for (std::unique_ptr<OutputSegment<E>> &seg : ctx.segments)
    for (Chunk<E> *chunk : seg->chunks)
      if (chunk->is_output_section)
        for (Subsection<E> *subsec : ((OutputSection<E> *)chunk)->members)
          for (UnwindRecord<E> &rec : subsec->get_unwind_records())
            records.push_back(rec);

  if (records.empty())
    return {};
  return UnwindEncoder<E>().encode(ctx, records);
}

template <typename E>
void UnwindInfoSection<E>::compute_size(Context<E> &ctx) {
  this->hdr.size = construct_unwind_info(ctx).size();
}

template <typename E>
void UnwindInfoSection<E>::copy_buf(Context<E> &ctx) {
  std::vector<u8> vec = construct_unwind_info(ctx);
  assert(this->hdr.size == vec.size());
  write_vector(ctx.buf + this->hdr.offset, vec);
}

template <typename E>
void GotSection<E>::add(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->got_idx == -1);
  sym->got_idx = syms.size();
  syms.push_back(sym);
  this->hdr.size = (syms.size() + subsections.size()) * word_size;
}

template <typename E>
void GotSection<E>::copy_buf(Context<E> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->hdr.offset);

  for (i64 i = 0; i < syms.size(); i++)
    if (!syms[i]->is_imported)
      buf[i] = syms[i]->get_addr(ctx);

  for (i64 i = 0; i < subsections.size(); i++)
    buf[i + syms.size()] = subsections[i]->get_addr(ctx);
}

template <typename E>
void LazySymbolPtrSection<E>::copy_buf(Context<E> &ctx) {
  u64 *buf = (u64 *)(ctx.buf + this->hdr.offset);

  for (i64 i = 0; i < ctx.stubs.syms.size(); i++)
    buf[i] = ctx.stub_helper.hdr.addr + E::stub_helper_hdr_size +
             i * E::stub_helper_size;
}

template <typename E>
void ThreadPtrsSection<E>::add(Context<E> &ctx, Symbol<E> *sym) {
  assert(sym->tlv_idx == -1);
  sym->tlv_idx = syms.size();
  syms.push_back(sym);
  this->hdr.size = syms.size() * word_size;
}

template <typename E>
void ThreadPtrsSection<E>::copy_buf(Context<E> &ctx) {
  ul64 *buf = (ul64 *)(ctx.buf + this->hdr.offset);
  memset(buf, 0, this->hdr.size);

  for (i64 i = 0; i < syms.size(); i++)
    if (Symbol<E> &sym = *syms[i]; !sym.is_imported)
      buf[i] = sym.get_addr(ctx);
}

template <typename E>
SectCreateSection<E>::SectCreateSection(Context<E> &ctx, std::string_view seg,
                                        std::string_view sect,
                                        std::string_view contents)
  : Chunk<E>(ctx, seg, sect), contents(contents) {
  this->hdr.size = contents.size();
  ctx.chunk_pool.emplace_back(this);
}

template <typename E>
void SectCreateSection<E>::copy_buf(Context<E> &ctx) {
  write_string(ctx.buf + this->hdr.offset, contents);
}

using E = MOLD_TARGET;

template class OutputSegment<E>;
template class OutputMachHeader<E>;
template class OutputSection<E>;
template class RebaseSection<E>;
template class BindSection<E>;
template class LazyBindSection<E>;
template class ExportSection<E>;
template class FunctionStartsSection<E>;
template class SymtabSection<E>;
template class StrtabSection<E>;
template class CodeSignatureSection<E>;
template class ObjcImageInfoSection<E>;
template class DataInCodeSection<E>;
template class StubsSection<E>;
template class StubHelperSection<E>;
template class UnwindInfoSection<E>;
template class GotSection<E>;
template class LazySymbolPtrSection<E>;
template class ThreadPtrsSection<E>;
template class SectCreateSection<E>;

} // namespace mold::macho
