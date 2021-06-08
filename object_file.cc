#include "mold.h"

#include <cstring>
#include <regex>
#include <unistd.h>
#include <zlib.h>

template <typename E>
InputFile<E>::InputFile(Context<E> &ctx, MemoryMappedFile<E> *mb)
  : mb(mb), name(mb->name) {
  if (mb->size() < sizeof(ElfEhdr<E>))
    Fatal(ctx) << *this << ": file too small";
  if (memcmp(mb->data(ctx), "\177ELF", 4))
    Fatal(ctx) << *this << ": not an ELF file";

  ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)mb->data(ctx);
  is_dso = (ehdr.e_type == ET_DYN);

  ElfShdr<E> *sh_begin = (ElfShdr<E> *)(mb->data(ctx) + ehdr.e_shoff);

  // e_shnum contains the total number of sections in an object file.
  // Since it is a 16-bit integer field, it's not large enough to
  // represent >65535 sections. If an object file contains more than 65535
  // sections, the actual number is stored to sh_size field.
  i64 num_sections = (ehdr.e_shnum == 0) ? sh_begin->sh_size : ehdr.e_shnum;

  if (mb->data(ctx) + mb->size() < (u8 *)(sh_begin + num_sections))
    Fatal(ctx) << *this << ": e_shoff or e_shnum corrupted: "
            << mb->size() << " " << num_sections;
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
ObjectFile<E>::ObjectFile(Context<E> &ctx, MemoryMappedFile<E> *mb,
                          std::string archive_name, bool is_in_lib)
  : InputFile<E>(ctx, mb), archive_name(archive_name), is_in_lib(is_in_lib) {
  this->is_alive = !is_in_lib;
}

template <typename E>
ObjectFile<E>::ObjectFile() {}

template <typename E>
ObjectFile<E> *
ObjectFile<E>::create(Context<E> &ctx, MemoryMappedFile<E> *mb,
                      std::string archive_name, bool is_in_lib) {
  ObjectFile<E> *obj = new ObjectFile<E>(ctx, mb, archive_name, is_in_lib);
  ctx.owning_objs.push_back(std::unique_ptr<ObjectFile<E>>(obj));
  return obj;
}

template <typename E>
static bool is_debug_section(const ElfShdr<E> &shdr, std::string_view name) {
  return !(shdr.sh_flags & SHF_ALLOC) &&
         (name.starts_with(".debug") || name.starts_with(".zdebug"));
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
    data = data.substr(align_to(hdr.n_descsz, E::wordsize));

    if (hdr.n_type != NT_GNU_PROPERTY_TYPE_0 || name != "GNU")
      continue;

    while (!desc.empty()) {
      u32 type = *(u32 *)desc.data();
      u32 size = *(u32 *)(desc.data() + 4);
      desc = desc.substr(8);
      if (type == GNU_PROPERTY_X86_FEATURE_1_AND)
        ret |= *(u32 *)desc.data();
      desc = desc.substr(align_to(size, E::wordsize));
    }
  }
  return ret;
}

template <typename E>
std::pair<std::string_view, const ElfShdr<E> *>
ObjectFile<E>::uncompress_contents(Context<E> &ctx, const ElfShdr<E> &shdr,
                                   std::string_view name) {
  if (shdr.sh_type == SHT_NOBITS)
    return {{}, &shdr};

  auto do_uncompress = [&](std::string_view data, u64 size) {
    u8 *buf = new u8[size];
    ctx.owning_bufs.push_back(std::unique_ptr<u8[]>(buf));

    unsigned long size2 = size;
    if (uncompress(buf, &size2, (u8 *)&data[0], data.size()) != Z_OK)
      Fatal(ctx) << *this << ": " << name << ": uncompress failed";
    if (size != size2)
      Fatal(ctx) << *this << ": " << name << ": uncompress: invalid size";
    return std::string_view((char *)buf, size);
  };

  auto copy_shdr = [&](const ElfShdr<E> &shdr) {
    ElfShdr<E> *ret = new ElfShdr<E>;
    ctx.owning_shdrs.push_back(std::unique_ptr<ElfShdr<E>>(ret));
    *ret = shdr;
    return ret;
  };

  if (name.starts_with(".zdebug")) {
    // Old-style compressed section
    std::string_view data = this->get_string(ctx, shdr);
    if (!data.starts_with("ZLIB") || data.size() <= 12)
      Fatal(ctx) << *this << ": " << name << ": corrupted compressed section";
    u64 size = read64be((u8 *)&data[4]);
    std::string_view contents = do_uncompress(data.substr(12), size);

    ElfShdr<E> *shdr2 = copy_shdr(shdr);
    shdr2->sh_size = size;
    return {contents, shdr2};
  }

  if (shdr.sh_flags & SHF_COMPRESSED) {
    // New-style compressed section
    std::string_view data = this->get_string(ctx, shdr);
    if (data.size() < sizeof(ElfChdr<E>))
      Fatal(ctx) << *this << ": " << name << ": corrupted compressed section";
    ElfChdr<E> &hdr = *(ElfChdr<E> *)&data[0];
    data = data.substr(sizeof(ElfChdr<E>));

    if (hdr.ch_type != ELFCOMPRESS_ZLIB)
      Fatal(ctx) << *this << ": " << name << ": unsupported compression type";

    ElfShdr<E> *shdr2 = copy_shdr(shdr);
    shdr2->sh_flags &= ~(u64)(SHF_COMPRESSED);
    shdr2->sh_size = hdr.ch_size;
    shdr2->sh_addralign = hdr.ch_addralign;

    std::string_view contents = do_uncompress(data, hdr.ch_size);
    return {contents, shdr2};
  }

  return {this->get_string(ctx, shdr), &shdr};
}

template <typename E>
void ObjectFile<E>::initialize_sections(Context<E> &ctx) {
  // Read sections
  for (i64 i = 0; i < this->elf_sections.size(); i++) {
    const ElfShdr<E> &shdr = this->elf_sections[i];

    if ((shdr.sh_flags & SHF_EXCLUDE) && !(shdr.sh_flags & SHF_ALLOC))
      continue;

    switch (shdr.sh_type) {
    case SHT_GROUP: {
      // Get the signature of this section group.
      if (shdr.sh_info >= elf_syms.size())
        Fatal(ctx) << *this << ": invalid symbol index";
      const ElfSym<E> &sym = elf_syms[shdr.sh_info];
      std::string_view signature = symbol_strtab.data() + sym.st_name;

      // Get comdat group members.
      std::span<u32> entries = this->template get_data<u32>(ctx, shdr);

      if (entries.empty())
        Fatal(ctx) << *this << ": empty SHT_GROUP";
      if (entries[0] == 0)
        continue;
      if (entries[0] != GRP_COMDAT)
        Fatal(ctx) << *this << ": unsupported SHT_GROUP format";

      ComdatGroup *group = ctx.comdat_groups.insert(signature, ComdatGroup());
      comdat_groups.push_back({group, entries.subspan(1)});
      break;
    }
    case SHT_SYMTAB_SHNDX:
      symtab_shndx_sec = this->template get_data<u32>(ctx, shdr);
      break;
    case SHT_SYMTAB:
    case SHT_STRTAB:
    case SHT_REL:
    case SHT_RELA:
    case SHT_NULL:
      break;
    default: {
      std::string_view name = this->shstrtab.data() + shdr.sh_name;
      if (name == ".note.GNU-stack" || name.starts_with(".gnu.warning."))
        continue;

      if (name == ".note.gnu.property") {
        this->features = read_note_gnu_property(ctx, shdr);
        continue;
      }

      if ((ctx.arg.strip_all || ctx.arg.strip_debug) &&
          is_debug_section(shdr, name))
        continue;

      std::string_view contents;
      const ElfShdr<E> *shdr2;
      std::tie(contents, shdr2) = uncompress_contents(ctx, shdr, name);

      this->sections[i] =
        std::make_unique<InputSection<E>>(ctx, *this, *shdr2, name,
                                          contents, i);

      static Counter counter("regular_sections");
      counter++;
      break;
    }
    }
  }

  // Attach relocation sections to their target sections.
  for (i64 i = 0; i < this->elf_sections.size(); i++) {
    const ElfShdr<E> &shdr = this->elf_sections[i];
    if (shdr.sh_type != (E::is_rel ? SHT_REL : SHT_RELA))
      continue;

    if (shdr.sh_info >= sections.size())
      Fatal(ctx) << *this << ": invalid relocated section index: "
                 << (u32)shdr.sh_info;

    if (std::unique_ptr<InputSection<E>> &target = sections[shdr.sh_info]) {
      assert(target->relsec_idx == -1);
      target->relsec_idx = i;

      if (target->shdr.sh_flags & SHF_ALLOC) {
        i64 size = shdr.sh_size / sizeof(ElfRel<E>);
        target->rel_types.reset(new u8[size]);
      }
    }
  }
}

template <typename E>
void ObjectFile<E>::initialize_ehframe_sections(Context<E> &ctx) {
  for (i64 i = 0; i < sections.size(); i++) {
    std::unique_ptr<InputSection<E>> &isec = sections[i];
    if (isec && isec->is_alive && isec->name() == ".eh_frame") {
      read_ehframe(ctx, *isec);
      isec->is_ehframe = true;
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

  // Verify relocations.
  for (i64 i = 1; i < rels.size(); i++)
    if (rels[i].r_type != E::R_NONE &&
        rels[i].r_offset <= rels[i - 1].r_offset)
      Fatal(ctx) << isec << ": relocation offsets must increase monotonically";

  // Read CIEs and FDEs until empty.
  std::string_view contents = this->get_string(ctx, isec.shdr);
  i64 rel_idx = 0;

  for (std::string_view data = contents; !data.empty();) {
    i64 size = *(u32 *)data.data();
    if (size == 0) {
      if (data.size() != 4)
        Fatal(ctx) << isec << ": garbage at end of section";
      break;
    }

    i64 begin_offset = data.data() - contents.data();
    i64 end_offset = begin_offset + size + 4;
    i64 id = *(u32 *)(data.data() + 4);
    data = data.substr(size + 4);

    i64 rel_begin = rel_idx;
    while (rel_idx < rels.size() && rels[rel_idx].r_offset < end_offset)
      rel_idx++;
    assert(begin_offset <= rels[rel_begin].r_offset);

    if (id == 0) {
      // This is CIE.
      cies.push_back(CieRecord<E>(ctx, *this, isec, begin_offset, rel_begin));
    } else {
      // This is FDE.
      if (rel_begin == rel_idx) {
        // FDE has no valid relocation, which means FDE is dead from
        // the beginning. Compilers usually don't create such FDE, but
        // `ld -r` tend to generate such dead FDEs.
        continue;
      }

      if (rels[rel_begin].r_offset - begin_offset != 8)
        Fatal(ctx) << isec << ": FDE's first relocation should have offset 8";

      fdes.push_back(FdeRecord<E>(begin_offset, rel_begin));
    }
  }

  // Associate CIEs to FDEs.
  auto find_cie = [&](i64 offset) -> CieRecord<E> * {
    for (i64 i = cies_begin; i < cies.size(); i++)
      if (cies[i].input_offset == offset)
        return &cies[i];
    Fatal(ctx) << isec << ": bad FDE pointer";
  };

  for (i64 i = fdes_begin; i < fdes.size(); i++) {
    i64 cie_offset = *(i32 *)(contents.data() + fdes[i].input_offset + 4);
    fdes[i].cie = find_cie(fdes[i].input_offset + 4 - cie_offset);
  }

  // We assume that FDEs for the same input sections are contiguous
  // in `fdes` vector.
  std::stable_sort(fdes.begin() + fdes_begin, fdes.end(),
                   [&](const FdeRecord<E> &a, const FdeRecord<E> &b) {
    InputSection<E> *x = this->symbols[rels[a.rel_idx].r_sym]->input_section;
    InputSection<E> *y = this->symbols[rels[b.rel_idx].r_sym]->input_section;
    return x->get_priority() < y->get_priority();
  });

  // Associate FDEs to input sections.
  for (i64 i = fdes_begin; i < fdes.size();) {
    InputSection<E> *isec =
      this->symbols[rels[fdes[i].rel_idx].r_sym]->input_section;
    assert(isec->fde_begin == -1);
    isec->fde_begin = i++;

    while (i < fdes.size() &&
           isec == this->symbols[rels[fdes[i].rel_idx].r_sym]->input_section)
      i++;
    isec->fde_end = i;
  }
}

template <typename E>
static bool should_write_to_local_symtab(Context<E> &ctx, Symbol<E> &sym) {
  if (ctx.arg.discard_all || ctx.arg.strip_all || ctx.arg.retain_symbols_file)
    return false;
  if (sym.get_type() == STT_SECTION)
    return false;

  // Local symbols are discarded if --discard-local is given or they
  // are not in a mergeable section. I *believe* we exclude symbols in
  // mergeable sections because (1) they are too many and (2) they are
  // merged, so their origins shouldn't matter, but I dont' really
  // know the rationale. Anyway, this is the behavior of the
  // traditional linkers.
  if (sym.name().starts_with(".L")) {
    if (ctx.arg.discard_locals)
      return false;

    if (InputSection<E> *isec = sym.input_section)
      if (isec->shdr.sh_flags & SHF_MERGE)
        return false;
  }

  return true;
}

// Returns a symbol object for a given key. This function handles
// the -wrap option.
template <typename E>
static Symbol<E> *insert_symbol(Context<E> &ctx, const ElfSym<E> &esym,
                                std::string_view key, std::string_view name) {
  if (esym.is_undef() && name.starts_with("__real_") &&
      ctx.arg.wrap.count(name.substr(7))) {
    return Symbol<E>::intern(ctx, key.substr(7), name.substr(7));
  }

  Symbol<E> *sym = Symbol<E>::intern(ctx, key, name);

  if (esym.is_undef() && sym->wrap) {
    key = save_string(ctx, "__wrap_" + std::string(key));
    name = save_string(ctx, "__wrap_" + std::string(name));
    return Symbol<E>::intern(ctx, key, name);
  }
  return sym;
}

template <typename E>
void ObjectFile<E>::initialize_symbols(Context<E> &ctx) {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter += elf_syms.size();

  // Initialize local symbols
  this->local_syms.reset(new Symbol<E>[first_global]);
  new (&this->local_syms[0]) Symbol<E>;

  for (i64 i = 1; i < first_global; i++) {
    const ElfSym<E> &esym = elf_syms[i];
    std::string_view name = symbol_strtab.data() + esym.st_name;

    if (name.empty() && esym.st_type == STT_SECTION)
      if (InputSection<E> *sec = get_section(esym))
        name = sec->name();

    Symbol<E> &sym = this->local_syms[i];
    new (&sym) Symbol<E>(name);
    sym.file = this;
    sym.value = esym.st_value;
    sym.sym_idx = i;

    if (!esym.is_abs()) {
      if (esym.is_common())
        Fatal(ctx) << *this << ": common local symbol?";
      sym.input_section = get_section(esym);
    }

    if (should_write_to_local_symtab(ctx, sym)) {
      sym.write_to_symtab = true;
      strtab_size += sym.name().size() + 1;
      num_local_symtab++;
    }
  }

  this->symbols.resize(elf_syms.size());

  i64 num_globals = elf_syms.size() - first_global;
  sym_fragments.resize(elf_syms.size());
  symvers.resize(num_globals);

  for (i64 i = 0; i < first_global; i++)
    this->symbols[i] = &this->local_syms[i];

  // Initialize global symbols
  for (i64 i = first_global; i < elf_syms.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];

    // Get a symbol name
    std::string_view key = symbol_strtab.data() + esym.st_name;
    std::string_view name = key;

    // Parse symbol version after atsign
    if (i64 pos = name.find('@'); pos != name.npos) {
      std::string_view ver = name.substr(pos + 1);
      name = name.substr(0, pos);
      if (ver.starts_with('@'))
        key = name;
      if (esym.is_defined())
        symvers[i - first_global] = ver.data();
    }

    this->symbols[i] = insert_symbol(ctx, esym, key, name);
    if (esym.is_common())
      has_common_symbol = true;
  }
}

template <typename E>
struct MergeableSection {
  std::vector<SectionFragment<E> *> fragments;
  std::vector<u32> frag_offsets;
};

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
static MergeableSection<E>
split_section(Context<E> &ctx, InputSection<E> &sec) {
  MergeableSection<E> rec;

  MergedSection<E> *parent =
    MergedSection<E>::get_instance(ctx, sec.name(), sec.shdr.sh_type,
                                   sec.shdr.sh_flags);

  std::string_view data = sec.contents;
  const char *begin = data.data();
  u64 entsize = sec.shdr.sh_entsize;

  static_assert(sizeof(SectionFragment<E>::alignment) == 2);
  if (sec.shdr.sh_addralign >= UINT16_MAX)
    Fatal(ctx) << sec << ": alignment too large";

  if (sec.shdr.sh_flags & SHF_STRINGS) {
    while (!data.empty()) {
      size_t end = find_null(data, entsize);
      if (end == data.npos)
        Fatal(ctx) << sec << ": string is not null terminated";

      std::string_view substr = data.substr(0, end + entsize);
      data = data.substr(end + entsize);

      SectionFragment<E> *frag = parent->insert(substr, sec.shdr.sh_addralign);
      rec.fragments.push_back(frag);
      rec.frag_offsets.push_back(substr.data() - begin);
    }
  } else {
    if (data.size() % entsize)
      Fatal(ctx) << sec << ": section size is not multiple of sh_entsize";

    while (!data.empty()) {
      std::string_view substr = data.substr(0, entsize);
      data = data.substr(entsize);

      SectionFragment<E> *frag = parent->insert(substr, sec.shdr.sh_addralign);
      rec.fragments.push_back(frag);
      rec.frag_offsets.push_back(substr.data() - begin);
    }
  }

  static Counter counter("string_fragments");
  counter += rec.fragments.size();

  return rec;
}

template <typename E>
void ObjectFile<E>::initialize_mergeable_sections(Context<E> &ctx) {
  std::vector<MergeableSection<E>> mergeable_sections(sections.size());

  for (i64 i = 0; i < sections.size(); i++) {
    std::unique_ptr<InputSection<E>> &isec = sections[i];
    if (isec && isec->is_alive && (isec->shdr.sh_flags & SHF_MERGE)) {
      mergeable_sections[i] = split_section(ctx, *isec);
      isec->is_alive = false;
    }
  }

  // Initialize rel_fragments
  for (std::unique_ptr<InputSection<E>> &isec : sections) {
    if (!isec || !isec->is_alive)
      continue;

    std::span<ElfRel<E>> rels = isec->get_rels(ctx);
    if (rels.empty())
      continue;

    i64 len = 0;

    for (i64 i = 0; i < rels.size(); i++) {
      const ElfRel<E> &rel = rels[i];
      const ElfSym<E> &esym = elf_syms[rel.r_sym];

      if (esym.st_type == STT_SECTION) {
        MergeableSection<E> &m = mergeable_sections[get_shndx(esym)];
        if (!m.fragments.empty())
          len++;
      }
    }

    if (len == 0)
      continue;

    isec->rel_fragments.reset(new SectionFragmentRef<E>[len + 1]);
    i64 frag_idx = 0;

    for (i64 i = 0; i < rels.size(); i++) {
      const ElfRel<E> &rel = rels[i];
      const ElfSym<E> &esym = elf_syms[rel.r_sym];
      if (esym.st_type != STT_SECTION)
        continue;

      MergeableSection<E> &m = mergeable_sections[get_shndx(esym)];
      if (m.fragments.empty())
        continue;

      i64 offset = esym.st_value + isec->get_addend(rel);
      std::span<u32> offsets = m.frag_offsets;

      auto it = std::upper_bound(offsets.begin(), offsets.end(), offset);
      if (it == offsets.begin())
        Fatal(ctx) << *this << ": bad relocation at " << rel.r_sym;
      i64 idx = it - 1 - offsets.begin();

      isec->rel_fragments[frag_idx++] = {m.fragments[idx], (i32)i,
                                         (i32)(offset - offsets[idx])};
    }

    isec->rel_fragments[frag_idx++] = {nullptr, -1, -1};
  }

  // Initialize sym_fragments
  for (i64 i = 0; i < elf_syms.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];
    if (esym.is_abs() || esym.is_common())
      continue;

    MergeableSection<E> &m = mergeable_sections[get_shndx(esym)];
    if (m.fragments.empty())
      continue;

    std::span<u32> offsets = m.frag_offsets;

    auto it = std::upper_bound(offsets.begin(), offsets.end(), esym.st_value);
    if (it == offsets.begin())
      Fatal(ctx) << *this << ": bad symbol value: " << esym.st_value;
    i64 idx = it - 1 - offsets.begin();

    if (i < first_global)
      this->symbols[i]->value = esym.st_value - offsets[idx];

    sym_fragments[i].frag = m.fragments[idx];
    sym_fragments[i].addend = esym.st_value - offsets[idx];
  }

  for (MergeableSection<E> &m : mergeable_sections)
    fragments.insert(fragments.end(), m.fragments.begin(), m.fragments.end());
}

template <typename E>
void ObjectFile<E>::parse(Context<E> &ctx) {
  sections.resize(this->elf_sections.size());
  symtab_sec = this->find_section(SHT_SYMTAB);

  if (symtab_sec) {
    first_global = symtab_sec->sh_info;
    elf_syms = this->template get_data<ElfSym<E>>(ctx, *symtab_sec);
    symbol_strtab = this->get_string(ctx, symtab_sec->sh_link);
  }

  initialize_sections(ctx);
  initialize_symbols(ctx);
  initialize_mergeable_sections(ctx);
  initialize_ehframe_sections(ctx);
}

// Symbols with higher priorities overwrites symbols with lower priorities.
// Here is the list of priorities, from the highest to the lowest.
//
//  1. Strong defined symbol
//  2. Common symbol
//  3. Weak defined symbol
//  4. Strong or weak defined symbol in an archive member
//  5. Unclaimed (nonexistent) symbol
//
// Ties are broken by file priority.
template <typename E>
static u64 get_rank(InputFile<E> *file, const ElfSym<E> &esym,
                    InputSection<E> *isec) {
  if (esym.st_bind == STB_WEAK)
    return (3 << 24) + file->priority;
  if (esym.is_common())
    return (2 << 24) + file->priority;
  return (1 << 24) + file->priority;
}

template <typename E>
static u64 get_rank(const Symbol<E> &sym) {
  if (!sym.file)
    return 5 << 24;
  if (sym.is_lazy)
    return (4 << 24) + sym.file->priority;
  return get_rank(sym.file, sym.esym(), sym.input_section);
}

template <typename E>
void ObjectFile<E>::maybe_override_symbol(Context<E> &ctx, Symbol<E> &sym,
                                          i64 symidx) {
  InputSection<E> *isec = nullptr;
  const ElfSym<E> &esym = elf_syms[symidx];
  if (!esym.is_abs() && !esym.is_common())
    isec = get_section(esym);

  u64 new_rank = get_rank(this, esym, isec);

  std::lock_guard lock(sym.mu);
  u64 existing_rank = get_rank(sym);

  if (new_rank < existing_rank) {
    sym.file = this;
    sym.input_section = isec;

    if (SectionFragmentRef<E> &ref = sym_fragments[symidx]; ref.frag)
      sym.value = ref.addend;
    else
      sym.value = esym.st_value;

    sym.sym_idx = symidx;
    sym.ver_idx = ctx.arg.default_version;
    sym.is_lazy = false;
    sym.is_imported = false;
    sym.is_exported = false;

    if (sym.traced) {
      bool is_weak = (esym.st_bind == STB_WEAK);
      SyncOut(ctx) << "trace-symbol: " << *sym.file
                << (is_weak ? ": weak definition of " : ": definition of ")
                << sym;
    }
  }
}

template <typename E>
void ObjectFile<E>::merge_visibility(Context<E> &ctx, Symbol<E> &sym,
                                     u8 visibility) {
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

  u8 val = sym.visibility;

  while (priority(visibility) < priority(val))
    if (sym.visibility.compare_exchange_strong(val, visibility))
      break;
}

template <typename E>
void ObjectFile<E>::resolve_lazy_symbols(Context<E> &ctx) {
  assert(is_in_lib);

  for (i64 i = first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = elf_syms[i];

    if (!esym.is_defined())
      continue;

    std::lock_guard lock(sym.mu);
    if (!sym.file || this->priority < sym.file->priority) {
      sym.file = this;
      sym.is_lazy = true;
      if (sym.traced)
        SyncOut(ctx) << "trace-symbol: " << *sym.file
                  << ": lazy definition of " << sym;
    }
  }
}

template <typename E>
void ObjectFile<E>::resolve_regular_symbols(Context<E> &ctx) {
  assert(!is_in_lib);

  for (i64 i = first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = elf_syms[i];
    merge_visibility(ctx, sym, exclude_libs ? STV_HIDDEN : esym.st_visibility);

    if (esym.is_defined())
      maybe_override_symbol(ctx, sym, i);
  }
}

template <typename E>
void
ObjectFile<E>::mark_live_objects(Context<E> &ctx,
                                 std::function<void(ObjectFile<E> *)> feeder) {
  assert(this->is_alive);

  for (i64 i = first_global; i < this->symbols.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];
    Symbol<E> &sym = *this->symbols[i];
    merge_visibility(ctx, sym, exclude_libs ? STV_HIDDEN : esym.st_visibility);

    if (esym.is_defined()) {
      if (is_in_lib)
        maybe_override_symbol(ctx, sym, i);
      continue;
    }

    bool is_weak = (esym.st_bind == STB_WEAK);

    if (sym.traced) {
      SyncOut(ctx) << "trace-symbol: " << *this
                << (is_weak ? ": weak reference to " : ": reference to ")
                << sym;
    }

    if (!is_weak && sym.file && !sym.file->is_alive.exchange(true)) {
      feeder((ObjectFile<E> *)sym.file);
      if (sym.traced)
        SyncOut(ctx) << "trace-symbol: " << *this << " keeps " << *sym.file
                  << " for " << sym;
    }
  }
}

template <typename E>
void ObjectFile<E>::convert_undefined_weak_symbols(Context<E> &ctx) {
  for (i64 i = first_global; i < this->symbols.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];

    if (esym.is_undef() && esym.st_bind == STB_WEAK) {
      Symbol<E> &sym = *this->symbols[i];
      std::lock_guard lock(sym.mu);

      bool is_new = !sym.file;
      bool tie_but_higher_priority =
        !is_new && sym.is_undef_weak() && this->priority < sym.file->priority;

      if (is_new || tie_but_higher_priority) {
        sym.file = this;
        sym.input_section = nullptr;
        sym.value = 0;
        sym.sym_idx = i;
        sym.ver_idx = ctx.arg.default_version;
        sym.is_lazy = false;

        if (ctx.arg.shared)
          sym.is_imported = true;

        if (sym.traced)
          SyncOut(ctx) << "trace-symbol: " << *this
                    << ": unresolved weak symbol " << sym;
      }
    }
  }
}

template <typename E>
void ObjectFile<E>::resolve_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    u32 cur = group->owner;
    while (cur == -1 || cur > this->priority)
      if (group->owner.compare_exchange_weak(cur, this->priority))
        break;
  }
}

template <typename E>
void ObjectFile<E>::eliminate_duplicate_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    if (group->owner == this->priority)
      continue;

    std::span<u32> entries = pair.second;
    for (u32 i : entries)
      if (sections[i])
        sections[i]->kill();
  }
}

template <typename E>
void ObjectFile<E>::claim_unresolved_symbols() {
  if (!this->is_alive)
    return;

  for (i64 i = first_global; i < this->symbols.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];
    Symbol<E> &sym = *this->symbols[i];

    if (esym.is_defined())
      continue;

    std::lock_guard lock(sym.mu);
    if (sym.sym_idx == -1 || sym.is_undef()) {
      if (sym.file && sym.file->priority < this->priority)
        continue;
      sym.file = this;
      sym.value = 0;
      sym.sym_idx = i;
      sym.is_imported = true;
      sym.is_exported = false;
    }
  }
}

template <typename E>
void ObjectFile<E>::scan_relocations(Context<E> &ctx) {
  // Scan relocations against seciton contents
  for (std::unique_ptr<InputSection<E>> &isec : sections)
    if (isec && isec->is_alive && (isec->shdr.sh_flags & SHF_ALLOC))
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

template <typename E>
void ObjectFile<E>::convert_common_symbols(Context<E> &ctx) {
  if (!has_common_symbol)
    return;

  OutputSection<E> *osec =
    OutputSection<E>::get_instance(ctx, ".common", SHT_NOBITS,
                                   SHF_WRITE | SHF_ALLOC);

  for (i64 i = first_global; i < elf_syms.size(); i++) {
    if (!elf_syms[i].is_common())
      continue;

    Symbol<E> *sym = this->symbols[i];
    if (sym->file != this) {
      if (ctx.arg.warn_common)
        Warn(ctx) << *this << ": " << "multiple common symbols: " << *sym;
      continue;
    }

    assert(sym->esym().st_value);

    auto *shdr = new ElfShdr<E>;
    ctx.owning_shdrs.push_back(std::unique_ptr<ElfShdr<E>>(shdr));

    memset(shdr, 0, sizeof(*shdr));
    shdr->sh_flags = SHF_ALLOC;
    shdr->sh_type = SHT_NOBITS;
    shdr->sh_size = elf_syms[i].st_size;
    shdr->sh_addralign = sym->esym().st_value;

    std::unique_ptr<InputSection<E>> isec =
      std::make_unique<InputSection<E>>(ctx, *this, *shdr, ".common",
                                        std::string_view(), sections.size());
    isec->output_section = osec;
    sym->input_section = isec.get();
    sym->value = 0;
    sections.push_back(std::move(isec));
  }
}

template <typename E>
static bool should_write_to_global_symtab(Symbol<E> &sym) {
  return sym.get_type() != STT_SECTION && sym.is_alive();
}

template <typename E>
void ObjectFile<E>::compute_symtab(Context<E> &ctx) {
  if (ctx.arg.retain_symbols_file) {
    std::span<Symbol<E> *> syms(this->symbols);
    for (Symbol<E> *sym : syms.subspan(first_global)) {
      if (sym->file == this && sym->write_to_symtab) {
        strtab_size += sym->name().size() + 1;
        num_global_symtab++;
      }
    }
    return;
  }

  if (ctx.arg.strip_all)
    return;

  if (ctx.arg.gc_sections && !ctx.arg.discard_all) {
    // Detect symbols pointing to sections discarded by -gc-sections
    // to not copy them to symtab.
    for (i64 i = 1; i < first_global; i++) {
      Symbol<E> &sym = *this->symbols[i];

      if (sym.write_to_symtab && !sym.is_alive()) {
        strtab_size -= sym.name().size() + 1;
        num_local_symtab--;
        sym.write_to_symtab = false;
      }
    }
  }

  // Compute the size of global symbols.
  for (i64 i = first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];

    if (sym.file == this && should_write_to_global_symtab(sym)) {
      strtab_size += sym.name().size() + 1;
      sym.write_to_symtab = true;
      num_global_symtab++;
    }
  }
}

template <typename E>
void ObjectFile<E>::write_symtab(Context<E> &ctx) {
  u8 *symtab_base = ctx.buf + ctx.symtab->shdr.sh_offset;
  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  i64 strtab_off = strtab_offset;
  i64 symtab_off;

  auto write_sym = [&](Symbol<E> &sym) {
    ElfSym<E> &esym = *(ElfSym<E> *)(symtab_base + symtab_off);
    symtab_off += sizeof(esym);

    esym = sym.esym();
    esym.st_name = strtab_off;

    if (sym.get_type() == STT_TLS)
      esym.st_value = sym.get_addr(ctx) - ctx.tls_begin;
    else
      esym.st_value = sym.get_addr(ctx);

    if (sym.input_section)
      esym.st_shndx = sym.input_section->output_section->shndx;
    else if (sym.shndx)
      esym.st_shndx = sym.shndx;
    else if (esym.is_undef())
      esym.st_shndx = SHN_UNDEF;
    else
      esym.st_shndx = SHN_ABS;

    write_string(strtab_base + strtab_off, sym.name());
    strtab_off += sym.name().size() + 1;
  };

  symtab_off = local_symtab_offset;
  for (i64 i = 1; i < first_global; i++) {
    Symbol<E> &sym = *this->symbols[i];
    if (sym.write_to_symtab)
      write_sym(sym);
  }

  symtab_off = global_symtab_offset;
  for (i64 i = first_global; i < elf_syms.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    if (sym.file == this && sym.write_to_symtab)
      write_sym(sym);
  }
}

bool is_c_identifier(std::string_view name) {
  static std::regex re("[a-zA-Z_][a-zA-Z0-9_]*");
  return std::regex_match(name.begin(), name.end(), re);
}

template <typename E>
ObjectFile<E> *
ObjectFile<E>::create_internal_file(Context<E> &ctx) {
  ObjectFile<E> *obj = new ObjectFile<E>;
  ctx.owning_objs.push_back(std::unique_ptr<ObjectFile<E>>(obj));

  // Create linker-synthesized symbols.
  auto *esyms = new std::vector<ElfSym<E>>(1);
  obj->symbols.push_back(new Symbol<E>);
  obj->first_global = 1;
  obj->is_alive = true;
  obj->priority = 1;

  auto add = [&](std::string_view name) {
    ElfSym<E> esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = STV_HIDDEN;
    esyms->push_back(esym);

    Symbol<E> *sym = Symbol<E>::intern(ctx, name);
    obj->symbols.push_back(sym);
    return sym;
  };

  ctx.__ehdr_start = add("__ehdr_start");
  ctx.__init_array_start = add("__init_array_start");
  ctx.__init_array_end = add("__init_array_end");
  ctx.__fini_array_start = add("__fini_array_start");
  ctx.__fini_array_end = add("__fini_array_end");
  ctx.__preinit_array_start = add("__preinit_array_start");
  ctx.__preinit_array_end = add("__preinit_array_end");
  ctx._DYNAMIC = add("_DYNAMIC");
  ctx._GLOBAL_OFFSET_TABLE_ = add("_GLOBAL_OFFSET_TABLE_");
  ctx.__bss_start = add("__bss_start");
  ctx._end = add("_end");
  ctx._etext = add("_etext");
  ctx._edata = add("_edata");
  ctx.__executable_start = add("__executable_start");

  ctx.__rel_iplt_start =
    add(E::is_rel ? "__rel_iplt_start" : "__rela_iplt_start");
  ctx.__rel_iplt_end =
    add(E::is_rel ? "__rel_iplt_end" : "__rela_iplt_end");

  if (ctx.arg.eh_frame_hdr)
    ctx.__GNU_EH_FRAME_HDR = add("__GNU_EH_FRAME_HDR");

  if (!Symbol<E>::intern(ctx, "end")->file)
    ctx.end = add("end");
  if (!Symbol<E>::intern(ctx, "etext")->file)
    ctx.etext = add("etext");
  if (!Symbol<E>::intern(ctx, "edata")->file)
    ctx.edata = add("edata");

  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (!is_c_identifier(chunk->name))
      continue;

    add(save_string(ctx, "__start_" + std::string(chunk->name)));
    add(save_string(ctx, "__stop_" + std::string(chunk->name)));
  }

  obj->elf_syms = *esyms;
  obj->sym_fragments.resize(obj->elf_syms.size());

  i64 num_globals = obj->elf_syms.size() - obj->first_global;
  obj->symvers.resize(num_globals);

  ctx.on_exit.push_back([=]() {
    delete esyms;
    delete obj->symbols[0];
  });

  return obj;
}

template <typename E>
std::ostream &operator<<(std::ostream &out, const InputFile<E> &file) {
  if (file.is_dso) {
    out << path_clean(file.name);
    return out;
  }

  ObjectFile<E> *obj = (ObjectFile<E> *)&file;
  if (obj->archive_name == "")
    out << path_clean(obj->name);
  else
    out << path_clean(obj->archive_name) << "(" << obj->name + ")";
  return out;
}

template <typename E>
SharedFile<E> *
SharedFile<E>::create(Context<E> &ctx, MemoryMappedFile<E> *mb) {
  SharedFile<E> *obj = new SharedFile(ctx, mb);
  ctx.owning_dsos.push_back(std::unique_ptr<SharedFile<E>>(obj));
  return obj;
}

template <typename E>
SharedFile<E>::SharedFile(Context<E> &ctx, MemoryMappedFile<E> *mb)
  : InputFile<E>(ctx, mb) {
  this->is_alive = !ctx.as_needed;
}

template <typename E>
std::string_view SharedFile<E>::get_soname(Context<E> &ctx) {
  if (ElfShdr<E> *sec = this->find_section(SHT_DYNAMIC))
    for (ElfDyn<E> &dyn : this->template get_data<ElfDyn<E>>(ctx, *sec))
      if (dyn.d_tag == DT_SONAME)
        return symbol_strtab.data() + dyn.d_val;
  if (this->mb->given_fullpath)
    return this->name;
  return path_filename(this->name);
}

template <typename E>
void SharedFile<E>::parse(Context<E> &ctx) {
  symtab_sec = this->find_section(SHT_DYNSYM);
  if (!symtab_sec)
    return;

  symbol_strtab = this->get_string(ctx, symtab_sec->sh_link);
  soname = get_soname(ctx);
  version_strings = read_verdef(ctx);

  // Read a symbol table.
  i64 first_global = symtab_sec->sh_info;
  std::span<ElfSym<E>> esyms =
    this->template get_data<ElfSym<E>>(ctx, *symtab_sec);

  std::span<u16> vers;
  if (ElfShdr<E> *sec = this->find_section(SHT_GNU_VERSYM))
    vers = this->template get_data<u16>(ctx, *sec);

  for (i64 i = first_global; i < esyms.size(); i++) {
    std::string_view name = symbol_strtab.data() + esyms[i].st_name;

    if (!esyms[i].is_defined()) {
      undefs.push_back(Symbol<E>::intern(ctx, name));
      continue;
    }

    if (vers.empty()) {
      elf_syms.push_back(&esyms[i]);
      versyms.push_back(VER_NDX_GLOBAL);
      this->symbols.push_back(Symbol<E>::intern(ctx, name));
    } else {
      u16 ver = vers[i] & ~VERSYM_HIDDEN;
      if (ver == VER_NDX_LOCAL)
        continue;

      std::string verstr(version_strings[ver]);
      std::string_view mangled =
        save_string(ctx, std::string(name) + "@" + verstr);

      elf_syms.push_back(&esyms[i]);
      versyms.push_back(ver);
      this->symbols.push_back(Symbol<E>::intern(ctx, mangled, name));

      if (!(vers[i] & VERSYM_HIDDEN)) {
        elf_syms.push_back(&esyms[i]);
        versyms.push_back(ver);
        this->symbols.push_back(Symbol<E>::intern(ctx, name));
      }
    }
  }

  static Counter counter("dso_syms");
  counter += elf_syms.size();
}

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
void SharedFile<E>::resolve_dso_symbols(Context<E> &ctx) {
  for (i64 i = 0; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = *elf_syms[i];

    std::lock_guard lock(sym.mu);

    if (!sym.file || this->priority < sym.file->priority) {
      sym.file = this;
      sym.input_section = nullptr;
      sym.value = esym.st_value;
      sym.sym_idx = i;
      sym.ver_idx = versyms[i];
      sym.is_weak = true;
      sym.is_imported = true;
      sym.is_exported = false;

      if (sym.traced)
        SyncOut(ctx) << "trace-symbol: " << *sym.file << ": definition of "
                     << sym;
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
  ElfEhdr<E> *ehdr = (ElfEhdr<E> *)this->mb->data(ctx);
  ElfPhdr<E> *phdr = (ElfPhdr<E> *)(this->mb->data(ctx) + ehdr->e_phoff);
  u64 val = sym->esym().st_value;

  for (i64 i = 0; i < ehdr->e_phnum; i++)
    if (phdr[i].p_type == PT_LOAD && !(phdr[i].p_flags & PF_W) &&
        phdr[i].p_vaddr <= val && val < phdr[i].p_vaddr + phdr[i].p_memsz)
      return true;
  return false;
}

template <typename E>
i64 FdeRecord<E>::size() const {
  return *(u32 *)(cie->contents.data() + input_offset) + 4;
}

template <typename E>
std::string_view FdeRecord<E>::get_contents() const {
  return cie->contents.substr(input_offset, size());
}

template <typename E>
std::span<ElfRel<E>> FdeRecord<E>::get_rels() const {
  std::span<ElfRel<E>> rels = cie->rels;
  i64 size = get_contents().size();
  i64 end = rel_idx;
  while (end < rels.size() && rels[end].r_offset < input_offset + size)
    end++;
  return rels.subspan(rel_idx, end - rel_idx);
}

template <typename E>
i64 CieRecord<E>::size() const {
  return *(u32 *)(contents.data() + input_offset) + 4;
}

template <typename E>
std::string_view CieRecord<E>::get_contents() const {
  return contents.substr(input_offset, size());
}

template <typename E>
std::span<ElfRel<E>> CieRecord<E>::get_rels() const {
  i64 size = get_contents().size();
  i64 end = rel_idx;
  while (end < rels.size() && rels[end].r_offset < input_offset + size)
    end++;
  return rels.subspan(rel_idx, end - rel_idx);
}

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

#define INSTANTIATE(E)                                                  \
  template class ObjectFile<E>;                                         \
  template class SharedFile<E>;                                         \
  template class CieRecord<E>;                                          \
  template class FdeRecord<E>;                                          \
  template std::ostream &operator<<(std::ostream &, const InputFile<E> &)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
