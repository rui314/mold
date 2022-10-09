#include "mold.h"

#include <limits>
#include <zlib.h>
#include <zstd.h>

namespace mold::elf {

template <typename E>
bool CieRecord<E>::equals(const CieRecord<E> &other) const {
  if (get_contents() != other.get_contents())
    return false;

  std::span<const ElfRel<E>> x = get_rels();
  std::span<const ElfRel<E>> y = other.get_rels();
  if (x.size() != y.size())
    return false;

  for (i64 i = 0; i < x.size(); i++) {
    if (x[i].r_offset - input_offset != y[i].r_offset - other.input_offset ||
        x[i].r_type != y[i].r_type ||
        file.symbols[x[i].r_sym] != other.file.symbols[y[i].r_sym] ||
        input_section.get_addend(x[i]) != other.input_section.get_addend(y[i]))
      return false;
  }
  return true;
}

static i64 to_p2align(u64 alignment) {
  if (alignment == 0)
    return 0;
  return std::countr_zero(alignment);
}

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file,
                              std::string_view name, i64 shndx)
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
  if constexpr (!is_rela<E>)
    uncompress(ctx);

  output_section =
    OutputSection<E>::get_instance(ctx, name, shdr().sh_type, shdr().sh_flags);
}

template <typename E>
void InputSection<E>::uncompress(Context<E> &ctx) {
  if (!(shdr().sh_flags & SHF_COMPRESSED) || uncompressed)
    return;

  u8 *buf = new u8[sh_size];
  uncompress_to(ctx, buf);
  contents = std::string_view((char *)buf, sh_size);
  ctx.string_pool.emplace_back(buf);
  uncompressed = true;
}

template <typename E>
void InputSection<E>::uncompress_to(Context<E> &ctx, u8 *buf) {
  if (!(shdr().sh_flags & SHF_COMPRESSED) || uncompressed) {
    memcpy(buf, contents.data(), contents.size());
    return;
  }

  if (contents.size() < sizeof(ElfChdr<E>))
    Fatal(ctx) << *this << ": corrupted compressed section";

  ElfChdr<E> &hdr = *(ElfChdr<E> *)&contents[0];
  std::string_view data = contents.substr(sizeof(ElfChdr<E>));

  switch (hdr.ch_type) {
  case ELFCOMPRESS_ZLIB: {
    unsigned long size = sh_size;
    if (::uncompress(buf, &size, (u8 *)data.data(), data.size()) != Z_OK)
      Fatal(ctx) << *this << ": uncompress failed";
    assert(size == sh_size);
    break;
  }
  case ELFCOMPRESS_ZSTD:
    if (ZSTD_decompress(buf, sh_size, (u8 *)data.data(), data.size()) != sh_size)
      Fatal(ctx) << *this << ": ZSTD_decompress failed";
    break;
  default:
    Fatal(ctx) << *this << ": unsupported compression type: 0x"
               << std::hex << hdr.ch_type;
  }
}

template <typename E>
static ScanAction get_rel_action(Context<E> &ctx, Symbol<E> &sym,
                                 const ScanAction table[3][4]) {
  auto get_output_type = [&] {
    if (ctx.arg.shared)
      return 0;
    if (ctx.arg.pie)
      return 1;
    return 2;
  };

  auto get_sym_type = [&] {
    if (sym.is_absolute())
      return 0;
    if (!sym.is_imported)
      return 1;
    if (sym.get_type() != STT_FUNC)
      return 2;
    return 3;
  };

  return table[get_output_type()][get_sym_type()];
}

template <typename E>
void InputSection<E>::scan_rel(Context<E> &ctx, Symbol<E> &sym,
                               const ElfRel<E> &rel,
                               const ScanAction table[3][4]) {
  auto error = [&] {
    std::string msg = sym.is_absolute() ? "-fno-PIC" : "-fPIC";
    Error(ctx) << *this << ": " << rel << " relocation at offset 0x"
               << std::hex << rel.r_offset << " against symbol `"
               << sym << "' can not be used; recompile with " << msg;
  };

  auto check_textrel = [&] {
    if (this->shdr().sh_flags & SHF_WRITE)
      return;

    if (ctx.arg.z_text) {
      error();
    } else if (ctx.arg.warn_textrel) {
      Warn(ctx) << *this << ": relocation against symbol `" << sym
                << "' in read-only section";
    }
    ctx.has_textrel = true;
  };

  switch (get_rel_action(ctx, sym, table)) {
  case NONE:
    return;
  case ERROR:
    error();
    return;
  case COPYREL:
    if (!ctx.arg.z_copyreloc) {
      error();
    } else if (sym.esym().st_visibility == STV_PROTECTED) {
      Error(ctx) << *this << ": cannot make copy relocation for protected symbol '"
                 << sym << "', defined in " << *sym.file
                 << "; recompile with -fPIC";
    }
    sym.flags |= NEEDS_COPYREL;
    return;
  case PLT:
    sym.flags |= NEEDS_PLT;
    return;
  case CPLT:
    sym.flags |= NEEDS_CPLT;
    return;
  case DYNREL:
    assert(sym.is_imported);
    check_textrel();
    this->file.num_dynrel++;
    return;
  case BASEREL:
    check_textrel();
    if (!this->is_relr_reloc(ctx, rel))
      this->file.num_dynrel++;
    return;
  default:
    unreachable();
  }
}

template <typename E>
void InputSection<E>::apply_dyn_absrel(Context<E> &ctx, Symbol<E> &sym,
                                       const ElfRel<E> &rel, u8 *loc,
                                       u64 S, i64 A, u64 P,
                                       ElfRel<E> *&dynrel,
                                       const ScanAction table[3][4]) {
  switch (get_rel_action(ctx, sym, table)) {
  case COPYREL:
  case CPLT:
  case NONE:
    *(Word<E> *)loc = S + A;
    break;
  case BASEREL:
    if (is_relr_reloc(ctx, rel)) {
      *(Word<E> *)loc = S + A;
    } else {
      *dynrel++ = ElfRel<E>(P, E::R_RELATIVE, 0, S + A);
      if (ctx.arg.apply_dynamic_relocs)
        *(Word<E> *)loc = S + A;
    }
    break;
  case DYNREL:
    *dynrel++ = ElfRel<E>(P, E::R_ABS, sym.get_dynsym_idx(ctx), A);
    if (ctx.arg.apply_dynamic_relocs)
      *(Word<E> *)loc = A;
    break;
  default:
    unreachable();
  }
}

template <typename E>
void InputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  if (shdr().sh_type == SHT_NOBITS || sh_size == 0)
    return;

  // Copy data
  if constexpr (is_riscv<E>) {
    copy_contents_riscv(ctx, buf);
  } else {
    uncompress_to(ctx, buf);
  }

  // Apply relocations
  if (shdr().sh_flags & SHF_ALLOC)
    apply_reloc_alloc(ctx, buf);
  else
    apply_reloc_nonalloc(ctx, buf);
}

// Get the name of a function containin a given offset.
template <typename E>
std::string_view InputSection<E>::get_func_name(Context<E> &ctx, i64 offset) {
  for (const ElfSym<E> &esym : file.elf_syms) {
    if (esym.st_shndx == shndx && esym.st_type == STT_FUNC &&
        esym.st_value <= offset && offset < esym.st_value + esym.st_size) {
      std::string_view name = file.symbol_strtab.data() + esym.st_name;
      if (ctx.arg.demangle)
        return demangle(name);
      return name;
    }
  }
  return "";
}

// Record an undefined symbol error which will be displayed all at
// once by report_undef_errors().
template <typename E>
void InputSection<E>::record_undef_error(Context<E> &ctx, const ElfRel<E> &rel) {
  std::stringstream ss;
  if (std::string_view source = file.get_source_name(); !source.empty())
    ss << ">>> referenced by " << source << "\n";
  else
    ss << ">>> referenced by " << *this << "\n";

  ss << ">>>               " << file;
  if (std::string_view func = get_func_name(ctx, rel.r_offset); !func.empty())
    ss << ":(" << func << ")";

  Symbol<E> &sym = *file.symbols[rel.r_sym];

  typename decltype(ctx.undef_errors)::accessor acc;
  ctx.undef_errors.insert(acc, {sym.name(), {}});
  acc->second.push_back(ss.str());
}

// Report all undefined symbols, grouped by symbol.
template <typename E>
void report_undef_errors(Context<E> &ctx) {
  constexpr i64 max_errors = 3;

  for (auto &pair : ctx.undef_errors) {
    std::string_view sym_name = pair.first;
    std::span<std::string> errors = pair.second;

    if (ctx.arg.demangle)
      sym_name = demangle(sym_name);

    std::stringstream ss;
    ss << "undefined symbol: " << sym_name << "\n";

    for (i64 i = 0; i < errors.size() && i < max_errors; i++)
      ss << errors[i];

    if (errors.size() > max_errors)
      ss << ">>> referenced " << (errors.size() - max_errors) << " more times\n";

    if (ctx.arg.unresolved_symbols == UNRESOLVED_ERROR)
      Error(ctx) << ss.str();
    else if (ctx.arg.unresolved_symbols == UNRESOLVED_WARN)
      Warn(ctx) << ss.str();
  }

  ctx.checkpoint();
}

using E = MOLD_TARGET;

template struct CieRecord<E>;
template class InputSection<E>;
template void report_undef_errors(Context<E> &);

} // namespace mold::elf
