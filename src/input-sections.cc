#include "mold.h"

#include <zlib.h>
#include <zstd.h>

namespace mold {

static i64 to_p2align(u64 alignment) {
  if (alignment == 0)
    return 0;
  return std::countr_zero(alignment);
}

template <typename E>
bool cie_equals(const CieRecord<E> &a, const CieRecord<E> &b) {
  if (a.get_contents() != b.get_contents())
    return false;

  std::span<const ElfRel<E>> x = a.get_rels();
  std::span<const ElfRel<E>> y = b.get_rels();
  if (x.size() != y.size())
    return false;

  for (i64 i = 0; i < x.size(); i++)
    if (x[i].r_offset - a.input_offset != y[i].r_offset - b.input_offset ||
        x[i].r_type != y[i].r_type ||
        a.file.symbols[x[i].r_sym] != b.file.symbols[y[i].r_sym] ||
        get_addend(a.input_section, x[i]) != get_addend(b.input_section, y[i]))
      return false;
  return true;
}

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file, i64 shndx)
  : file(file), shndx(shndx) {
  if (shndx < file.elf_sections.size())
    contents = {(char *)file.mf->data + shdr().sh_offset, (size_t)shdr().sh_size};

  if (shdr().sh_flags & SHF_COMPRESSED) {
    ElfChdr<E> &chdr = *(ElfChdr<E> *)&contents[0];
    sh_size = chdr.ch_size;
    p2align = to_p2align(chdr.ch_addralign);
  } else {
    sh_size = shdr().sh_size;
    p2align = to_p2align(shdr().sh_addralign);
  }

  // Sections may have been compressed. We usually uncompress them
  // directly into the mmap'ed output file, but we want to uncompress
  // early for REL-type ELF types to read relocation addends from
  // section contents. For RELA-type, we don't need to do this because
  // addends are in relocations.
  //
  // SH-4 stores addends to sections despite being RELA, which is a
  // special (and buggy) case.
  if constexpr (!E::is_rela || is_sh4<E>)
    uncompress(ctx);
}

template <typename E>
void InputSection<E>::uncompress(Context<E> &ctx) {
  if (!(shdr().sh_flags & SHF_COMPRESSED) || uncompressed)
    return;

  u8 *buf = new u8[sh_size];
  copy_contents_to(ctx, buf, sh_size);
  contents = std::string_view((char *)buf, sh_size);
  ctx.string_pool.emplace_back(buf);
  uncompressed = true;
}

template <typename E>
void InputSection<E>::copy_contents_to(Context<E> &ctx, u8 *buf, i64 sz) {
  if (!(shdr().sh_flags & SHF_COMPRESSED) || uncompressed) {
    memcpy(buf, contents.data(), sz);
    return;
  }

  if (contents.size() < sizeof(ElfChdr<E>))
    Fatal(ctx) << *this << ": corrupted compressed section";

  ElfChdr<E> &hdr = *(ElfChdr<E> *)&contents[0];
  std::string_view data = contents.substr(sizeof(ElfChdr<E>));

  switch (hdr.ch_type) {
  case ELFCOMPRESS_ZLIB: {
    z_stream s = {};
    inflateInit(&s);
    s.next_in = (u8 *)data.data();
    s.avail_in = data.size();
    s.next_out = buf;
    s.avail_out = sz;

    int r;
    while (s.total_out < sz && (r = inflate(&s, Z_NO_FLUSH)) == Z_OK);
    if (s.total_out < sz && r != Z_STREAM_END)
      Fatal(ctx) << *this << ": uncompress failed: " << s.msg;
    inflateEnd(&s);
    break;
  }
  case ELFCOMPRESS_ZSTD: {
    ZSTD_DCtx *dctx = ZSTD_createDCtx();
    ZSTD_inBuffer in = { data.data(), data.size() };
    ZSTD_outBuffer out = { buf, (size_t)sz };

    while (out.pos < out.size) {
      size_t r = ZSTD_decompressStream(dctx, &out, &in);
      if (ZSTD_isError(r))
        Fatal(ctx) << *this << ": uncompress failed: " << ZSTD_getErrorName(r);
      if (r == 0 && out.pos < out.size)
        Fatal(ctx) << *this << ": uncompress failed: premature end of input";
    }
    ZSTD_freeDCtx(dctx);
    break;
  }
  default:
    Fatal(ctx) << *this << ": unsupported compression type: 0x"
               << std::hex << hdr.ch_type;
  }

  msan_unpoison(buf, sz);
}

typedef enum : u8 { NONE, ERROR, COPYREL, PLT, CPLT } Action;

template <typename E>
static void do_action(Context<E> &ctx, Action action, InputSection<E> &isec,
                      Symbol<E> &sym, const ElfRel<E> &rel) {
  switch (action) {
  case NONE:
    break;
  case ERROR:
    Error(ctx) << isec << ": " << rel << " relocation at offset 0x"
               << std::hex << rel.r_offset << " against symbol `"
               << sym << "' can not be used; recompile with -fPIC";
    break;
  case COPYREL:
    sym.flags |= NEEDS_COPYREL;
    break;
  case PLT:
    // Create a PLT entry
    sym.flags |= NEEDS_PLT;
    break;
  case CPLT:
    // Create a canonical PLT entry
    sym.flags |= NEEDS_CPLT;
    break;
  }
}

template <typename E>
static inline i64 get_output_type(Context<E> &ctx) {
  if (ctx.arg.shared)
    return 0;
  if (ctx.arg.pie)
    return 1;
  return 2;
}

template <typename E>
static inline i64 get_sym_type(Symbol<E> &sym) {
  if (sym.is_absolute())
    return 0;
  if (!sym.is_imported)
    return 1;
  if (sym.get_type() != STT_FUNC)
    return 2;
  return 3;
}

template <typename E>
void InputSection<E>::scan_pcrel(Context<E> &ctx, Symbol<E> &sym,
                                 const ElfRel<E> &rel) {
  // This is for PC-relative relocations (e.g. R_X86_64_PC32).
  // We cannot promote them to dynamic relocations because the dynamic
  // linker generally does not support PC-relative relocations.
  static Action table[][4] = {
    // Absolute  Local    Imported data  Imported code
    {  ERROR,    NONE,    ERROR,         PLT    },  // Shared object
    {  ERROR,    NONE,    COPYREL,       CPLT   },  // Position-independent exec
    {  NONE,     NONE,    COPYREL,       CPLT   },  // Position-dependent exec
  };

  Action action = table[get_output_type(ctx)][get_sym_type(sym)];
  do_action(ctx, action, *this, sym, rel);
}

template <typename E>
void InputSection<E>::scan_absrel(Context<E> &ctx, Symbol<E> &sym,
                                  const ElfRel<E> &rel) {
  // This is a decision table for absolute relocations that is smaller
  // than the pointer size (e.g. R_X86_64_32). Since the dynamic linker
  // generally does not support dynamic relocations smaller than the
  // pointer size, we need to report an error if a relocation cannot be
  // resolved at link-time.
  static Action table[][4] = {
    // Absolute  Local    Imported data  Imported code
    {  NONE,     ERROR,   ERROR,         ERROR },  // Shared object
    {  NONE,     ERROR,   ERROR,         ERROR },  // Position-independent exec
    {  NONE,     NONE,    COPYREL,       CPLT  },  // Position-dependent exec
  };

  Action action = table[get_output_type(ctx)][get_sym_type(sym)];
  do_action(ctx, action, *this, sym, rel);
}

template <typename E>
void InputSection<E>::scan_tlsdesc(Context<E> &ctx, Symbol<E> &sym) {
  if (ctx.arg.static_ || (ctx.arg.relax && sym.is_tprel_linktime_const(ctx))) {
    // Relax TLSDESC to Local Exec. In this case, we directly materialize
    // a TP-relative offset, so no dynamic relocation is needed.
    //
    // TLSDESC relocs must always be relaxed for statically-linked
    // executables even if -no-relax is given. It is because a
    // statically-linked executable doesn't contain a trampoline
    // function needed for TLSDESC.
  } else if (ctx.arg.relax && sym.is_tprel_runtime_const(ctx)) {
    // In this condition, TP-relative offset of a thread-local variable
    // is known at process startup time, so we can relax TLSDESC to the
    // code that reads the TP-relative offset from GOT and add TP to it.
    sym.flags |= NEEDS_GOTTP;
  } else {
    // If no relaxation is doable, we simply create a TLSDESC dynamic
    // relocation.
    sym.flags |= NEEDS_TLSDESC;
  }
}

template <typename E>
void InputSection<E>::check_tlsle(Context<E> &ctx, Symbol<E> &sym,
                                  const ElfRel<E> &rel) {
  if (ctx.arg.shared)
    Error(ctx) << *this << ": relocation " << rel << " against `" << sym
               << "` can not be used when making a shared object;"
               << " recompile with -fPIC";
}

template <typename E>
void InputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  if (shdr().sh_type == SHT_NOBITS || sh_size == 0)
    return;

  // Copy data. In RISC-V and LoongArch object files, sections are not
  // atomic unit of copying because of relaxation. That is, some
  // relocations are allowed to remove bytes from the middle of a
  // section and shrink the overall size of it.
  if constexpr (is_riscv<E> || is_loongarch<E>) {
    std::span<RelocDelta> deltas = extra.r_deltas;

    if (deltas.empty()) {
      // If a section is not relaxed, we can copy it as a one big chunk.
      copy_contents_to(ctx, buf, sh_size);
    } else {
      // A relaxed section is copied piece-wise.
      memcpy(buf, contents.data(), deltas[0].offset);

      for (i64 i = 0; i < deltas.size(); i++) {
        i64 offset = deltas[i].offset;
        i64 delta = deltas[i].delta;
        i64 end = (i + 1 == deltas.size()) ? contents.size() : deltas[i + 1].offset;
        i64 removed_bytes = get_removed_bytes(deltas, i);
        memcpy(buf + offset + removed_bytes - delta,
               contents.data() + offset + removed_bytes,
               end - offset - removed_bytes);
      }
    }
  } else {
    copy_contents_to(ctx, buf, sh_size);
  }

  // Apply relocations
  if (!ctx.arg.relocatable) {
    if (shdr().sh_flags & SHF_ALLOC)
      apply_reloc_alloc(ctx, buf);
    else
      apply_reloc_nonalloc(ctx, buf);
  }
}

// Get the name of a function containin a given offset.
template <typename E>
std::string_view
InputSection<E>::get_func_name(Context<E> &ctx, i64 offset) const {
  for (Symbol<E> *sym : file.symbols)
    if (sym->file == &file)
      if (const ElfSym<E> &esym = sym->esym();
          esym.st_shndx == shndx && esym.st_type == STT_FUNC &&
          esym.st_value <= offset && offset < esym.st_value + esym.st_size)
        return ctx.arg.demangle ? demangle(*sym) : sym->name();
  return "";
}

// Test if the symbol a given relocation refers to has already been resolved.
// If not, record that error and returns true.
template <typename E>
bool InputSection<E>::record_undef_error(Context<E> &ctx, const ElfRel<E> &rel) {
  // If a relocation refers to a linker-synthesized symbol for a
  // section fragment, it's always been resolved.
  if (file.elf_syms.size() <= rel.r_sym)
    return false;

  Symbol<E> &sym = *file.symbols[rel.r_sym];
  const ElfSym<E> &esym = file.elf_syms[rel.r_sym];

  // If a symbol is defined in a comdat group, and the comdat group is
  // discarded, the symbol may not have an owner. It is technically an
  // violation of the One Definition Rule, so it is a programmer's fault.
  if (!sym.file) {
    Error(ctx) << *this << ": " << sym << " refers to a discarded COMDAT section"
               << " probably due to an ODR violation";
    return true;
  }

  auto record = [&] {
    std::stringstream ss;
    if (std::string_view source = file.get_source_name(); !source.empty())
      ss << ">>> referenced by " << source << "\n";
    else
      ss << ">>> referenced by " << *this << "\n";

    ss << ">>>               " << file;
    if (std::string_view func = get_func_name(ctx, rel.r_offset); !func.empty())
      ss << ":(" << func << ")";
    ss << '\n';

    typename decltype(ctx.undef_errors)::accessor acc;
    ctx.undef_errors.insert(acc, {&sym, {}});
    acc->second.push_back(ss.str());
  };

  // A non-weak undefined symbol must be promoted to an imported symbol
  // or resolved to an defined symbol. Otherwise, we need to report an
  // error or warn on it.
  //
  // Every ELF file has an absolute local symbol as its first symbol.
  // Referring to that symbol is always valid.
  bool is_undef = esym.is_undef() && !esym.is_weak() && sym.sym_idx;

  if (is_undef && sym.esym().is_undef()) {
    if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR && !sym.is_imported) {
      record();
      return true;
    }
    if (ctx.arg.unresolved_symbols == UNRESOLVED_WARN) {
      record();
      return false;
    }
  }

  return false;
}

template <typename E>
MergeableSection<E>::MergeableSection(Context<E> &ctx, MergedSection<E> &parent,
                                      std::unique_ptr<InputSection<E>> &isec)
  : parent(parent), p2align(isec->p2align), input_section(std::move(isec)) {
  input_section->uncompress(ctx);

  std::scoped_lock lock(parent.mu);
  parent.members.push_back(this);
}

static size_t find_null(std::string_view data, i64 pos, i64 entsize) {
  if (entsize == 1)
    return data.find('\0', pos);

  for (; pos <= data.size() - entsize; pos += entsize)
    if (data.substr(pos, entsize).find_first_not_of('\0') == data.npos)
      return pos;

  return data.npos;
}

// Mergeable sections (sections with SHF_MERGE bit) typically contain
// string literals. Linker is expected to split the section contents
// into null-terminated strings, merge them with mergeable strings
// from other object files, and emit uniquified strings to an output
// file.
//
// This mechanism reduces the size of an output file. If two source
// files happen to contain the same string literal, the output will
// contain only a single copy of it.
//
// It is less common than string literals, but mergeable sections can
// contain fixed-sized read-only records too.
//
// This function splits the section contents into small pieces that we
// call "section fragments". Section fragment is a unit of merging.
//
// We do not support mergeable sections that have relocations.
template <typename E>
void MergeableSection<E>::split_contents(Context<E> &ctx) {
  std::string_view data = input_section->contents;
  if (data.size() > UINT32_MAX)
    Fatal(ctx) << *input_section << ": mergeable section too large";

  i64 entsize = parent.shdr.sh_entsize;

  // Split sections
  if (parent.shdr.sh_flags & SHF_STRINGS) {
    for (i64 pos = 0; pos < data.size();) {
      frag_offsets.push_back(pos);
      size_t end = find_null(data, pos, entsize);
      if (end == data.npos)
        Fatal(ctx) << *input_section << ": string is not null terminated";
      pos = end + entsize;
    }
  } else {
    if (data.size() % entsize)
      Fatal(ctx) << *input_section
                 << ": section size is not multiple of sh_entsize";
    frag_offsets.reserve(data.size() / entsize);

    for (i64 pos = 0; pos < data.size(); pos += entsize)
      frag_offsets.push_back(pos);
  }

  // Compute hashes for section pieces
  HyperLogLog estimator;
  hashes.reserve(frag_offsets.size());

  for (i64 i = 0; i < frag_offsets.size(); i++) {
    u64 hash = hash_string(get_contents(i));
    hashes.push_back(hash);
    estimator.insert(hash);
  }

  parent.estimator.merge(estimator);

  static Counter counter("string_fragments");
  counter += frag_offsets.size();
}

template <typename E>
void MergeableSection<E>::resolve_contents(Context<E> &ctx) {
  fragments.reserve(frag_offsets.size());
  for (i64 i = 0; i < frag_offsets.size(); i++)
    fragments.push_back(parent.insert(ctx, get_contents(i), hashes[i], p2align));

  // Reclaim memory as we'll never use this vector again
  hashes.clear();
  hashes.shrink_to_fit();
}

using E = MOLD_TARGET;

template bool cie_equals(const CieRecord<E> &, const CieRecord<E> &);
template class InputSection<E>;
template class MergeableSection<E>;

} // namespace mold
