#include "mold.h"

#include <limits>
#include <zlib.h>

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
    contents = {(char *)file.mf->data + shdr().sh_offset, (size_t)shdr().sh_size};

  if (name.starts_with(".zdebug")) {
    sh_size = *(ub64 *)&contents[4];
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

  // Sections may have been compressed. We usually uncompress them
  // directly into the mmap'ed output file, but we want to uncompress
  // early for REL-type ELF types to read relocation addends from
  // section contents. For RELA-type, we don't need to do this because
  // addends are in relocations.
  if (E::is_rel)
    uncompress(ctx);

  output_section =
    OutputSection<E>::get_instance(ctx, name, shdr().sh_type, shdr().sh_flags);
}

template <typename E>
void InputSection<E>::uncompress(Context<E> &ctx) {
  if (!compressed || uncompressed)
    return;

  u8 *buf = new u8[sh_size];
  uncompress_to(ctx, buf);
  contents = {(char *)buf, sh_size};
  ctx.string_pool.emplace_back(buf);
  uncompressed = true;
}

template <typename E>
void InputSection<E>::uncompress_to(Context<E> &ctx, u8 *buf) {
  if (!compressed || uncompressed) {
    memcpy(buf, contents.data(), contents.size());
    return;
  }

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
    Fatal(ctx) << *this << ": unsupported compression type: 0x"
               << std::hex << hdr.ch_type;
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
  case CPLT:
    sym.flags |= NEEDS_CPLT;
    return;
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
  } else if (compressed) {
    uncompress_to(ctx, buf);
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
void add_undef(Context<E> &ctx, InputFile<E> &file, Symbol<E> &sym,
               InputSection<E> *section, typename E::WordTy r_offset) {
  assert(!ctx.undefined_done);
  ctx.undefined.push_back({file, sym, section, r_offset});
}

template <typename E>
void report_undef(Context<E> &ctx) {
  setup_context_debuginfo(ctx);

  // Report all undefined symbols, grouped by symbol.
  std::unordered_set<Symbol<E>*> handled;
  for (const typename Context<E>::Undefined &group : ctx.undefined) {
    if (handled.contains(&group.sym))
        continue;
    handled.emplace(&group.sym);

    std::stringstream report;
    report << "undefined symbol: " << group.sym << "\n";

    int count = 0;
    constexpr int max_reported_count = 3;
    for (const typename Context<E>::Undefined &undef : ctx.undefined) {
      if (&undef.sym != &group.sym)
        continue;
      if (++count > max_reported_count)
        continue;

      InputFile<E> &file = undef.file;

      // Find the source file which references the symbol. First try debuginfo,
      // as that one provides also source location. Debuginfo needs to be relocated,
      // so this uses the resulting debuginfo rather than debuginfo in the object file.
      std::string_view source_name;
      std::string_view directory;
      i32 line = 0;
      i32 column = 0;
      bool line_valid = false;
      ObjectFile<E> * object_file = dynamic_cast<ObjectFile<E> *>(&file);
      if (object_file != nullptr && object_file->debug_info && undef.section != nullptr) {
        if (object_file->compunits.empty())
          object_file->compunits = read_compunits(ctx, *object_file);
        std::tie(source_name, directory, line, column) = find_source_location(ctx,
          *object_file, undef.r_offset + undef.section->get_addr());
        line_valid = !source_name.empty();
      }

      if (source_name.empty()) {
        // If using debuginfo fails, find the source file from symtab. It should be listed
        // in symtab as STT_FILE, the closest one before the undefined entry.
        auto sym_pos = std::find(file.symbols.begin(), file.symbols.end(), &undef.sym);
        if (sym_pos != file.symbols.end()) {
          while (sym_pos != file.symbols.begin()) {
            --sym_pos;
            Symbol<E> *tmp = *sym_pos;
            if (tmp->file && tmp->get_type() == STT_FILE) {
              source_name = tmp->name();
              break;
            }
          }
        }
      }

      // Find the function that references the symbol by trying to find the relocation offset
      // inside the section in one of the function ranges given by symtab.
      std::string function_name;
      if (undef.section != nullptr) {
        for (const ElfSym<E> & elfsym : file.elf_syms) {
          if (elfsym.st_shndx == undef.section->shndx && elfsym.st_type == STT_FUNC
            && undef.r_offset >= elfsym.st_value && undef.r_offset < elfsym.st_value + elfsym.st_size) {
            function_name = file.symbol_strtab_name(elfsym.st_name);
            if (ctx.arg.demangle)
              function_name = demangle(function_name);
            break;
          }
        }
      }

      if (!source_name.empty()) {
        std::string location(source_name);
        if (line != 0)
          location += ":" + std::to_string(line);
        if (column != 0)
          location += ":" + std::to_string(column);
        if (!directory.empty())
          report << ">>> referenced by " << location << " (" << directory << "/" << location << ")\n";
        else
          report << ">>> referenced by " << location << "\n";
      } else
        report << ">>> referenced by " << file << "\n";
      report << ">>>               " << file;
      if (!function_name.empty())
        report << ":(" << function_name << ")";
      report << "\n";

      if (ctx.arg.warn_once)
        break;
    }

    if (count > max_reported_count)
      report << ">>> referenced " << (count - max_reported_count) << " more times\n";

    switch (ctx.arg.unresolved_symbols) {
    case UNRESOLVED_ERROR:
      Error(ctx) << report.str();
      break;
    case UNRESOLVED_WARN:
      Warn(ctx) << report.str();
      break;
    case UNRESOLVED_IGNORE:
      break;
    }
  }

  ctx.undefined_done = true;
}

#define INSTANTIATE(E)                                                  \
  template struct CieRecord<E>;                                         \
  template class InputSection<E>;                                       \
  template void add_undef(Context<E> &, InputFile<E> &, Symbol<E> &,    \
    InputSection<E> *section, typename E::WordTy r_offset);             \
  template void report_undef(Context<E> &)


INSTANTIATE_ALL;

} // namespace mold::elf
