#include "mold.h"

#include <cstring>
#include <regex>

#ifndef _WIN32
# include <unistd.h>
#endif

namespace mold::elf {

template <typename E>
InputFile<E>::InputFile(Context<E> &ctx, MappedFile<Context<E>> *mf)
  : mf(mf), filename(mf->name) {
  if (mf->size < sizeof(ElfEhdr<E>))
    Fatal(ctx) << *this << ": file too small";
  if (memcmp(mf->data, "\177ELF", 4))
    Fatal(ctx) << *this << ": not an ELF file";

  ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)mf->data;
  is_dso = (ehdr.e_type == ET_DYN);

  ElfShdr<E> *sh_begin = (ElfShdr<E> *)(mf->data + ehdr.e_shoff);

  // e_shnum contains the total number of sections in an object file.
  // Since it is a 16-bit integer field, it's not large enough to
  // represent >65535 sections. If an object file contains more than 65535
  // sections, the actual number is stored to sh_size field.
  i64 num_sections = (ehdr.e_shnum == 0) ? sh_begin->sh_size : ehdr.e_shnum;

  if (mf->data + mf->size < (u8 *)(sh_begin + num_sections))
    Fatal(ctx) << *this << ": e_shoff or e_shnum corrupted: "
               << mf->size << " " << num_sections;
  elf_sections = {sh_begin, sh_begin + num_sections};

  // e_shstrndx is a 16-bit field. If .shstrtab's section index is
  // too large, the actual number is stored to sh_link field.
  i64 shstrtab_idx = (ehdr.e_shstrndx == SHN_XINDEX)
    ? sh_begin->sh_link : ehdr.e_shstrndx;

  shstrtab = this->get_string(ctx, shstrtab_idx);
}

template <typename E>
ElfShdr<E> *InputFile<E>::find_section(i64 type) {
  for (ElfShdr<E> &sec : elf_sections)
    if (sec.sh_type == type)
      return &sec;
  return nullptr;
}

template <typename E>
void InputFile<E>::clear_symbols() {
  for (Symbol<E> *sym : get_global_syms()) {
    std::scoped_lock lock(sym->mu);
    if (sym->file == this) {
      sym->file = nullptr;
      sym->shndx = 0;
      sym->value = -1;
      sym->sym_idx = -1;
      sym->ver_idx = 0;
      sym->is_weak = false;
      sym->is_imported = false;
      sym->is_exported = false;
    }
  }
}

// Find the source filename. It should be listed in symtab as STT_FILE.
template <typename E>
std::string_view InputFile<E>::get_source_name() const {
  for (i64 i = 0; i < first_global; i++)
    if (Symbol<E> *sym = symbols[i]; sym->get_type() == STT_FILE)
      return sym->name();
  return "";
}

template <typename E>
ObjectFile<E>::ObjectFile(Context<E> &ctx, MappedFile<Context<E>> *mf,
                          std::string archive_name, bool is_in_lib)
  : InputFile<E>(ctx, mf), archive_name(archive_name), is_in_lib(is_in_lib) {
  this->is_alive = !is_in_lib;
}

template <typename E>
ObjectFile<E> *
ObjectFile<E>::create(Context<E> &ctx, MappedFile<Context<E>> *mf,
                      std::string archive_name, bool is_in_lib) {
  ObjectFile<E> *obj = new ObjectFile<E>(ctx, mf, archive_name, is_in_lib);
  ctx.obj_pool.emplace_back(obj);
  return obj;
}

template <typename E>
static bool is_debug_section(const ElfShdr<E> &shdr, std::string_view name) {
  return !(shdr.sh_flags & SHF_ALLOC) && name.starts_with(".debug");
}

template <typename E>
u32 ObjectFile<E>::read_note_gnu_property(Context<E> &ctx,
                                          const ElfShdr<E> &shdr) {
  std::string_view data = this->get_string(ctx, shdr);
  u32 ret = 0;

  while (!data.empty()) {
    ElfNhdr<E> &hdr = *(ElfNhdr<E> *)data.data();
    data = data.substr(sizeof(hdr));

    std::string_view name = data.substr(0, hdr.n_namesz - 1);
    data = data.substr(align_to(hdr.n_namesz, 4));

    std::string_view desc = data.substr(0, hdr.n_descsz);
    data = data.substr(align_to(hdr.n_descsz, sizeof(Word<E>)));

    if (hdr.n_type != NT_GNU_PROPERTY_TYPE_0 || name != "GNU")
      continue;

    while (!desc.empty()) {
      u32 type = *(U32<E> *)desc.data();
      u32 size = *(U32<E> *)(desc.data() + 4);
      desc = desc.substr(8);
      if (type == GNU_PROPERTY_X86_FEATURE_1_AND)
        ret |= *(U32<E> *)desc.data();
      desc = desc.substr(align_to(size, sizeof(Word<E>)));
    }
  }
  return ret;
}

template <typename E>
void ObjectFile<E>::initialize_sections(Context<E> &ctx) {
  // Read sections
  for (i64 i = 0; i < this->elf_sections.size(); i++) {
    const ElfShdr<E> &shdr = this->elf_sections[i];

    if ((shdr.sh_flags & SHF_EXCLUDE) && !(shdr.sh_flags & SHF_ALLOC) &&
        shdr.sh_type != SHT_LLVM_ADDRSIG)
      continue;

    switch (shdr.sh_type) {
    case SHT_GROUP: {
      // Get the signature of this section group.
      if (shdr.sh_info >= this->elf_syms.size())
        Fatal(ctx) << *this << ": invalid symbol index";
      const ElfSym<E> &sym = this->elf_syms[shdr.sh_info];
      std::string_view signature = this->symbol_strtab.data() + sym.st_name;

      // Ignore a broken comdat group GCC emits for .debug_macros.
      // https://github.com/rui314/mold/issues/438
      if (signature.starts_with("wm4."))
        continue;

      // Get comdat group members.
      std::span<U32<E>> entries = this->template get_data<U32<E>>(ctx, shdr);

      if (entries.empty())
        Fatal(ctx) << *this << ": empty SHT_GROUP";
      if (entries[0] == 0)
        continue;
      if (entries[0] != GRP_COMDAT)
        Fatal(ctx) << *this << ": unsupported SHT_GROUP format";

      typename decltype(ctx.comdat_groups)::const_accessor acc;
      ctx.comdat_groups.insert(acc, {signature, ComdatGroup()});
      ComdatGroup *group = const_cast<ComdatGroup *>(&acc->second);
      comdat_groups.push_back({group, entries.subspan(1)});
      break;
    }
    case SHT_SYMTAB_SHNDX:
      symtab_shndx_sec = this->template get_data<U32<E>>(ctx, shdr);
      break;
    case SHT_SYMTAB:
    case SHT_STRTAB:
    case SHT_REL:
    case SHT_RELA:
    case SHT_NULL:
    case SHT_ARM_ATTRIBUTES:
      break;
    default: {
      std::string_view name = this->shstrtab.data() + shdr.sh_name;

      // .note.GNU-stack section controls executable-ness of the stack
      // area in GNU linkers. We ignore that section because silently
      // making the stack area executable is too dangerous. Tell our
      // users about the difference if that matters.
      if (name == ".note.GNU-stack") {
        if (shdr.sh_flags & SHF_EXECINSTR) {
          if (!ctx.arg.z_execstack && !ctx.arg.z_execstack_if_needed)
            Warn(ctx) << *this << ": this file may cause a segmentation"
              " fault because it requires an executable stack. See"
              " https://github.com/rui314/mold/tree/main/docs/execstack.md"
              " for more info.";
          needs_executable_stack = true;
        }
        continue;
      }

      if (name.starts_with(".gnu.warning."))
        continue;

      if (name == ".note.gnu.property") {
        this->features = read_note_gnu_property(ctx, shdr);
        continue;
      }

      // Ignore these sections for compatibility with old glibc i386 CRT files.
      if (name == ".gnu.linkonce.t.__x86.get_pc_thunk.bx" ||
          name == ".gnu.linkonce.t.__i686.get_pc_thunk.bx")
        continue;

      // Also ignore this for compatibility with ICC
      if (name == ".gnu.linkonce.d.DW.ref.__gxx_personality_v0")
        continue;

      // Ignore debug sections if --strip-all or --strip-debug is given.
      if ((ctx.arg.strip_all || ctx.arg.strip_debug) &&
          is_debug_section(shdr, name))
        continue;

      this->sections[i] = std::make_unique<InputSection<E>>(ctx, *this, name, i);

      // Save .llvm_addrsig for --icf=safe.
      if (shdr.sh_type == SHT_LLVM_ADDRSIG)
        llvm_addrsig = this->sections[i].get();

      // Save debug sections for --gdb-index.
      if (ctx.arg.gdb_index) {
        InputSection<E> *isec = this->sections[i].get();

        if (name == ".debug_info")
          debug_info = isec;
        if (name == ".debug_ranges")
          debug_ranges = isec;
        if (name == ".debug_rnglists")
          debug_rnglists = isec;

        // If --gdb-index is given, contents of .debug_gnu_pubnames and
        // .debug_gnu_pubtypes are copied to .gdb_index, so keeping them
        // in an output file is just a waste of space.
        if (name == ".debug_gnu_pubnames") {
          debug_pubnames = isec;
          isec->is_alive = false;
        }

        if (name == ".debug_gnu_pubtypes") {
          debug_pubtypes = isec;
          isec->is_alive = false;
        }

        // .debug_types is similar to .debug_info but contains type info
        // only. It exists only in DWARF 4, has been removed in DWARF 5 and
        // neither GCC nor Clang generate it by default
        // (-fdebug-types-section is needed). As such there is probably
        // little need to support it.
        if (name == ".debug_types")
          Fatal(ctx) << *this << ": mold's --gdb-index is not compatible"
            " with .debug_types; to fix this error, remove"
            " -fdebug-types-section and recompile";
      }

      static Counter counter("regular_sections");
      counter++;
      break;
    }
    }
  }

  // Attach relocation sections to their target sections.
  for (i64 i = 0; i < this->elf_sections.size(); i++) {
    const ElfShdr<E> &shdr = this->elf_sections[i];
    if (shdr.sh_type != (is_rela<E> ? SHT_RELA : SHT_REL))
      continue;

    if (shdr.sh_info >= sections.size())
      Fatal(ctx) << *this << ": invalid relocated section index: "
                 << (u32)shdr.sh_info;

    if (std::unique_ptr<InputSection<E>> &target = sections[shdr.sh_info]) {
      assert(target->relsec_idx == -1);
      target->relsec_idx = i;
    }
  }
}

template <typename E>
void ObjectFile<E>::initialize_ehframe_sections(Context<E> &ctx) {
  for (i64 i = 0; i < sections.size(); i++) {
    std::unique_ptr<InputSection<E>> &isec = sections[i];
    if (isec && isec->is_alive && isec->name() == ".eh_frame") {
      read_ehframe(ctx, *isec);
      isec->is_alive = false;
    }
  }
}

// .eh_frame contains data records explaining how to handle exceptions.
// When an exception is thrown, the runtime searches a record from
// .eh_frame with the current program counter as a key. A record that
// covers the current PC explains how to find a handler and how to
// transfer the control ot it.
//
// Unlike the most other sections, linker has to parse .eh_frame contents
// because of the following reasons:
//
// - There's usually only one .eh_frame section for each object file,
//   which explains how to handle exceptions for all functions in the same
//   object. If we just copy them, the resulting .eh_frame section will
//   contain lots of records for dead sections (i.e. de-duplicated inline
//   functions). We want to copy only records for live functions.
//
// - .eh_frame contains two types of records: CIE and FDE. There's usually
//   only one CIE at beginning of .eh_frame section followed by FDEs.
//   Compiler usually emits the identical CIE record for all object files.
//   We want to merge identical CIEs in an output .eh_frame section to
//   reduce the section size.
//
// - Scanning a .eh_frame section to find a record is an O(n) operation
//   where n is the number of records in the section. To reduce it to
//   O(log n), linker creates a .eh_frame_hdr section. The section
//   contains a sorted list of [an address in .text, an FDE address whose
//   coverage starts at the .text address] to make binary search doable.
//   In order to create .eh_frame_hdr, linker has to read .eh_frame.
//
// This function parses an input .eh_frame section.
template <typename E>
void ObjectFile<E>::read_ehframe(Context<E> &ctx, InputSection<E> &isec) {
  std::span<ElfRel<E>> rels = isec.get_rels(ctx);
  i64 cies_begin = cies.size();
  i64 fdes_begin = fdes.size();

  // Read CIEs and FDEs until empty.
  std::string_view contents = this->get_string(ctx, isec.shdr());
  i64 rel_idx = 0;

  for (std::string_view data = contents; !data.empty();) {
    i64 size = *(U32<E> *)data.data();
    if (size == 0)
      break;

    i64 begin_offset = data.data() - contents.data();
    i64 end_offset = begin_offset + size + 4;
    i64 id = *(U32<E> *)(data.data() + 4);
    data = data.substr(size + 4);

    i64 rel_begin = rel_idx;
    while (rel_idx < rels.size() && rels[rel_idx].r_offset < end_offset)
      rel_idx++;
    assert(rel_idx == rels.size() || begin_offset <= rels[rel_begin].r_offset);

    if (id == 0) {
      // This is CIE.
      cies.emplace_back(ctx, *this, isec, begin_offset, rels, rel_begin);
    } else {
      // This is FDE.
      if (rel_begin == rel_idx || rels[rel_begin].r_sym == 0) {
        // FDE has no valid relocation, which means FDE is dead from
        // the beginning. Compilers usually don't create such FDE, but
        // `ld -r` tend to generate such dead FDEs.
        continue;
      }

      if (rels[rel_begin].r_offset - begin_offset != 8)
        Fatal(ctx) << isec << ": FDE's first relocation should have offset 8";

      fdes.emplace_back(begin_offset, rel_begin);
    }
  }

  // Associate CIEs to FDEs.
  auto find_cie = [&](i64 offset) {
    for (i64 i = cies_begin; i < cies.size(); i++)
      if (cies[i].input_offset == offset)
        return i;
    Fatal(ctx) << isec << ": bad FDE pointer";
  };

  for (i64 i = fdes_begin; i < fdes.size(); i++) {
    i64 cie_offset = *(I32<E> *)(contents.data() + fdes[i].input_offset + 4);
    fdes[i].cie_idx = find_cie(fdes[i].input_offset + 4 - cie_offset);
  }

  auto get_isec = [&](const FdeRecord<E> &fde) -> InputSection<E> * {
    return get_section(this->elf_syms[rels[fde.rel_idx].r_sym]);
  };

  // We assume that FDEs for the same input sections are contiguous
  // in `fdes` vector.
  std::stable_sort(fdes.begin() + fdes_begin, fdes.end(),
                   [&](const FdeRecord<E> &a, const FdeRecord<E> &b) {
    return get_isec(a)->get_priority() < get_isec(b)->get_priority();
  });

  // Associate FDEs to input sections.
  for (i64 i = fdes_begin; i < fdes.size();) {
    InputSection<E> *isec = get_isec(fdes[i]);
    assert(isec->fde_begin == -1);
    isec->fde_begin = i++;

    while (i < fdes.size() && isec == get_isec(fdes[i]))
      i++;
    isec->fde_end = i;
  }
}

// Returns a symbol object for a given key. This function handles
// the -wrap option.
template <typename E>
static Symbol<E> *insert_symbol(Context<E> &ctx, const ElfSym<E> &esym,
                                std::string_view key, std::string_view name) {
  if (esym.is_undef() && name.starts_with("__real_") &&
      ctx.arg.wrap.contains(name.substr(7))) {
    return get_symbol(ctx, key.substr(7), name.substr(7));
  }

  Symbol<E> *sym = get_symbol(ctx, key, name);

  if (esym.is_undef() && sym->wrap) {
    key = save_string(ctx, "__wrap_" + std::string(key));
    name = save_string(ctx, "__wrap_" + std::string(name));
    return get_symbol(ctx, key, name);
  }
  return sym;
}

template <typename E>
void ObjectFile<E>::initialize_symbols(Context<E> &ctx) {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter += this->elf_syms.size();

  // Initialize local symbols
  this->local_syms.reset(new Symbol<E>[this->first_global]);

  new (&this->local_syms[0]) Symbol<E>;
  this->local_syms[0].file = this;
  this->local_syms[0].sym_idx = 0;

  for (i64 i = 1; i < this->first_global; i++) {
    const ElfSym<E> &esym = this->elf_syms[i];
    if (esym.is_common())
      Fatal(ctx) << *this << ": common local symbol?";

    std::string_view name = this->symbol_strtab.data() + esym.st_name;
    if (name.empty() && esym.st_type == STT_SECTION)
      if (InputSection<E> *sec = get_section(esym))
        name = sec->name();

    Symbol<E> &sym = this->local_syms[i];
    new (&sym) Symbol<E>(name);
    sym.file = this;
    sym.value = esym.st_value;
    sym.sym_idx = i;

    if (!esym.is_abs())
      sym.shndx = esym.is_abs() ? 0 : get_shndx(esym);
  }

  this->symbols.resize(this->elf_syms.size());

  i64 num_globals = this->elf_syms.size() - this->first_global;
  sym_fragments.resize(this->elf_syms.size());
  symvers.resize(num_globals);

  for (i64 i = 0; i < this->first_global; i++)
    this->symbols[i] = &this->local_syms[i];

  // Initialize global symbols
  for (i64 i = this->first_global; i < this->elf_syms.size(); i++) {
    const ElfSym<E> &esym = this->elf_syms[i];

    // Get a symbol name
    std::string_view key = this->symbol_strtab.data() + esym.st_name;
    std::string_view name = key;

    // Parse symbol version after atsign
    if (i64 pos = name.find('@'); pos != name.npos) {
      std::string_view ver = name.substr(pos + 1);
      name = name.substr(0, pos);

      if (!ver.empty() && ver != "@") {
        if (ver.starts_with('@'))
          key = name;
        if (esym.is_defined())
          symvers[i - this->first_global] = ver.data();
      }
    }

    this->symbols[i] = insert_symbol(ctx, esym, key, name);
    if (esym.is_common())
      has_common_symbol = true;
  }
}

// Relocations are usually sorted by r_offset in relocation tables,
// but for some reason only RISC-V does not follow that convention.
// We expect them to be sorted, so sort them if necessary.
template <typename E>
void ObjectFile<E>::sort_relocations(Context<E> &ctx) {
  if constexpr (is_riscv<E>) {
    auto less = [&](const ElfRel<E> &a, const ElfRel<E> &b) {
      return a.r_offset < b.r_offset;
    };

    for (i64 i = 1; i < sections.size(); i++) {
      std::unique_ptr<InputSection<E>> &isec = sections[i];
      if (!isec || !isec->is_alive || !(isec->shdr().sh_flags & SHF_ALLOC))
        continue;

      std::span<ElfRel<E>> rels = isec->get_rels(ctx);
      if (!std::is_sorted(rels.begin(), rels.end(), less))
        sort(rels, less);
    }
  }
}

static size_t find_null(std::string_view data, u64 entsize) {
  if (entsize == 1)
    return data.find('\0');

  for (i64 i = 0; i <= data.size() - entsize; i += entsize)
    if (data.substr(i, i + entsize).find_first_not_of('\0') == data.npos)
      return i;

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
static std::unique_ptr<MergeableSection<E>>
split_section(Context<E> &ctx, InputSection<E> &sec) {
  std::unique_ptr<MergeableSection<E>> rec(new MergeableSection<E>);
  rec->parent = MergedSection<E>::get_instance(ctx, sec.name(), sec.shdr().sh_type,
                                               sec.shdr().sh_flags);
  rec->p2align = sec.p2align;

  // If thes section contents are compressed, uncompress them.
  sec.uncompress(ctx);

  std::string_view data = sec.contents;
  const char *begin = data.data();
  u64 entsize = sec.shdr().sh_entsize;
  HyperLogLog estimator;

  // Split sections
  if (sec.shdr().sh_flags & SHF_STRINGS) {
    while (!data.empty()) {
      size_t end = find_null(data, entsize);
      if (end == data.npos)
        Fatal(ctx) << sec << ": string is not null terminated";

      std::string_view substr = data.substr(0, end + entsize);
      data = data.substr(end + entsize);

      rec->strings.push_back(substr);
      rec->frag_offsets.push_back(substr.data() - begin);

      u64 hash = hash_string(substr);
      rec->hashes.push_back(hash);
      estimator.insert(hash);
    }
  } else {
    if (data.size() % entsize)
      Fatal(ctx) << sec << ": section size is not multiple of sh_entsize";

    while (!data.empty()) {
      std::string_view substr = data.substr(0, entsize);
      data = data.substr(entsize);

      rec->strings.push_back(substr);
      rec->frag_offsets.push_back(substr.data() - begin);

      u64 hash = hash_string(substr);
      rec->hashes.push_back(hash);
      estimator.insert(hash);
    }
  }

  rec->parent->estimator.merge(estimator);

  static Counter counter("string_fragments");
  counter += rec->fragments.size();
  return rec;
}

// Usually a section is an atomic unit of inclusion and exclusion.
// The linker doesn't care its contents. However, if a section is a
// mergeable section (a section with SHF_MERGE bit set), the linker
// is expected to split it into smaller pieces and merge each piece
// with other pieces from different object files. In mold, we call
// the atomic unit of mergeable section "section pieces".
//
// This feature is typically used for string literals. String literals
// are usually put into a mergeable section by a compiler. If the same
// string literal happen to occur in two different translation units,
// a linker merges them into a single instance of a string, so that
// a linker's output doesn't contain duplicate string literals.
//
// Handling relocations referring mergeable sections is a bit tricky.
// Assume that we have a mergeable section with the following contents
// and symbols:
//
//
//   Hello world\0foo bar\0
//   ^            ^
//   .rodata      .L.str1
//   .L.str0
//
// '\0' represents a NUL byte. This mergeable section contains two
// section pieces, "Hello world" and "foo bar". The first string is
// referred by two symbols, .rodata and .L.str0, and the second by
// .L.str1. .rodata is a section symbol and therefore a local symbol
// and refers the begining of the section.
//
// In this example, there are actually two different ways to point to
// string "foo bar", because .rodata+12 and .L.str1+0 refer the same
// place in the section. This kind of "out-of-bound" reference occurs
// only when a symbol is a section symbol. In other words, compiler
// may use an offset from the beginning of a section to refer any
// section piece in a section, but it doesn't do for any other types
// of symbols.
//
// In mold, we attach section pieces to either relocations or symbols.
// If a relocation refers a section symbol whose section is a
// mergeable section, a section piece is attached to the relocation.
// If a non-section symbol refers a section piece, the section piece
// is attached to the symbol.
template <typename E>
void ObjectFile<E>::initialize_mergeable_sections(Context<E> &ctx) {
  mergeable_sections.resize(sections.size());

  for (i64 i = 0; i < sections.size(); i++) {
    std::unique_ptr<InputSection<E>> &isec = sections[i];
    if (isec && isec->is_alive && (isec->shdr().sh_flags & SHF_MERGE) &&
        isec->sh_size && isec->shdr().sh_entsize &&
        isec->relsec_idx == -1) {
      mergeable_sections[i] = split_section(ctx, *isec);
      isec->is_alive = false;
    }
  }
}

template <typename E>
void ObjectFile<E>::register_section_pieces(Context<E> &ctx) {
  for (std::unique_ptr<MergeableSection<E>> &m : mergeable_sections) {
    if (m) {
      m->fragments.reserve(m->strings.size());
      for (i64 i = 0; i < m->strings.size(); i++)
        m->fragments.push_back(m->parent->insert(m->strings[i], m->hashes[i],
                                                 m->p2align));

      // Shrink vectors that we will never use again to reclaim memory.
      m->strings.clear();
      m->hashes.clear();
    }
  }

  // Initialize rel_fragments
  for (std::unique_ptr<InputSection<E>> &isec : sections) {
    if (!isec || !isec->is_alive || !(isec->shdr().sh_flags & SHF_ALLOC))
      continue;

    std::span<ElfRel<E>> rels = isec->get_rels(ctx);
    if (rels.empty())
      continue;

    // Compute the size of rel_fragments.
    i64 len = 0;
    for (i64 i = 0; i < rels.size(); i++) {
      const ElfRel<E> &rel = rels[i];
      const ElfSym<E> &esym = this->elf_syms[rel.r_sym];
      if (esym.st_type == STT_SECTION && mergeable_sections[get_shndx(esym)])
        len++;
    }

    if (len == 0)
      continue;
    assert(sizeof(SectionFragmentRef<E>) * (len + 1) < UINT32_MAX);

    isec->rel_fragments.reset(new SectionFragmentRef<E>[len + 1]);
    i64 frag_idx = 0;

    // Fill rel_fragments contents.
    for (i64 i = 0; i < rels.size(); i++) {
      const ElfRel<E> &rel = rels[i];
      const ElfSym<E> &esym = this->elf_syms[rel.r_sym];
      if (esym.st_type != STT_SECTION)
        continue;

      std::unique_ptr<MergeableSection<E>> &m =
        mergeable_sections[get_shndx(esym)];
      if (!m)
        continue;

      i64 offset = esym.st_value + isec->get_addend(rel);
      std::span<u32> offsets = m->frag_offsets;

      auto it = std::upper_bound(offsets.begin(), offsets.end(), offset);
      if (it == offsets.begin())
        Fatal(ctx) << *this << ": bad relocation at " << rel.r_sym;
      i64 idx = it - 1 - offsets.begin();

      isec->rel_fragments[frag_idx++] = {m->fragments[idx], (i32)i,
                                         (i32)(offset - offsets[idx])};
    }

    isec->rel_fragments[frag_idx] = {nullptr, -1, -1};
  }

  // Initialize sym_fragments
  for (i64 i = 1; i < this->elf_syms.size(); i++) {
    const ElfSym<E> &esym = this->elf_syms[i];
    if (esym.is_abs() || esym.is_common() || esym.is_undef())
      continue;

    std::unique_ptr<MergeableSection<E>> &m =
      mergeable_sections[get_shndx(esym)];
    if (!m)
      continue;

    std::span<u32> offsets = m->frag_offsets;

    auto it = std::upper_bound(offsets.begin(), offsets.end(), esym.st_value);
    if (it == offsets.begin())
      Fatal(ctx) << *this << ": bad symbol value: " << esym.st_value;
    i64 idx = it - 1 - offsets.begin();

    if (i < this->first_global)
      this->symbols[i]->value = esym.st_value - offsets[idx];

    sym_fragments[i].frag = m->fragments[idx];
    sym_fragments[i].addend = esym.st_value - offsets[idx];
  }
}

template <typename E>
void ObjectFile<E>::mark_addrsig(Context<E> &ctx) {
  // Parse a .llvm_addrsig section.
  if (llvm_addrsig) {
    u8 *cur = (u8 *)llvm_addrsig->contents.data();
    u8 *end = cur + llvm_addrsig->contents.size();

    while (cur != end) {
      Symbol<E> &sym = *this->symbols[read_uleb(cur)];
      if (sym.file == this)
        if (InputSection<E> *isec = sym.get_input_section())
          isec->address_significant = true;
    }
  }

  // We treat a symbol's address as significant if
  //
  // 1. we have no address significance information for the symbol, or
  // 2. the symbol can be referenced from the outside in an address-
  //    significant manner.
  for (Symbol<E> *sym : this->symbols)
    if (sym->file == this)
      if (InputSection<E> *isec = sym->get_input_section())
        if (!llvm_addrsig || sym->is_exported)
          isec->address_significant = true;
}

template <typename E>
void ObjectFile<E>::parse(Context<E> &ctx) {
  sections.resize(this->elf_sections.size());
  symtab_sec = this->find_section(SHT_SYMTAB);

  if (symtab_sec) {
    // In ELF, all local symbols precede global symbols in the symbol table.
    // sh_info has an index of the first global symbol.
    this->first_global = symtab_sec->sh_info;
    this->elf_syms = this->template get_data<ElfSym<E>>(ctx, *symtab_sec);
    this->symbol_strtab = this->get_string(ctx, symtab_sec->sh_link);
  }

  initialize_sections(ctx);
  initialize_symbols(ctx);
  sort_relocations(ctx);
  initialize_mergeable_sections(ctx);
  initialize_ehframe_sections(ctx);
}

// Symbols with higher priorities overwrites symbols with lower priorities.
// Here is the list of priorities, from the highest to the lowest.
//
//  1. Strong defined symbol
//  2. Weak defined symbol
//  3. Strong defined symbol in a DSO/archive
//  4. Weak Defined symbol in a DSO/archive
//  5. Common symbol
//  6. Common symbol in an archive
//  7. Unclaimed (nonexistent) symbol
//
// Ties are broken by file priority.
template <typename E>
static u64 get_rank(InputFile<E> *file, const ElfSym<E> &esym, bool is_lazy) {
  if (esym.is_common()) {
    assert(!file->is_dso);
    if (is_lazy)
      return (6 << 24) + file->priority;
    return (5 << 24) + file->priority;
  }

  // GCC creates symbols in COMDATs with STB_GNU_UNIQUE instead of
  // STB_WEAK if it was configured to do so at build time or the
  // -fgnu-unique flag was given. In order to to not select a
  // GNU_UNIQUE symbol in a discarded COMDAT section, we treat it as
  // if it were weak.
  //
  // It looks like STB_GNU_UNIQUE is not a popular option anymore and
  // often disabled by default though.
  bool is_weak = (esym.st_bind == STB_WEAK || esym.st_bind == STB_GNU_UNIQUE);

  if (file->is_dso || is_lazy) {
    if (is_weak)
      return (4 << 24) + file->priority;
    return (3 << 24) + file->priority;
  }
  if (is_weak)
    return (2 << 24) + file->priority;
  return (1 << 24) + file->priority;
}

template <typename E>
static u64 get_rank(const Symbol<E> &sym) {
  if (!sym.file)
    return 7 << 24;
  return get_rank(sym.file, sym.esym(), !sym.file->is_alive);
}

// Symbol's visibility is set to the most restrictive one. For example,
// if one input file has a defined symbol `foo` with the default
// visibility and the other input file has an undefined symbol `foo`
// with the hidden visibility, the resulting symbol is a hidden defined
// symbol.
template <typename E>
void ObjectFile<E>::merge_visibility(Context<E> &ctx, Symbol<E> &sym,
                                     u8 visibility) {
  // Canonicalize visibility
  if (visibility == STV_INTERNAL)
    visibility = STV_HIDDEN;

  auto priority = [&](u8 visibility) {
    switch (visibility) {
    case STV_HIDDEN:
      return 1;
    case STV_PROTECTED:
      return 2;
    case STV_DEFAULT:
      return 3;
    }
    Fatal(ctx) << *this << ": unknown symbol visibility: " << sym;
  };

  update_minimum(sym.visibility, visibility, [&](u8 a, u8 b) {
    return priority(a) < priority(b);
  });
}

template <typename E>
static void print_trace_symbol(Context<E> &ctx, InputFile<E> &file,
                               const ElfSym<E> &esym, Symbol<E> &sym) {
  if (esym.is_defined())
    SyncOut(ctx) << "trace-symbol: " << file << ": definition of " << sym;
  else if (esym.is_weak())
    SyncOut(ctx) << "trace-symbol: " << file << ": weak reference to " << sym;
  else
    SyncOut(ctx) << "trace-symbol: " << file << ": reference to " << sym;
}

template <typename E>
void ObjectFile<E>::resolve_symbols(Context<E> &ctx) {
  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = this->elf_syms[i];

    if (esym.is_undef())
      continue;

    InputSection<E> *isec = nullptr;
    if (!esym.is_abs() && !esym.is_common()) {
      isec = get_section(esym);
      if (!isec)
        continue;
    }

    std::scoped_lock lock(sym.mu);
    if (get_rank(this, esym, !this->is_alive) < get_rank(sym)) {
      sym.file = this;
      sym.shndx = isec ? isec->shndx : 0;
      sym.value = esym.st_value;
      sym.sym_idx = i;
      sym.ver_idx = ctx.default_version;
      sym.is_weak = esym.is_weak();
      sym.is_imported = false;
      sym.is_exported = false;
    }
  }
}

template <typename E>
void
ObjectFile<E>::mark_live_objects(Context<E> &ctx,
                                 std::function<void(InputFile<E> *)> feeder) {
  assert(this->is_alive);

  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    const ElfSym<E> &esym = this->elf_syms[i];
    Symbol<E> &sym = *this->symbols[i];

    if (esym.is_defined() && exclude_libs)
      merge_visibility(ctx, sym, STV_HIDDEN);
    else
      merge_visibility(ctx, sym, esym.st_visibility);

    if (sym.traced)
      print_trace_symbol(ctx, *this, esym, sym);

    if (esym.is_weak())
      continue;

    std::scoped_lock lock(sym.mu);
    if (!sym.file)
      continue;

    bool keep = esym.is_undef() || (esym.is_common() && !sym.esym().is_common());
    if (keep && !sym.file->is_alive.exchange(true)) {
      feeder(sym.file);

      if (sym.traced)
        SyncOut(ctx) << "trace-symbol: " << *this << " keeps " << *sym.file
                     << " for " << sym;
    }
  }
}

// Comdat groups are used to de-duplicate functions and data that may
// be included into multiple object files. C++ compiler uses comdat
// groups to de-duplicate instantiated templates.
//
// For example, if a compiler decides to instantiate `std::vector<int>`,
// it generates code and data for `std::vector<int>` and put them into a
// comdat group whose name is the mangled name of `std::vector<int>`.
// The instantiation may happen multiple times for different translation
// units. Then linker de-duplicates them so that the resulting executable
// contains only a single copy of `std::vector<int>`.
template <typename E>
void ObjectFile<E>::resolve_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    update_minimum(group->owner, this->priority);
  }
}

template <typename E>
void ObjectFile<E>::eliminate_duplicate_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    if (group->owner == this->priority)
      continue;

    std::span<U32<E>> entries = pair.second;
    for (u32 i : entries)
      if (sections[i])
        sections[i]->kill();
  }
}

template <typename E>
void ObjectFile<E>::claim_unresolved_symbols(Context<E> &ctx) {
  if (!this->is_alive)
    return;

  auto report_undef = [&](Symbol<E> &sym) {
    std::stringstream ss;
    if (std::string_view source = this->get_source_name(); !source.empty())
      ss << ">>> referenced by " << source << "\n";
    else
      ss << ">>> referenced by " << *this << "\n";

    typename decltype(ctx.undef_errors)::accessor acc;
    ctx.undef_errors.insert(acc, {sym.name(), {}});
    acc->second.push_back(ss.str());
  };

  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    const ElfSym<E> &esym = this->elf_syms[i];
    Symbol<E> &sym = *this->symbols[i];
    if (!esym.is_undef())
      continue;

    std::scoped_lock lock(sym.mu);

    // If a protected/hidden undefined symbol is resolved to an
    // imported symbol, it's handled as if no symbols were found.
    if (sym.file && sym.file->is_dso &&
        (sym.visibility == STV_PROTECTED || sym.visibility == STV_HIDDEN)) {
      report_undef(sym);
      continue;
    }

    if (sym.file &&
        (!sym.esym().is_undef() || sym.file->priority <= this->priority))
      continue;

    // If a symbol name is in the form of "foo@version", search for
    // symbol "foo" and check if the symbol has version "version".
    std::string_view key = this->symbol_strtab.data() + esym.st_name;
    if (i64 pos = key.find('@'); pos != key.npos) {
      Symbol<E> *sym2 = get_symbol(ctx, key.substr(0, pos));
      if (sym2->file && sym2->file->is_dso &&
          sym2->get_version() == key.substr(pos + 1)) {
        this->symbols[i] = sym2;
        continue;
      }
    }

    auto claim = [&] {
      sym.file = this;
      sym.shndx = 0;
      sym.value = 0;
      sym.sym_idx = i;
      sym.is_weak = false;
      sym.is_exported = false;
    };

    if (ctx.arg.unresolved_symbols == UNRESOLVED_WARN)
      report_undef(sym);

    // Convert remaining undefined symbols to dynamic symbols.
    if (ctx.arg.shared) {
      // Traditionally, remaining undefined symbols cause a link failure
      // only when we are creating an executable. Undefined symbols in
      // shared objects are promoted to dynamic symbols, so that they'll
      // get another chance to be resolved at run-time. You can change the
      // behavior by passing `-z defs` to the linker.
      //
      // Even if `-z defs` is given, weak undefined symbols are still
      // promoted to dynamic symbols for compatibility with other linkers.
      // Some major programs, notably Firefox, depend on the behavior
      // (they use this loophole to export symbols from libxul.so).
      if (!ctx.arg.z_defs || esym.is_undef_weak() ||
          ctx.arg.unresolved_symbols != UNRESOLVED_ERROR) {
        claim();
        sym.ver_idx = 0;
        sym.is_imported = true;

        if (sym.traced)
          SyncOut(ctx) << "trace-symbol: " << *this << ": unresolved"
                       << (esym.is_weak() ? " weak" : "")
                       << " symbol " << sym;
        continue;
      }
    }

    // Convert remaining undefined symbols to absolute symbols with value 0.
    if (ctx.arg.unresolved_symbols != UNRESOLVED_ERROR ||
        ctx.arg.noinhibit_exec || esym.is_undef_weak()) {
      claim();
      sym.ver_idx = ctx.default_version;
      sym.is_imported = false;
    }
  }
}

template <typename E>
void ObjectFile<E>::convert_hidden_symbols(Context<E> &ctx) {
  if (!this->is_alive)
    return;

  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];

    if (sym.visibility != STV_HIDDEN)
      continue;

    std::scoped_lock lock(sym.mu);

    // make the symbol local
    sym.is_imported = false;
    sym.is_exported = false;
    sym.is_weak = false;
  }
}

template <typename E>
void ObjectFile<E>::scan_relocations(Context<E> &ctx) {
  // Scan relocations against seciton contents
  for (std::unique_ptr<InputSection<E>> &isec : sections)
    if (isec && isec->is_alive && (isec->shdr().sh_flags & SHF_ALLOC))
      isec->scan_relocations(ctx);

  // Scan relocations against exception frames
  for (CieRecord<E> &cie : cies) {
    for (ElfRel<E> &rel : cie.get_rels()) {
      Symbol<E> &sym = *this->symbols[rel.r_sym];

      if (sym.is_imported) {
        if (sym.get_type() != STT_FUNC)
          Fatal(ctx) << *this << ": " << sym
                  << ": .eh_frame CIE record with an external data reference"
                  << " is not supported";
        sym.flags |= NEEDS_PLT;
      }
    }
  }
}

// Common symbols are used by C's tantative definitions. Tentative
// definition is an obscure C feature which allows users to omit `extern`
// from global variable declarations in a header file. For example, if you
// have a tentative definition `int foo;` in a header which is included
// into multiple translation units, `foo` will be included into multiple
// object files, but it won't cause the duplicate symbol error. Instead,
// the linker will merge them into a single instance of `foo`.
//
// If a header file contains a tentative definition `int foo;` and one of
// a C file contains a definition with initial value such as `int foo = 5;`,
// then the "real" definition wins. The symbol for the tentative definition
// will be resolved to the real definition. If there is no "real"
// definition, the tentative definition gets the default initial value 0.
//
// Tentative definitions are represented as "common symbols" in an object
// file. In this function, we allocate spaces in .common or .tls_common
// for remaining common symbols that were not resolved to usual defined
// symbols in previous passes.
template <typename E>
void ObjectFile<E>::convert_common_symbols(Context<E> &ctx) {
  if (!has_common_symbol)
    return;

  OutputSection<E> *common =
    OutputSection<E>::get_instance(ctx, ".common", SHT_NOBITS,
                                   SHF_WRITE | SHF_ALLOC);

  OutputSection<E> *tls_common =
    OutputSection<E>::get_instance(ctx, ".tls_common", SHT_NOBITS,
                                   SHF_WRITE | SHF_ALLOC | SHF_TLS);

  for (i64 i = this->first_global; i < this->elf_syms.size(); i++) {
    if (!this->elf_syms[i].is_common())
      continue;

    Symbol<E> &sym = *this->symbols[i];
    std::scoped_lock lock(sym.mu);

    if (sym.file != this) {
      if (ctx.arg.warn_common)
        Warn(ctx) << *this << ": multiple common symbols: " << sym;
      continue;
    }

    elf_sections2.push_back({});
    ElfShdr<E> &shdr = elf_sections2.back();
    memset(&shdr, 0, sizeof(shdr));

    bool is_tls = (sym.get_type() == STT_TLS);
    shdr.sh_flags = is_tls ? (SHF_ALLOC | SHF_TLS) : SHF_ALLOC;
    shdr.sh_type = SHT_NOBITS;
    shdr.sh_size = this->elf_syms[i].st_size;
    shdr.sh_addralign = this->elf_syms[i].st_value;

    i64 idx = this->elf_sections.size() + elf_sections2.size() - 1;
    std::unique_ptr<InputSection<E>> isec =
      std::make_unique<InputSection<E>>(ctx, *this,
                                        is_tls ? ".tls_common" : ".common",
                                        idx);
    isec->output_section = is_tls ? tls_common : common;

    sym.file = this;
    sym.shndx = idx;
    sym.value = 0;
    sym.sym_idx = i;
    sym.ver_idx = ctx.default_version;
    sym.is_weak = false;
    sym.is_imported = false;
    sym.is_exported = false;

    sections.push_back(std::move(isec));
  }
}

template <typename E>
static bool should_write_to_local_symtab(Context<E> &ctx, Symbol<E> &sym) {
  if (sym.get_type() == STT_SECTION)
    return false;

  // Local symbols are discarded if --discard-local is given or they
  // are in a mergeable section. I *believe* we exclude symbols in
  // mergeable sections because (1) there are too many and (2) they are
  // merged, so their origins shouldn't matter, but I don't really
  // know the rationale. Anyway, this is the behavior of the
  // traditional linkers.
  if (sym.name().starts_with(".L")) {
    if (ctx.arg.discard_locals)
      return false;

    if (InputSection<E> *isec = sym.get_input_section())
      if (isec->shdr().sh_flags & SHF_MERGE)
        return false;
  }

  return true;
}

template <typename E>
void ObjectFile<E>::compute_symtab(Context<E> &ctx) {
  if (ctx.arg.strip_all)
    return;

  auto is_alive = [&](Symbol<E> &sym) -> bool {
    if (!ctx.arg.gc_sections)
      return true;

    if (SectionFragment<E> *frag = sym.get_frag())
      return frag->is_alive;
    if (InputSection<E> *isec = sym.get_input_section())
      return isec->is_alive;
    return true;
  };

  // Compute the size of local symbols
  if (!ctx.arg.discard_all && !ctx.arg.strip_all && !ctx.arg.retain_symbols_file) {
    for (i64 i = 1; i < this->first_global; i++) {
      Symbol<E> &sym = *this->symbols[i];

      if (is_alive(sym) && should_write_to_local_symtab(ctx, sym)) {
        this->strtab_size += sym.name().size() + 1;
        this->num_local_symtab++;
        sym.write_to_symtab = true;
      }
    }
  }

  // Compute the size of global symbols.
  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];

    if (sym.file == this && is_alive(sym) &&
        (!ctx.arg.retain_symbols_file || sym.write_to_symtab)) {
      this->strtab_size += sym.name().size() + 1;
      // Global symbols can be demoted to local symbols based on visibility,
      // version scripts etc.
      if (sym.is_local())
        this->num_local_symtab++;
      else
        this->num_global_symtab++;
      sym.write_to_symtab = true;
    }
  }
}

template <typename E>
void ObjectFile<E>::populate_symtab(Context<E> &ctx) {
  ElfSym<E> *symtab_base = (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset);

  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  i64 strtab_off = this->strtab_offset;

  auto write_sym = [&](Symbol<E> &sym, i64 &symtab_idx) {
    ElfSym<E> &esym = symtab_base[symtab_idx++];
    esym = to_output_esym(ctx, sym);
    esym.st_name = strtab_off;
    write_string(strtab_base + strtab_off, sym.name());
    strtab_off += sym.name().size() + 1;
  };

  i64 local_symtab_idx = this->local_symtab_idx;
  i64 global_symtab_idx = this->global_symtab_idx;
  for (i64 i = 1; i < this->first_global; i++) {
    Symbol<E> &sym = *this->symbols[i];
    if (sym.write_to_symtab)
      write_sym(sym, local_symtab_idx);
  }

  for (i64 i = this->first_global; i < this->elf_syms.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    if (sym.file == this && sym.write_to_symtab) {
      if (sym.is_local())
        write_sym(sym, local_symtab_idx);
      else
        write_sym(sym, global_symtab_idx);
    }
  }
}

bool is_c_identifier(std::string_view name) {
  static std::regex re("[a-zA-Z_][a-zA-Z0-9_]*",
                       std::regex_constants::optimize);
  return std::regex_match(name.begin(), name.end(), re);
}

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputFile<E> &file) {
  if (file.is_dso) {
    out << path_clean(file.filename);
    return out;
  }

  ObjectFile<E> *obj = (ObjectFile<E> *)&file;
  if (obj->archive_name == "")
    out << path_clean(obj->filename);
  else
    out << path_clean(obj->archive_name) << "(" << obj->filename + ")";
  return out;
}

template <typename E>
SharedFile<E> *
SharedFile<E>::create(Context<E> &ctx, MappedFile<Context<E>> *mf) {
  SharedFile<E> *obj = new SharedFile(ctx, mf);
  ctx.dso_pool.emplace_back(obj);
  return obj;
}

template <typename E>
SharedFile<E>::SharedFile(Context<E> &ctx, MappedFile<Context<E>> *mf)
  : InputFile<E>(ctx, mf) {
  this->is_needed = ctx.as_needed;
  this->is_alive = !ctx.as_needed;
}

template <typename E>
std::string SharedFile<E>::get_soname(Context<E> &ctx) {
  if (ElfShdr<E> *sec = this->find_section(SHT_DYNAMIC))
    for (ElfDyn<E> &dyn : this->template get_data<ElfDyn<E>>(ctx, *sec))
      if (dyn.d_tag == DT_SONAME)
        return this->symbol_strtab.data() + dyn.d_val;

  if (this->mf->given_fullpath)
    return this->filename;

  return filepath(this->filename).filename().string();
}

template <typename E>
void SharedFile<E>::parse(Context<E> &ctx) {
  symtab_sec = this->find_section(SHT_DYNSYM);
  if (!symtab_sec)
    return;

  this->symbol_strtab = this->get_string(ctx, symtab_sec->sh_link);
  soname = get_soname(ctx);
  version_strings = read_verdef(ctx);

  // Read a symbol table.
  std::span<ElfSym<E>> esyms = this->template get_data<ElfSym<E>>(ctx, *symtab_sec);

  std::span<U16<E>> vers;
  if (ElfShdr<E> *sec = this->find_section(SHT_GNU_VERSYM))
    vers = this->template get_data<U16<E>>(ctx, *sec);

  for (i64 i = symtab_sec->sh_info; i < esyms.size(); i++) {
    u16 ver;
    if (vers.empty() || esyms[i].is_undef())
      ver = VER_NDX_GLOBAL;
    else
      ver = (vers[i] & ~VERSYM_HIDDEN);

    if (ver == VER_NDX_LOCAL)
      continue;

    std::string_view name = this->symbol_strtab.data() + esyms[i].st_name;
    bool is_hidden = (!vers.empty() && (vers[i] & VERSYM_HIDDEN));

    this->elf_syms2.push_back(esyms[i]);
    this->versyms.push_back(ver);

    if (is_hidden) {
      std::string_view mangled_name = save_string(
        ctx, std::string(name) + "@" + std::string(version_strings[ver]));
      this->symbols.push_back(get_symbol(ctx, mangled_name, name));
    } else {
      this->symbols.push_back(get_symbol(ctx, name));
    }
  }

  this->elf_syms = elf_syms2;
  this->first_global = 0;

  static Counter counter("dso_syms");
  counter += this->elf_syms.size();
}

// Symbol versioning is a GNU extension to the ELF file format. I don't
// particularly like the feature as it complicates the semantics of
// dynamic linking, but we need to support it anyway because it is
// mandatory on glibc-based systems such as most Linux distros.
//
// Let me explain what symbol versioning is. Symbol versioning is a
// mechanism to allow multiple symbols of the same name but of different
// versions live together in a shared object file. It's convenient if you
// want to make an API-breaking change to some function but want to keep
// old programs working with the newer libraries.
//
// With symbol versioning, dynamic symbols are resolved by (name, version)
// tuple instead of just by name. For example, glibc 2.35 defines two
// different versions of `posix_spawn`, `posix_spawn` of version
// "GLIBC_2.15" and that of version "GLIBC_2.2.5". Any executable that
// uses `posix_spawn` is linked either to that of "GLIBC_2.15" or that of
// "GLIBC_2.2.5"
//
// Versions are just stirngs, and no ordering is defined between them.
// For example, "GLIBC_2.15" is not considered a newer version of
// "GLIBC_2.2.5" or vice versa. They are considered just different.
//
// If a shared object file has versioned symbols, it contains a parallel
// array for the symbol table. Version strings can be found in that
// parallel table.
//
// One version is considered the "default" version for each shared object.
// If an undefiend symbol `foo` is resolved to a symbol defined by the
// shared object, it's marked so that it'll be resolved to (`foo`, the
// default version of the library) at load-time.
template <typename E>
std::vector<std::string_view> SharedFile<E>::read_verdef(Context<E> &ctx) {
  std::vector<std::string_view> ret(VER_NDX_LAST_RESERVED + 1);

  ElfShdr<E> *verdef_sec = this->find_section(SHT_GNU_VERDEF);
  if (!verdef_sec)
    return ret;

  std::string_view verdef = this->get_string(ctx, *verdef_sec);
  std::string_view strtab = this->get_string(ctx, verdef_sec->sh_link);

  ElfVerdef<E> *ver = (ElfVerdef<E> *)verdef.data();

  for (;;) {
    if (ret.size() <= ver->vd_ndx)
      ret.resize(ver->vd_ndx + 1);

    ElfVerdaux<E> *aux = (ElfVerdaux<E> *)((u8 *)ver + ver->vd_aux);
    ret[ver->vd_ndx] = strtab.data() + aux->vda_name;
    if (!ver->vd_next)
      break;

    ver = (ElfVerdef<E> *)((u8 *)ver + ver->vd_next);
  }
  return ret;
}

template <typename E>
void SharedFile<E>::resolve_symbols(Context<E> &ctx) {
  for (i64 i = 0; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = this->elf_syms[i];
    if (esym.is_undef())
      continue;

    std::scoped_lock lock(sym.mu);

    if (get_rank(this, esym, false) < get_rank(sym)) {
      sym.file = this;
      sym.shndx = 0;
      sym.value = esym.st_value;
      sym.sym_idx = i;
      sym.ver_idx = versyms[i];
      sym.is_weak = false;
      sym.is_imported = false;
      sym.is_exported = false;
    }
  }
}

template <typename E>
void
SharedFile<E>::mark_live_objects(Context<E> &ctx,
                                 std::function<void(InputFile<E> *)> feeder) {
  for (i64 i = 0; i < this->elf_syms.size(); i++) {
    const ElfSym<E> &esym = this->elf_syms[i];
    Symbol<E> &sym = *this->symbols[i];

    if (sym.traced)
      print_trace_symbol(ctx, *this, esym, sym);

    if (esym.is_undef() && sym.file && sym.file != this &&
        !sym.file->is_alive.exchange(true)) {
      feeder(sym.file);

      if (sym.traced)
        SyncOut(ctx) << "trace-symbol: " << *this << " keeps " << *sym.file
                     << " for " << sym;
    }
  }
}

template <typename E>
std::vector<Symbol<E> *> SharedFile<E>::find_aliases(Symbol<E> *sym) {
  assert(sym->file == this);
  std::vector<Symbol<E> *> vec;
  for (Symbol<E> *sym2 : this->symbols)
    if (sym2->file == this && sym != sym2 &&
        sym->esym().st_value == sym2->esym().st_value)
      vec.push_back(sym2);
  return vec;
}

template <typename E>
bool SharedFile<E>::is_readonly(Context<E> &ctx, Symbol<E> *sym) {
  ElfPhdr<E> *phdr = this->get_phdr();
  u64 val = sym->esym().st_value;

  for (i64 i = 0; i < this->get_ehdr().e_phnum; i++)
    if (phdr[i].p_type == PT_LOAD && !(phdr[i].p_flags & PF_W) &&
        phdr[i].p_vaddr <= val && val < phdr[i].p_vaddr + phdr[i].p_memsz)
      return true;
  return false;
}

template <typename E>
void SharedFile<E>::compute_symtab(Context<E> &ctx) {
  if (ctx.arg.strip_all)
    return;

  // Compute the size of global symbols.
  for (i64 i = this->first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];

    if (sym.file == this && (sym.is_imported || sym.is_exported) &&
        (!ctx.arg.retain_symbols_file || sym.write_to_symtab)) {
      this->strtab_size += sym.name().size() + 1;
      this->num_global_symtab++;
      sym.write_to_symtab = true;
    }
  }
}

template <typename E>
void SharedFile<E>::populate_symtab(Context<E> &ctx) {
  ElfSym<E> *symtab =
    (ElfSym<E> *)(ctx.buf + ctx.symtab->shdr.sh_offset) + this->global_symtab_idx;

  u8 *strtab = ctx.buf + ctx.strtab->shdr.sh_offset + this->strtab_offset;

  for (i64 i = this->first_global; i < this->elf_syms.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    if (sym.file != this || !sym.write_to_symtab)
      continue;

    ElfSym<E> &esym = *symtab++;
    esym.st_name = strtab - (ctx.buf + ctx.strtab->shdr.sh_offset);
    esym.st_value = 0;
    esym.st_size = 0;
    esym.st_type = STT_NOTYPE;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = sym.visibility;
    esym.st_shndx = SHN_UNDEF;

    write_string(strtab, sym.name());
    strtab += sym.name().size() + 1;
  }
}

#define INSTANTIATE(E)                                                  \
  template class InputFile<E>;                                          \
  template class ObjectFile<E>;                                         \
  template class SharedFile<E>;                                         \
  template std::ostream &operator<<(std::ostream &, const InputFile<E> &)

INSTANTIATE_ALL;

} // namespace mold::elf
