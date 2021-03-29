#include "mold.h"

#include <cstring>
#include <fcntl.h>
#include <regex>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

template <typename E>
MemoryMappedFile<E> *MemoryMappedFile<E>::open(std::string path) {
  struct stat st;
  if (stat(path.c_str(), &st) == -1)
    return nullptr;
  u64 mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
  return new MemoryMappedFile(path, nullptr, st.st_size, mtime);
}

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::must_open(Context<E> &ctx, std::string path) {
  if (MemoryMappedFile *mb = MemoryMappedFile::open(path))
    return mb;
  Fatal(ctx) << "cannot open " << path;
}

template <typename E>
u8 *MemoryMappedFile<E>::data(Context<E> &ctx) {
  if (data_)
    return data_;

  std::lock_guard lock(mu);
  if (data_)
    return data_;

  i64 fd = ::open(name.c_str(), O_RDONLY);
  if (fd == -1)
    Fatal(ctx) << name << ": cannot open: " << strerror(errno);

  data_ = (u8 *)mmap(nullptr, size_, PROT_READ, MAP_PRIVATE, fd, 0);
  if (data_ == MAP_FAILED)
    Fatal(ctx) << name << ": mmap failed: " << strerror(errno);
  close(fd);
  return data_;
}

template <typename E>
MemoryMappedFile<E> *
MemoryMappedFile<E>::slice(std::string name, u64 start, u64 size) {
  MemoryMappedFile *mb = new MemoryMappedFile(name, data_ + start, size);
  mb->parent = this;
  return mb;
}

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
std::string_view
InputFile<E>::get_string(Context<E> &ctx, const ElfShdr<E> &shdr) {
  u8 *begin = mb->data(ctx) + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb->data(ctx) + mb->size() < end)
    Fatal(ctx) << *this << ": shdr corrupted";
  return {(char *)begin, (char *)end};
}

template <typename E>
std::string_view InputFile<E>::get_string(Context<E> &ctx, i64 idx) {
  assert(idx < elf_sections.size());

  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index: " << idx;
  return this->get_string(ctx, elf_sections[idx]);
}

template <typename E>
template <typename T>
std::span<T> InputFile<E>::get_data(Context<E> &ctx, const ElfShdr<E> &shdr) {
  std::string_view view = this->get_string(ctx, shdr);
  if (view.size() % sizeof(T))
    Fatal(ctx) << *this << ": corrupted section";
  return {(T *)view.data(), view.size() / sizeof(T)};
}

template <typename E>
template <typename T>
std::span<T> InputFile<E>::get_data(Context<E> &ctx, i64 idx) {
  if (elf_sections.size() <= idx)
    Fatal(ctx) << *this << ": invalid section index";
  return this->template get_data<T>(elf_sections[idx]);
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
static bool is_debug_section(const ElfShdr<E> &shdr, std::string_view name) {
  return !(shdr.sh_flags & SHF_ALLOC) &&
         (name.starts_with(".debug") || name.starts_with(".zdebug"));
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

      static ConcurrentMap<ComdatGroup> map;
      ComdatGroup *group = map.insert(signature, ComdatGroup());
      comdat_groups.push_back({group, entries.subspan(1)});

      static Counter counter("comdats");
      counter++;
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
      if (name == ".note.GNU-stack" || name == ".note.gnu.property")
        continue;

      if ((ctx.arg.strip_all || ctx.arg.strip_debug) &&
          is_debug_section(shdr, name))
        continue;

      this->sections[i] = new InputSection(ctx, *this, shdr, name, i);

      static Counter counter("regular_sections");
      counter++;
      break;
    }
    }
  }

  // Attach relocation sections to their target sections.
  for (const ElfShdr<E> &shdr : this->elf_sections) {
    if (shdr.sh_type != SHT_RELA)
      continue;

    if (shdr.sh_info >= sections.size())
      Fatal(ctx) << *this << ": invalid relocated section index: "
              << (u32)shdr.sh_info;

    if (InputSection<E> *target = sections[shdr.sh_info]) {
      target->rels = this->template get_data<ElfRela<E>>(ctx, shdr);
      target->has_fragments.resize(target->rels.size());
      if (target->shdr.sh_flags & SHF_ALLOC)
        target->rel_types.resize(target->rels.size());
    }
  }
}

template <typename E>
void ObjectFile<E>::initialize_ehframe_sections(Context<E> &ctx) {
  for (i64 i = 0; i < sections.size(); i++) {
    InputSection<E> *isec = sections[i];
    if (isec && isec->name == ".eh_frame") {
      read_ehframe(ctx, *isec);
      isec->is_ehframe = true;
      sections[i] = nullptr;
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
  std::span<ElfRela<E>> rels = isec.rels;
  std::string_view data = this->get_string(ctx, isec.shdr);
  const char *begin = data.data();

  if (data.empty()) {
    cies.push_back(CieRecord<E>{data});
    return;
  }

  std::unordered_map<i64, i64> offset_to_cie;
  i64 cur_cie = -1;
  i64 cur_cie_offset = -1;

  for (ElfRela<E> rel : rels)
    if (rel.r_type != R_X86_64_32 && rel.r_type != R_X86_64_64 &&
        rel.r_type != R_X86_64_PC32 && rel.r_type != R_X86_64_PC64)
      Fatal(ctx) << isec << ": unsupported relocation type: " << rel.r_type;

  while (!data.empty()) {
    i64 size = *(u32 *)data.data();
    if (size == 0) {
      if (data.size() != 4)
        Fatal(ctx) << isec << ": garbage at end of section";
      cies.push_back(CieRecord<E>{data});
      return;
    }

    i64 begin_offset = data.data() - begin;
    i64 end_offset = begin_offset + size + 4;

    if (!rels.empty() && rels[0].r_offset < begin_offset)
      Fatal(ctx) << isec << ": unsupported relocation order";

    std::string_view contents = data.substr(0, size + 4);
    data = data.substr(size + 4);
    i64 id = *(u32 *)(contents.data() + 4);

    std::vector<EhReloc<E>> eh_rels;
    while (!rels.empty() && rels[0].r_offset < end_offset) {
      if (id && first_global <= rels[0].r_sym)
        Fatal(ctx) << isec
                   << ": FDE with non-local relocations is not supported";

      Symbol<E> &sym = *this->symbols[rels[0].r_sym];
      eh_rels.push_back(EhReloc<E>{sym, rels[0].r_type,
                                   (u32)(rels[0].r_offset - begin_offset),
                                   rels[0].r_addend});
      rels = rels.subspan(1);
    }

    if (id == 0) {
      // CIE
      cur_cie = cies.size();
      offset_to_cie[begin_offset] = cies.size();
      cies.push_back(CieRecord<E>{contents, std::move(eh_rels)});
    } else {
      // FDE
      i64 cie_offset = begin_offset + 4 - id;
      if (cie_offset != cur_cie_offset) {
        auto it = offset_to_cie.find(cie_offset);
        if (it == offset_to_cie.end())
          Fatal(ctx) << isec << ": bad FDE pointer";
        cur_cie = it->second;
        cur_cie_offset = cie_offset;
      }

      if (eh_rels.empty())
        Fatal(ctx) << isec << ": FDE has no relocations";
      if (eh_rels[0].offset != 8)
        Fatal(ctx) << isec << ": FDE's first relocation should have offset 8";

      FdeRecord fde(contents, std::move(eh_rels), cur_cie);
      cies[cur_cie].fdes.push_back(std::move(fde));
    }
  }

  for (CieRecord<E> &cie : cies) {
    std::span<FdeRecord<E>> fdes = cie.fdes;
    while (!fdes.empty()) {
      InputSection<E> *isec = fdes[0].rels[0].sym.input_section;
      i64 i = 1;
      while (i < fdes.size() && isec == fdes[i].rels[0].sym.input_section)
        i++;
      isec->fdes = fdes.subspan(0, i);
      fdes = fdes.subspan(i);
    }
  }
}

template <typename E>
static bool should_write_to_local_symtab(Context<E> &ctx, Symbol<E> &sym) {
  if (ctx.arg.discard_all || ctx.arg.strip_all)
    return false;
  if (sym.get_type() == STT_SECTION)
    return false;

  // Local symbols are discarded if --discard-local is given or they
  // are not in a mergeable section. I *believe* we exclude symbols in
  // mergeable sections because (1) they are too many and (2) they are
  // merged, so their origins shouldn't matter, but I dont' really
  // know the rationale. Anyway, this is the behavior of the
  // traditional linkers.
  if (sym.name.starts_with(".L")) {
    if (ctx.arg.discard_locals)
      return false;

    if (InputSection<E> *isec = sym.input_section)
      if (isec->shdr.sh_flags & SHF_MERGE)
        return false;
  }

  return true;
}

template <typename E>
void ObjectFile<E>::initialize_symbols(Context<E> &ctx) {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter += elf_syms.size();

  // Initialize local symbols
  Symbol<E> *locals = new Symbol<E>[first_global];

  for (i64 i = 1; i < first_global; i++) {
    const ElfSym<E> &esym = elf_syms[i];
    Symbol<E> &sym = locals[i];

    sym.name = symbol_strtab.data() + esym.st_name;

    if (sym.name.empty() && esym.st_type == STT_SECTION)
      if (InputSection<E> *sec =  get_section(esym))
        sym.name = sec->name;

    sym.file = this;
    sym.value = esym.st_value;
    sym.esym = &esym;

    if (!esym.is_abs()) {
      if (esym.is_common())
        Fatal(ctx) << *this << ": common local symbol?";
      sym.input_section = get_section(esym);
    }

    if (should_write_to_local_symtab(ctx, sym)) {
      sym.write_to_symtab = true;
      strtab_size += sym.name.size() + 1;
      num_local_symtab++;
    }
  }

  this->symbols.resize(elf_syms.size());

  i64 num_globals = elf_syms.size() - first_global;
  sym_fragments.resize(num_globals);
  symvers.resize(num_globals);

  for (i64 i = 0; i < first_global; i++)
    this->symbols[i] = &locals[i];

  // Initialize global symbols
  for (i64 i = first_global; i < elf_syms.size(); i++) {
    const ElfSym<E> &esym = elf_syms[i];
    std::string_view key = symbol_strtab.data() + esym.st_name;
    std::string_view name = key;

    if (i64 pos = name.find('@'); pos != name.npos) {
      std::string_view ver = name.substr(pos + 1);
      name = name.substr(0, pos);
      if (ver.starts_with('@'))
        key = name;
      if (esym.is_defined())
        symvers[i - first_global] = ver.data();
    }

    this->symbols[i] = Symbol<E>::intern(ctx, key, name);

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
    MergedSection<E>::get_instance(sec.name, sec.shdr.sh_type,
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
    if (InputSection<E> *isec = sections[i]) {
      if (isec->shdr.sh_flags & SHF_MERGE) {
        mergeable_sections[i] = split_section(ctx, *isec);
        sections[i] = nullptr;
      }
    }
  }

  // Initialize rel_fragments
  for (InputSection<E> *isec : sections) {
    if (!isec || isec->rels.empty())
      continue;

    for (i64 i = 0; i < isec->rels.size(); i++) {
      const ElfRela<E> &rel = isec->rels[i];
      const ElfSym<E> &esym = elf_syms[rel.r_sym];
      if (esym.st_type != STT_SECTION)
        continue;

      MergeableSection<E> &m = mergeable_sections[get_shndx(esym)];
      if (m.fragments.empty())
        continue;

      i64 offset = esym.st_value + rel.r_addend;
      std::span<u32> offsets = m.frag_offsets;

      auto it = std::upper_bound(offsets.begin(), offsets.end(), offset);
      if (it == offsets.begin())
        Fatal(ctx) << *this << ": bad relocation at " << rel.r_sym;
      i64 idx = it - 1 - offsets.begin();

      SectionFragmentRef<E> ref{m.fragments[idx], (i32)(offset - offsets[idx])};
      isec->rel_fragments.push_back(ref);
      isec->has_fragments[i] = true;
    }
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

    if (i < first_global) {
      this->symbols[i]->frag = m.fragments[idx];
      this->symbols[i]->value = esym.st_value - offsets[idx];
    } else {
      sym_fragments[i - first_global].frag = m.fragments[idx];
      sym_fragments[i - first_global].addend = esym.st_value - offsets[idx];
    }
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
  return get_rank(sym.file, *sym.esym, sym.input_section);
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
    SectionFragmentRef<E> &ref = sym_fragments[symidx - first_global];

    sym.file = this;
    sym.input_section = isec;
    if (ref.frag) {
      sym.frag = ref.frag;
      sym.value = ref.addend;
    } else {
      sym.value = esym.st_value;
    }
    sym.ver_idx = ctx.arg.default_version;
    sym.esym = &esym;
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
    bool is_new = !sym.file;
    bool tie_but_higher_priority =
      sym.is_lazy && this->priority < sym.file->priority;

    if (is_new || tie_but_higher_priority) {
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
        sym.ver_idx = ctx.arg.default_version;
        sym.esym = &esym;
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

    static Counter counter("removed_comdat_mem");
    counter += entries.size();
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
    if (!sym.esym || sym.is_undef()) {
      if (sym.file && sym.file->priority < this->priority)
        continue;
      sym.file = this;
      sym.value = 0;
      sym.esym = &esym;
      sym.is_imported = true;
      sym.is_exported = false;
    }
  }
}

template <typename E>
void ObjectFile<E>::scan_relocations(Context<E> &ctx) {
  // Scan relocations against seciton contents
  for (InputSection<E> *isec : sections)
    if (isec)
      isec->scan_relocations(ctx);

  // Scan relocations against exception frames
  for (CieRecord<E> &cie : cies) {
    for (EhReloc<E> &rel : cie.rels) {
      if (rel.sym.is_imported) {
        if (rel.sym.get_type() != STT_FUNC)
          Fatal(ctx) << *this << ": " << rel.sym.name
                  << ": .eh_frame CIE record with an external data reference"
                  << " is not supported";
        rel.sym.flags |= NEEDS_PLT;
      }
    }
  }
}

template <typename E>
void ObjectFile<E>::convert_common_symbols(Context<E> &ctx) {
  if (!has_common_symbol)
    return;

  static OutputSection<E> *osec =
    OutputSection<E>::get_instance(".common", SHT_NOBITS, SHF_WRITE | SHF_ALLOC);

  for (i64 i = first_global; i < elf_syms.size(); i++) {
    if (!elf_syms[i].is_common())
      continue;

    Symbol<E> *sym = this->symbols[i];
    if (sym->file != this) {
      if (ctx.arg.warn_common)
        Warn(ctx) << *this << ": " << "multiple common symbols: " << *sym;
      continue;
    }

    assert(sym->esym->st_value);

    auto *shdr = new ElfShdr<E>;
    memset(shdr, 0, sizeof(*shdr));
    shdr->sh_flags = SHF_ALLOC;
    shdr->sh_type = SHT_NOBITS;
    shdr->sh_size = elf_syms[i].st_size;
    shdr->sh_addralign = sym->esym->st_value;

    InputSection<E> *isec =
      new InputSection(ctx, *this, *shdr, ".common", sections.size());
    isec->output_section = osec;
    sections.push_back(isec);

    sym->input_section = isec;
    sym->value = 0;
  }
}

template <typename E>
static bool should_write_to_global_symtab(Symbol<E> &sym) {
  return sym.get_type() != STT_SECTION && sym.is_alive();
}

template <typename E>
void ObjectFile<E>::compute_symtab(Context<E> &ctx) {
  if (ctx.arg.strip_all)
    return;

  if (ctx.arg.gc_sections && !ctx.arg.discard_all) {
    // Detect symbols pointing to sections discarded by -gc-sections
    // to not copy them to symtab.
    for (i64 i = 1; i < first_global; i++) {
      Symbol<E> &sym = *this->symbols[i];

      if (sym.write_to_symtab && !sym.is_alive()) {
        strtab_size -= sym.name.size() + 1;
        num_local_symtab--;
        sym.write_to_symtab = false;
      }
    }
  }

  // Compute the size of global symbols.
  for (i64 i = first_global; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];

    if (sym.file == this && should_write_to_global_symtab(sym)) {
      strtab_size += sym.name.size() + 1;
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

  auto write_sym = [&](i64 i) {
    Symbol<E> &sym = *this->symbols[i];
    ElfSym<E> &esym = *(ElfSym<E> *)(symtab_base + symtab_off);
    symtab_off += sizeof(esym);

    esym = elf_syms[i];
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

    write_string(strtab_base + strtab_off, sym.name);
    strtab_off += sym.name.size() + 1;
  };

  symtab_off = local_symtab_offset;
  for (i64 i = 1; i < first_global; i++)
    if (this->symbols[i]->write_to_symtab)
      write_sym(i);

  symtab_off = global_symtab_offset;
  for (i64 i = first_global; i < elf_syms.size(); i++)
    if (this->symbols[i]->file == this && this->symbols[i]->write_to_symtab)
      write_sym(i);
}

bool is_c_identifier(std::string_view name) {
  static std::regex re("[a-zA-Z_][a-zA-Z0-9_]*");
  return std::regex_match(name.begin(), name.end(), re);
}

template <typename E>
ObjectFile<E>::ObjectFile(Context<E> &ctx) {
  // Create linker-synthesized symbols.
  auto *esyms = new std::vector<ElfSym<E>>(1);
  this->symbols.push_back(new Symbol<E>);
  this->first_global = 1;
  this->is_alive = true;
  this->priority = 1;

  auto add = [&](std::string_view name, u8 visibility = STV_DEFAULT) {
    ElfSym<E> esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = visibility;
    esyms->push_back(esym);

    Symbol<E> *sym = Symbol<E>::intern(ctx, name);
    this->symbols.push_back(sym);
    return sym;
  };

  ctx.__ehdr_start = add("__ehdr_start", STV_HIDDEN);
  ctx.__rela_iplt_start = add("__rela_iplt_start", STV_HIDDEN);
  ctx.__rela_iplt_end = add("__rela_iplt_end", STV_HIDDEN);
  ctx.__init_array_start = add("__init_array_start", STV_HIDDEN);
  ctx.__init_array_end = add("__init_array_end", STV_HIDDEN);
  ctx.__fini_array_start = add("__fini_array_start", STV_HIDDEN);
  ctx.__fini_array_end = add("__fini_array_end", STV_HIDDEN);
  ctx.__preinit_array_start = add("__preinit_array_start", STV_HIDDEN);
  ctx.__preinit_array_end = add("__preinit_array_end", STV_HIDDEN);
  ctx._DYNAMIC = add("_DYNAMIC", STV_HIDDEN);
  ctx._GLOBAL_OFFSET_TABLE_ = add("_GLOBAL_OFFSET_TABLE_", STV_HIDDEN);
  ctx.__bss_start = add("__bss_start", STV_HIDDEN);
  ctx._end = add("_end", STV_HIDDEN);
  ctx._etext = add("_etext", STV_HIDDEN);
  ctx._edata = add("_edata", STV_HIDDEN);
  ctx.__executable_start = add("__executable_start", STV_HIDDEN);

  if (ctx.arg.eh_frame_hdr)
    ctx.__GNU_EH_FRAME_HDR = add("__GNU_EH_FRAME_HDR", STV_HIDDEN);

  for (OutputChunk<E> *chunk : ctx.chunks) {
    if (!is_c_identifier(chunk->name))
      continue;

    auto *start = new std::string("__start_" + std::string(chunk->name));
    auto *stop = new std::string("__stop_" + std::string(chunk->name));
    add(*start, STV_HIDDEN);
    add(*stop, STV_HIDDEN);
  }

  elf_syms = *esyms;

  i64 num_globals = elf_syms.size() - first_global;
  sym_fragments.resize(num_globals);
  symvers.resize(num_globals);
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
  return this->name;
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
        *new std::string(std::string(name) + "@" + verstr);

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
void SharedFile<E>::resolve_symbols(Context<E> &ctx) {
  for (i64 i = 0; i < this->symbols.size(); i++) {
    Symbol<E> &sym = *this->symbols[i];
    const ElfSym<E> &esym = *elf_syms[i];

    std::lock_guard lock(sym.mu);

    bool is_new = !sym.file;
    bool tie_but_higher_priority =
      !is_new && sym.file->is_dso && this->priority < sym.file->priority;

    if (is_new || tie_but_higher_priority) {
      sym.file = this;
      sym.input_section = nullptr;
      sym.frag = nullptr;
      sym.value = esym.st_value;
      sym.ver_idx = versyms[i];
      sym.esym = &esym;
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
        sym->esym->st_value == sym2->esym->st_value)
      vec.push_back(sym2);
  return vec;
}

template <typename E>
bool SharedFile<E>::is_readonly(Context<E> &ctx, Symbol<E> *sym) {
  ElfEhdr<E> *ehdr = (ElfEhdr<E> *)this->mb->data(ctx);
  ElfPhdr<E> *phdr = (ElfPhdr<E> *)(this->mb->data(ctx) + ehdr->e_phoff);
  u64 val = sym->esym->st_value;

  for (i64 i = 0; i < ehdr->e_phnum; i++)
    if (phdr[i].p_type == PT_LOAD && !(phdr[i].p_flags & PF_W) &&
        phdr[i].p_vaddr <= val && val < phdr[i].p_vaddr + phdr[i].p_memsz)
      return true;
  return false;
}

template class MemoryMappedFile<ELF64LE>;
template class ObjectFile<ELF64LE>;
template class SharedFile<ELF64LE>;

template
std::ostream &operator<<(std::ostream &out, const InputFile<ELF64LE> &file);
