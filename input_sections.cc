#include "mold.h"

#include <limits>

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file,
                              const ElfShdr<E> &shdr, std::string_view name,
                              std::string_view contents, i64 section_idx)
  : file(file), shdr(shdr), nameptr(name.data()), namelen(name.size()),
    contents(contents), section_idx(section_idx) {
  // As a special case, we want to map .ctors and .dtors to
  // .init_array and .fini_array, respectively. However, old CRT
  // object files are not compatible with this translation, so we need
  // to keep them as-is if a section came from crtbegin.o or crtend.o.
  //
  // Yeah, this is an ugly hack, but the fundamental problem is that
  // we have two different mechanism, ctors/dtors and init_array/fini_array
  // for the same purpose. The latter was introduced to replace the
  // former, but as it is often the case, the former still lingers
  // around, so we need to keep this code to conver the old mechanism
  // to the new one.
  std::string_view stem = path_filename(file.filename);
  if (stem != "crtbegin.o" && stem != "crtend.o" &&
      stem != "crtbeginS.o" && stem != "crtendS.o") {
    if (name == ".ctors" || name.starts_with(".ctors."))
      name = ".init_array";
    else if (name == ".dtors" || name.starts_with(".dtors."))
      name = ".fini_array";
  }

  output_section =
    OutputSection<E>::get_instance(ctx, name, shdr.sh_type, shdr.sh_flags);
}

template <typename E>
void InputSection<E>::write_to(Context<E> &ctx, u8 *buf) {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  memcpy(buf, contents.data(), contents.size());

  // Apply relocations
  if (shdr.sh_flags & SHF_ALLOC)
    apply_reloc_alloc(ctx, buf);
  else
    apply_reloc_nonalloc(ctx, buf);

  // As a special case, .ctors and .dtors section contents are
  // reversed. These sections are now obsolete and mapped to
  // .init_array and .fini_array, but they have to be reversed to
  // maintain the original semantics.
  bool init_fini = output_section->name == ".init_array" ||
                   output_section->name == ".fini_array";
  bool ctors_dtors = name().starts_with(".ctors") ||
                     name().starts_with(".dtors");
  if (init_fini && ctors_dtors)
    std::reverse((typename E::WordTy *)buf,
                 (typename E::WordTy *)(buf + shdr.sh_size));
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
static i64 get_sym_type(Context<E> &ctx, Symbol<E> &sym) {
  if (sym.is_absolute(ctx))
    return 0;
  if (!sym.is_imported)
    return 1;
  if (sym.get_type() != STT_FUNC)
    return 2;
  return 3;
}

template <typename E>
void InputSection<E>::dispatch(Context<E> &ctx, Action table[3][4],
                               u16 rel_type, i64 i) {
  std::span<ElfRel<E>> rels = get_rels(ctx);
  const ElfRel<E> &rel = rels[i];
  Symbol<E> &sym = *file.symbols[rel.r_sym];
  bool is_writable = (shdr.sh_flags & SHF_WRITE);
  Action action = table[get_output_type(ctx)][get_sym_type(ctx, sym)];

  switch (action) {
  case NONE:
    rel_types[i] = rel_type;
    return;
  case ERROR:
    break;
  case COPYREL:
    if (!ctx.arg.z_copyreloc)
      break;
    if (sym.esym().st_visibility == STV_PROTECTED)
      Error(ctx) << *this << ": cannot make copy relocation for "
                 << " protected symbol '" << sym << "', defined in "
                 << *sym.file;
    sym.flags |= NEEDS_COPYREL;
    rel_types[i] = rel_type;
    return;
  case PLT:
    sym.flags |= NEEDS_PLT;
    rel_types[i] = rel_type;
    return;
  case DYNREL:
    if (!is_writable) {
      if (ctx.arg.z_text)
        break;
      ctx.has_textrel = true;
    }
    sym.flags |= NEEDS_DYNSYM;
    rel_types[i] = R_DYN;
    file.num_dynrel++;
    return;
  case BASEREL:
    if (!is_writable) {
      if (ctx.arg.z_text)
        break;
      ctx.has_textrel = true;
    }
    rel_types[i] = R_BASEREL;
    file.num_dynrel++;
    return;
  default:
    unreachable(ctx);
  }

  Error(ctx) << *this << ": " << rel_to_string<E>(rel.r_type)
             << " relocation against symbol `" << sym
             << "' can not be used; recompile with -fPIE";
}

template <typename E>
void InputSection<E>::report_undef(Context<E> &ctx, Symbol<E> &sym) {
  switch (ctx.arg.unresolved_symbols) {
  case UnresolvedKind::ERROR:
    Error(ctx) << "undefined symbol: " << file << ": " << sym;
    break;
  case UnresolvedKind::WARN:
    Warn(ctx) << "undefined symbol: " << file << ": " << sym;
    break;
  case UnresolvedKind::IGNORE:
    break;
  }
}

template class InputSection<X86_64>;
template class InputSection<I386>;
