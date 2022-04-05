#include "mold.h"

#include <limits>
#include <zlib.h>

namespace mold::elf {

template <typename E>
bool CieRecord<E>::equals(const CieRecord<E> &other) const {
  if (get_contents() != other.get_contents())
    return false;

  std::span<ElfRel<E>> x = get_rels();
  std::span<ElfRel<E>> y = other.get_rels();
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

static inline i64 to_p2align(u64 alignment) {
  if (alignment == 0)
    return 0;
  return std::countr_zero(alignment);
}

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file,
                              std::string_view name, i64 shndx)
  : file(file), shndx(shndx) {
  if (shndx < file.elf_sections.size())
    contents = {(char *)file.mf->data + shdr().sh_offset, shdr().sh_size};

  bool compressed;

  if (name.starts_with(".zdebug")) {
    sh_size = *(ubig64 *)&contents[4];
    p2align = to_p2align(shdr().sh_addralign);
    compressed = true;
  } else if (shdr().sh_flags & SHF_COMPRESSED) {
    ElfChdr<E> &chdr = *(ElfChdr<E> *)&contents[0];
    sh_size = chdr.ch_size;
    p2align = to_p2align(chdr.ch_addralign);
    compressed = true;
  } else {
    sh_size = shdr().sh_size;
    p2align = to_p2align(shdr().sh_addralign);
    compressed = false;
  }

  // Uncompress early if the relocation is REL-type so that we can read
  // addends from section contents. If RELA-type, we don't need to do this
  // because addends are in relocations.
  if (compressed && E::is_rel) {
    u8 *buf = new u8[sh_size];
    uncompress(ctx, buf);
    contents = {(char *)buf, sh_size};
    ctx.string_pool.emplace_back(buf);
  }

  output_section =
    OutputSection<E>::get_instance(ctx, name, shdr().sh_type, shdr().sh_flags);
}

template <typename E>
bool InputSection<E>::is_compressed() {
  return !E::is_rel &&
         (name().starts_with(".zdebug") || (shdr().sh_flags & SHF_COMPRESSED));
}

template <typename E>
void InputSection<E>::uncompress(Context<E> &ctx, u8 *buf) {
  auto do_uncompress = [&](std::string_view data) {
    unsigned long size = sh_size;
    if (::uncompress(buf, &size, (u8 *)data.data(), data.size()) != Z_OK)
      Fatal(ctx) << *this << ": uncompress failed";
    assert(size == sh_size);
  };

  if (name().starts_with(".zdebug")) {
    // Old-style compressed section
    if (!contents.starts_with("ZLIB") || contents.size() <= 12)
      Fatal(ctx) << *this << ": corrupted compressed section";
    do_uncompress(contents.substr(12));
    return;
  }

  assert(shdr().sh_flags & SHF_COMPRESSED);

  // New-style compressed section
  if (contents.size() < sizeof(ElfChdr<E>))
    Fatal(ctx) << *this << ": corrupted compressed section";

  ElfChdr<E> &hdr = *(ElfChdr<E> *)&contents[0];
  if (hdr.ch_type != ELFCOMPRESS_ZLIB)
    Fatal(ctx) << *this << ": unsupported compression type";
  do_uncompress(contents.substr(sizeof(ElfChdr<E>)));
}

template <typename E>
static i64 get_output_type(Context<E> &ctx) {
  if (ctx.arg.shared)
    return 0;
  if (ctx.arg.pie)
    return 1;
  return 2;
}

template <typename E>
static i64 get_sym_type(Symbol<E> &sym) {
  if (sym.is_absolute())
    return 0;
  if (!sym.is_imported)
    return 1;
  if (sym.get_type() != STT_FUNC)
    return 2;
  return 3;
}

template <typename E>
void InputSection<E>::dispatch(Context<E> &ctx, Action table[3][4], i64 i,
                               const ElfRel<E> &rel, Symbol<E> &sym) {
  Action action = table[get_output_type(ctx)][get_sym_type(sym)];
  bool is_writable = (shdr().sh_flags & SHF_WRITE);

  auto error = [&] {
    std::string msg = sym.is_absolute() ? "-fno-PIC" : "-fPIC";
    Error(ctx) << *this << ": " << rel << " relocation at offset 0x"
               << std::hex << rel.r_offset << " against symbol `"
               << sym << "' can not be used; recompile with " << msg;
  };

  auto warn_textrel = [&] {
    if (ctx.arg.warn_textrel)
      Warn(ctx) << *this << ": relocation against symbol `" << sym
                << "' in read-only section";
  };

  switch (action) {
  case NONE:
    return;
  case ERROR:
    error();
    return;
  case COPYREL:
    if (!ctx.arg.z_copyreloc) {
      error();
      return;
    }

    if (sym.esym().st_visibility == STV_PROTECTED) {
      Error(ctx) << *this
                 << ": cannot make copy relocation for protected symbol '"
                 << sym << "', defined in " << *sym.file
                 << "; recompile with -fPIC";
      return;
    }

    sym.flags |= NEEDS_COPYREL;
    return;
  case PLT:
    sym.flags |= NEEDS_PLT;
    return;
  case CPLT: {
    std::scoped_lock lock(sym.mu);
    sym.flags |= NEEDS_PLT;
    sym.is_canonical = true;
    return;
  }
  case DYNREL:
    if (!is_writable) {
      if (ctx.arg.z_text) {
        error();
        return;
      }
      warn_textrel();
      ctx.has_textrel = true;
    }

    assert(sym.is_imported);
    file.num_dynrel++;
    return;
  case BASEREL:
    if (!is_writable) {
      if (ctx.arg.z_text) {
        error();
        return;
      }
      warn_textrel();
      ctx.has_textrel = true;
    }

    if (!is_relr_reloc(ctx, rel))
      file.num_dynrel++;
    return;
  default:
    unreachable();
  }
}

template <typename E>
void InputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  if (shdr().sh_type == SHT_NOBITS || sh_size == 0)
    return;

  // Copy data
  if constexpr (std::is_same_v<E, RISCV64>) {
    copy_contents_riscv(ctx, buf);
  } else if (is_compressed()) {
    uncompress(ctx, buf);
  } else {
    memcpy(buf, contents.data(), contents.size());
  }

  // Apply relocations
  if (shdr().sh_flags & SHF_ALLOC)
    apply_reloc_alloc(ctx, buf);
  else
    apply_reloc_nonalloc(ctx, buf);
}

template <typename E>
void report_undef(Context<E> &ctx, InputFile<E> &file, Symbol<E> &sym) {
  if (ctx.arg.warn_once && !ctx.warned.insert({(void *)&sym, 1}))
    return;

  switch (ctx.arg.unresolved_symbols) {
  case UNRESOLVED_ERROR:
    Error(ctx) << "undefined symbol: " << file << ": " << sym;
    break;
  case UNRESOLVED_WARN:
    Warn(ctx) << "undefined symbol: " << file << ": " << sym;
    break;
  case UNRESOLVED_IGNORE:
    break;
  }
}

#define INSTANTIATE(E)                                                  \
  template struct CieRecord<E>;                                         \
  template class InputSection<E>;                                       \
  template void report_undef(Context<E> &, InputFile<E> &, Symbol<E> &)


INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(ARM64);
INSTANTIATE(ARM32);
INSTANTIATE(RISCV64);

} // namespace mold::elf
