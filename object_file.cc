#include "mold.h"

#include <cstring>
#include <fcntl.h>
#include <regex>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

MemoryMappedFile *MemoryMappedFile::open(std::string path) {
  struct stat st;
  if (stat(path.c_str(), &st) == -1)
    return nullptr;
  u64 mtime = (u64)st.st_mtim.tv_sec * 1000000000 + st.st_mtim.tv_nsec;
  return new MemoryMappedFile(path, nullptr, st.st_size, mtime);
}

MemoryMappedFile *MemoryMappedFile::must_open(std::string path) {
  if (MemoryMappedFile *mb = MemoryMappedFile::open(path))
    return mb;
  Fatal() << "cannot open " << path;
}

u8 *MemoryMappedFile::data() {
  if (data_)
    return data_;

  std::lock_guard lock(mu);
  if (data_)
    return data_;

  i64 fd = ::open(name.c_str(), O_RDONLY);
  if (fd == -1)
    Fatal() << name << ": cannot open: " << strerror(errno);

  data_ = (u8 *)mmap(nullptr, size_, PROT_READ, MAP_PRIVATE, fd, 0);
  if (data_ == MAP_FAILED)
    Fatal() << name << ": mmap failed: " << strerror(errno);
  close(fd);
  return data_;
}

MemoryMappedFile *MemoryMappedFile::slice(std::string name, u64 start,
                                          u64 size) {
  MemoryMappedFile *mb = new MemoryMappedFile(name, data_ + start, size);
  mb->parent = this;
  return mb;
}

InputFile::InputFile(MemoryMappedFile *mb) : mb(mb), name(mb->name) {
  if (mb->size() < sizeof(ElfEhdr))
    Fatal() << *this << ": file too small";
  if (memcmp(mb->data(), "\177ELF", 4))
    Fatal() << *this << ": not an ELF file";

  ElfEhdr &ehdr = *(ElfEhdr *)mb->data();
  is_dso = (ehdr.e_type == ET_DYN);

  ElfShdr *sh_begin = (ElfShdr *)(mb->data() + ehdr.e_shoff);

  // e_shnum contains the total number of sections in an object file.
  // Since it is a 16-bit integer field, it's not large enough to
  // represent >65535 sections. If an object file contains more than 65535
  // sections, the actual number is stored to sh_size field.
  i64 num_sections = (ehdr.e_shnum == 0) ? sh_begin->sh_size : ehdr.e_shnum;

  if (mb->data() + mb->size() < (u8 *)(sh_begin + num_sections))
    Fatal() << *this << ": e_shoff or e_shnum corrupted: "
            << mb->size() << " " << num_sections;
  elf_sections = {sh_begin, sh_begin + num_sections};

  // e_shstrndx is a 16-bit field. If .shstrtab's section index is
  // too large, the actual number is stored to sh_link field.
  i64 shstrtab_idx = (ehdr.e_shstrndx == SHN_XINDEX)
    ? sh_begin->sh_link : ehdr.e_shstrndx;

  shstrtab = get_string(shstrtab_idx);
}

std::string_view InputFile::get_string(const ElfShdr &shdr) {
  u8 *begin = mb->data() + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb->data() + mb->size() < end)
    Fatal() << *this << ": shdr corrupted";
  return {(char *)begin, (char *)end};
}

std::string_view InputFile::get_string(i64 idx) {
  assert(idx < elf_sections.size());

  if (elf_sections.size() <= idx)
    Fatal() << *this << ": invalid section index: " << idx;
  return get_string(elf_sections[idx]);
}

template<typename T>
std::span<T> InputFile::get_data(const ElfShdr &shdr) {
  std::string_view view = get_string(shdr);
  if (view.size() % sizeof(T))
    Fatal() << *this << ": corrupted section";
  return {(T *)view.data(), view.size() / sizeof(T)};
}

template<typename T>
std::span<T> InputFile::get_data(i64 idx) {
  if (elf_sections.size() <= idx)
    Fatal() << *this << ": invalid section index";
  return get_data<T>(elf_sections[idx]);
}

ElfShdr *InputFile::find_section(i64 type) {
  for (ElfShdr &sec : elf_sections)
    if (sec.sh_type == type)
      return &sec;
  return nullptr;
}

ObjectFile::ObjectFile(Context &ctx, MemoryMappedFile *mb,
                       std::string archive_name, bool is_in_lib)
  : InputFile(mb), archive_name(archive_name), is_in_lib(is_in_lib) {
  is_alive = !is_in_lib;
}

static bool is_debug_section(const ElfShdr &shdr, std::string_view name) {
  return !(shdr.sh_flags & SHF_ALLOC) &&
         (name.starts_with(".debug") || name.starts_with(".zdebug"));
}

void ObjectFile::initialize_sections(Context &ctx) {
  // Read sections
  for (i64 i = 0; i < elf_sections.size(); i++) {
    const ElfShdr &shdr = elf_sections[i];

    if ((shdr.sh_flags & SHF_EXCLUDE) && !(shdr.sh_flags & SHF_ALLOC))
      continue;

    switch (shdr.sh_type) {
    case SHT_GROUP: {
      // Get the signature of this section group.
      if (shdr.sh_info >= elf_syms.size())
        Fatal() << *this << ": invalid symbol index";
      const ElfSym &sym = elf_syms[shdr.sh_info];
      std::string_view signature = symbol_strtab.data() + sym.st_name;

      // Get comdat group members.
      std::span<u32> entries = get_data<u32>(shdr);

      if (entries.empty())
        Fatal() << *this << ": empty SHT_GROUP";
      if (entries[0] == 0)
        continue;
      if (entries[0] != GRP_COMDAT)
        Fatal() << *this << ": unsupported SHT_GROUP format";

      static ConcurrentMap<ComdatGroup> map;
      ComdatGroup *group = map.insert(signature, ComdatGroup());
      comdat_groups.push_back({group, entries.subspan(1)});

      static Counter counter("comdats");
      counter++;
      break;
    }
    case SHT_SYMTAB_SHNDX:
      symtab_shndx_sec = get_data<u32>(shdr);
      break;
    case SHT_SYMTAB:
    case SHT_STRTAB:
    case SHT_REL:
    case SHT_RELA:
    case SHT_NULL:
      break;
    default: {
      std::string_view name = shstrtab.data() + shdr.sh_name;
      if (name == ".note.GNU-stack" || name == ".note.gnu.property")
        continue;

      if ((ctx.arg.strip_all || ctx.arg.strip_debug) &&
          is_debug_section(shdr, name))
        continue;

      this->sections[i] = new InputSection(*this, shdr, name, i);

      static Counter counter("regular_sections");
      counter++;
      break;
    }
    }
  }

  // Attach relocation sections to their target sections.
  for (const ElfShdr &shdr : elf_sections) {
    if (shdr.sh_type != SHT_RELA)
      continue;

    if (shdr.sh_info >= sections.size())
      Fatal() << *this << ": invalid relocated section index: "
              << (u32)shdr.sh_info;

    if (InputSection *target = sections[shdr.sh_info]) {
      target->rels = get_data<ElfRela>(shdr);
      target->has_fragments.resize(target->rels.size());
      if (target->shdr.sh_flags & SHF_ALLOC)
        target->rel_types.resize(target->rels.size());
    }
  }
}

void ObjectFile::initialize_ehframe_sections() {
  for (i64 i = 0; i < sections.size(); i++) {
    InputSection *isec = sections[i];
    if (isec && isec->name == ".eh_frame") {
      read_ehframe(*isec);
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
void ObjectFile::read_ehframe(InputSection &isec) {
  std::span<ElfRela> rels = isec.rels;
  std::string_view data = get_string(isec.shdr);
  const char *begin = data.data();

  if (data.empty()) {
    cies.push_back(CieRecord{data});
    return;
  }

  std::unordered_map<i64, i64> offset_to_cie;
  i64 cur_cie = -1;
  i64 cur_cie_offset = -1;

  for (ElfRela rel : rels)
    if (rel.r_type != R_X86_64_32 && rel.r_type != R_X86_64_64 &&
        rel.r_type != R_X86_64_PC32 && rel.r_type != R_X86_64_PC64)
      Fatal() << isec << ": unsupported relocation type: " << rel.r_type;

  while (!data.empty()) {
    i64 size = *(u32 *)data.data();
    if (size == 0) {
      if (data.size() != 4)
        Fatal() << isec << ": garbage at end of section";
      cies.push_back(CieRecord{data});
      return;
    }

    i64 begin_offset = data.data() - begin;
    i64 end_offset = begin_offset + size + 4;

    if (!rels.empty() && rels[0].r_offset < begin_offset)
      Fatal() << isec << ": unsupported relocation order";

    std::string_view contents = data.substr(0, size + 4);
    data = data.substr(size + 4);
    i64 id = *(u32 *)(contents.data() + 4);

    std::vector<EhReloc> eh_rels;
    while (!rels.empty() && rels[0].r_offset < end_offset) {
      if (id && first_global <= rels[0].r_sym)
        Fatal() << isec << ": FDE with non-local relocations is not supported";

      Symbol &sym = *symbols[rels[0].r_sym];
      eh_rels.push_back(EhReloc{sym, rels[0].r_type,
                                (u32)(rels[0].r_offset - begin_offset),
                                rels[0].r_addend});
      rels = rels.subspan(1);
    }

    if (id == 0) {
      // CIE
      cur_cie = cies.size();
      offset_to_cie[begin_offset] = cies.size();
      cies.push_back(CieRecord{contents, std::move(eh_rels)});
    } else {
      // FDE
      i64 cie_offset = begin_offset + 4 - id;
      if (cie_offset != cur_cie_offset) {
        auto it = offset_to_cie.find(cie_offset);
        if (it == offset_to_cie.end())
          Fatal() << isec << ": bad FDE pointer";
        cur_cie = it->second;
        cur_cie_offset = cie_offset;
      }

      if (eh_rels.empty())
        Fatal() << isec << ": FDE has no relocations";
      if (eh_rels[0].offset != 8)
        Fatal() << isec << ": FDE's first relocation should have offset 8";

      FdeRecord fde(contents, std::move(eh_rels), cur_cie);
      cies[cur_cie].fdes.push_back(std::move(fde));
    }
  }

  for (CieRecord &cie : cies) {
    std::span<FdeRecord> fdes = cie.fdes;
    while (!fdes.empty()) {
      InputSection *isec = fdes[0].rels[0].sym.input_section;
      i64 i = 1;
      while (i < fdes.size() && isec == fdes[i].rels[0].sym.input_section)
        i++;
      isec->fdes = fdes.subspan(0, i);
      fdes = fdes.subspan(i);
    }
  }
}

static bool should_write_to_local_symtab(Context &ctx, Symbol &sym) {
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

    if (InputSection *isec = sym.input_section)
      if (isec->shdr.sh_flags & SHF_MERGE)
        return false;
  }

  return true;
}

void ObjectFile::initialize_symbols(Context &ctx) {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter += elf_syms.size();

  // Initialize local symbols
  Symbol *locals = new Symbol[first_global];

  for (i64 i = 1; i < first_global; i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = locals[i];

    sym.name = symbol_strtab.data() + esym.st_name;

    if (sym.name.empty() && esym.st_type == STT_SECTION)
      if (InputSection *sec =  get_section(esym))
        sym.name = sec->name;

    sym.file = this;
    sym.value = esym.st_value;
    sym.esym = &esym;

    if (!esym.is_abs()) {
      if (esym.is_common())
        Fatal() << *this << ": common local symbol?";
      sym.input_section = get_section(esym);
    }

    if (should_write_to_local_symtab(ctx, sym)) {
      sym.write_to_symtab = true;
      strtab_size += sym.name.size() + 1;
      num_local_symtab++;
    }
  }

  symbols.resize(elf_syms.size());

  i64 num_globals = elf_syms.size() - first_global;
  sym_fragments.resize(num_globals);
  symvers.resize(num_globals);

  for (i64 i = 0; i < first_global; i++)
    symbols[i] = &locals[i];

  // Initialize global symbols
  for (i64 i = first_global; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
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

    symbols[i] = Symbol::intern(key, name);

    if (esym.is_common())
      has_common_symbol = true;
  }
}

struct MergeableSection {
  std::vector<SectionFragment *> fragments;
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
static MergeableSection split_section(InputSection &sec) {
  MergeableSection rec;

  MergedSection *parent =
    MergedSection::get_instance(sec.name, sec.shdr.sh_type,
                                sec.shdr.sh_flags);

  std::string_view data = sec.contents;
  const char *begin = data.data();
  u64 entsize = sec.shdr.sh_entsize;

  static_assert(sizeof(SectionFragment::alignment) == 2);
  if (sec.shdr.sh_addralign >= UINT16_MAX)
    Fatal() << sec << ": alignment too large";

  if (sec.shdr.sh_flags & SHF_STRINGS) {
    while (!data.empty()) {
      size_t end = find_null(data, entsize);
      if (end == data.npos)
        Fatal() << sec << ": string is not null terminated";

      std::string_view substr = data.substr(0, end + entsize);
      data = data.substr(end + entsize);

      SectionFragment *frag = parent->insert(substr, sec.shdr.sh_addralign);
      rec.fragments.push_back(frag);
      rec.frag_offsets.push_back(substr.data() - begin);
    }
  } else {
    if (data.size() % entsize)
      Fatal() << sec << ": section size is not multiple of sh_entsize";

    while (!data.empty()) {
      std::string_view substr = data.substr(0, entsize);
      data = data.substr(entsize);

      SectionFragment *frag = parent->insert(substr, sec.shdr.sh_addralign);
      rec.fragments.push_back(frag);
      rec.frag_offsets.push_back(substr.data() - begin);
    }
  }

  static Counter counter("string_fragments");
  counter += rec.fragments.size();

  return rec;
}

void ObjectFile::initialize_mergeable_sections() {
  std::vector<MergeableSection> mergeable_sections(sections.size());

  for (i64 i = 0; i < sections.size(); i++) {
    if (InputSection *isec = sections[i]) {
      if (isec->shdr.sh_flags & SHF_MERGE) {
        mergeable_sections[i] = split_section(*isec);
        sections[i] = nullptr;
      }
    }
  }

  // Initialize rel_fragments
  for (InputSection *isec : sections) {
    if (!isec || isec->rels.empty())
      continue;

    for (i64 i = 0; i < isec->rels.size(); i++) {
      const ElfRela &rel = isec->rels[i];
      const ElfSym &esym = elf_syms[rel.r_sym];
      if (esym.st_type != STT_SECTION)
        continue;

      MergeableSection &m = mergeable_sections[get_shndx(esym)];
      if (m.fragments.empty())
        continue;

      i64 offset = esym.st_value + rel.r_addend;
      std::span<u32> offsets = m.frag_offsets;

      auto it = std::upper_bound(offsets.begin(), offsets.end(), offset);
      if (it == offsets.begin())
        Fatal() << *this << ": bad relocation at " << rel.r_sym;
      i64 idx = it - 1 - offsets.begin();

      SectionFragmentRef ref{m.fragments[idx], (i32)(offset - offsets[idx])};
      isec->rel_fragments.push_back(ref);
      isec->has_fragments[i] = true;
    }
  }

  // Initialize sym_fragments
  for (i64 i = 0; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    if (esym.is_abs() || esym.is_common())
      continue;

    MergeableSection &m = mergeable_sections[get_shndx(esym)];
    if (m.fragments.empty())
      continue;

    std::span<u32> offsets = m.frag_offsets;

    auto it = std::upper_bound(offsets.begin(), offsets.end(), esym.st_value);
    if (it == offsets.begin())
      Fatal() << *this << ": bad symbol value: " << esym.st_value;
    i64 idx = it - 1 - offsets.begin();

    if (i < first_global) {
      symbols[i]->frag = m.fragments[idx];
      symbols[i]->value = esym.st_value - offsets[idx];
    } else {
      sym_fragments[i - first_global].frag = m.fragments[idx];
      sym_fragments[i - first_global].addend = esym.st_value - offsets[idx];
    }
  }

  for (MergeableSection &m : mergeable_sections)
    fragments.insert(fragments.end(), m.fragments.begin(), m.fragments.end());
}

void ObjectFile::parse(Context &ctx) {
  sections.resize(elf_sections.size());
  symtab_sec = find_section(SHT_SYMTAB);

  if (symtab_sec) {
    first_global = symtab_sec->sh_info;
    elf_syms = get_data<ElfSym>(*symtab_sec);
    symbol_strtab = get_string(symtab_sec->sh_link);
  }

  initialize_sections(ctx);
  initialize_symbols(ctx);
  initialize_mergeable_sections();
  initialize_ehframe_sections();
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
static u64 get_rank(InputFile *file, const ElfSym &esym, InputSection *isec) {
  if (esym.st_bind == STB_WEAK)
    return (3 << 24) + file->priority;
  if (esym.is_common())
    return (2 << 24) + file->priority;
  return (1 << 24) + file->priority;
}

static u64 get_rank(const Symbol &sym) {
  if (!sym.file)
    return 5 << 24;
  if (sym.is_lazy)
    return (4 << 24) + sym.file->priority;
  return get_rank(sym.file, *sym.esym, sym.input_section);
}

void ObjectFile::maybe_override_symbol(Context &ctx, Symbol &sym, i64 symidx) {
  InputSection *isec = nullptr;
  const ElfSym &esym = elf_syms[symidx];
  if (!esym.is_abs() && !esym.is_common())
    isec = get_section(esym);

  u64 new_rank = get_rank(this, esym, isec);

  std::lock_guard lock(sym.mu);
  u64 existing_rank = get_rank(sym);

  if (new_rank < existing_rank) {
    SectionFragmentRef &ref = sym_fragments[symidx - first_global];

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
      SyncOut() << "trace-symbol: " << *sym.file
                << (is_weak ? ": weak definition of " : ": definition of ")
                << sym;
    }
  }
}

void ObjectFile::merge_visibility(Symbol &sym, u8 visibility) {
  auto priority = [&](u8 visibility) {
    switch (visibility) {
    case STV_HIDDEN:
      return 1;
    case STV_PROTECTED:
      return 2;
    case STV_DEFAULT:
      return 3;
    }
    Fatal() << *this << ": unknown symbol visibility: " << sym;
  };

  u8 val = sym.visibility;

  while (priority(visibility) < priority(val))
    if (sym.visibility.compare_exchange_strong(val, visibility))
      break;
}

void ObjectFile::resolve_lazy_symbols(Context &ctx) {
  assert(is_in_lib);

  for (i64 i = first_global; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];
    const ElfSym &esym = elf_syms[i];

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
        SyncOut() << "trace-symbol: " << *sym.file
                  << ": lazy definition of " << sym;
    }
  }
}

void ObjectFile::resolve_regular_symbols(Context &ctx) {
  assert(!is_in_lib);

  for (i64 i = first_global; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];
    const ElfSym &esym = elf_syms[i];
    merge_visibility(sym, exclude_libs ? STV_HIDDEN : esym.st_visibility);

    if (esym.is_defined())
      maybe_override_symbol(ctx, sym, i);
  }
}

void ObjectFile::mark_live_objects(Context &ctx,
                                   std::function<void(ObjectFile *)> feeder) {
  assert(is_alive);

  for (i64 i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];
    merge_visibility(sym, exclude_libs ? STV_HIDDEN : esym.st_visibility);

    if (esym.is_defined()) {
      if (is_in_lib)
        maybe_override_symbol(ctx, sym, i);
      continue;
    }

    bool is_weak = (esym.st_bind == STB_WEAK);

    if (sym.traced) {
      SyncOut() << "trace-symbol: " << *this
                << (is_weak ? ": weak reference to " : ": reference to ")
                << sym;
    }

    if (!is_weak && sym.file && !sym.file->is_alive.exchange(true)) {
      feeder((ObjectFile *)sym.file);
      if (sym.traced)
        SyncOut() << "trace-symbol: " << *this << " keeps " << *sym.file
                  << " for " << sym;
    }
  }
}

void ObjectFile::convert_undefined_weak_symbols(Context &ctx) {
  for (i64 i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];

    if (esym.is_undef() && esym.st_bind == STB_WEAK) {
      Symbol &sym = *symbols[i];
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
          SyncOut() << "trace-symbol: " << *this
                    << ": unresolved weak symbol " << sym;
      }
    }
  }
}

void ObjectFile::resolve_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    u32 cur = group->owner;
    while (cur == -1 || cur > this->priority)
      if (group->owner.compare_exchange_weak(cur, this->priority))
        break;
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
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

void ObjectFile::claim_unresolved_symbols() {
  if (!is_alive)
    return;

  for (i64 i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

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

void ObjectFile::scan_relocations(Context &ctx) {
  // Scan relocations against seciton contents
  for (InputSection *isec : sections)
    if (isec)
      isec->scan_relocations(ctx);

  // Scan relocations against exception frames
  for (CieRecord &cie : cies) {
    for (EhReloc &rel : cie.rels) {
      if (rel.sym.is_imported) {
        if (rel.sym.get_type() != STT_FUNC)
          Fatal() << *this << ": " << rel.sym.name
                  << ": .eh_frame CIE record with an external data reference"
                  << " is not supported";
        rel.sym.flags |= NEEDS_PLT;
      }
    }
  }
}

void ObjectFile::convert_common_symbols(Context &ctx) {
  if (!has_common_symbol)
    return;

  static OutputSection *osec =
    OutputSection::get_instance(".common", SHT_NOBITS, SHF_WRITE | SHF_ALLOC);

  for (i64 i = first_global; i < elf_syms.size(); i++) {
    if (!elf_syms[i].is_common())
      continue;

    Symbol *sym = symbols[i];
    if (sym->file != this) {
      if (ctx.arg.warn_common)
        Warn() << *this << ": " << "multiple common symbols: " << *sym;
      continue;
    }

    assert(sym->esym->st_value);

    auto *shdr = new ElfShdr;
    memset(shdr, 0, sizeof(*shdr));
    shdr->sh_flags = SHF_ALLOC;
    shdr->sh_type = SHT_NOBITS;
    shdr->sh_size = elf_syms[i].st_size;
    shdr->sh_addralign = sym->esym->st_value;

    InputSection *isec =
      new InputSection(*this, *shdr, ".common", sections.size());
    isec->output_section = osec;
    sections.push_back(isec);

    sym->input_section = isec;
    sym->value = 0;
  }
}

static bool should_write_to_global_symtab(Symbol &sym) {
  return sym.get_type() != STT_SECTION && sym.is_alive();
}

void ObjectFile::compute_symtab(Context &ctx) {
  if (ctx.arg.strip_all)
    return;

  if (ctx.arg.gc_sections && !ctx.arg.discard_all) {
    // Detect symbols pointing to sections discarded by -gc-sections
    // to not copy them to symtab.
    for (i64 i = 1; i < first_global; i++) {
      Symbol &sym = *symbols[i];

      if (sym.write_to_symtab && !sym.is_alive()) {
        strtab_size -= sym.name.size() + 1;
        num_local_symtab--;
        sym.write_to_symtab = false;
      }
    }
  }

  // Compute the size of global symbols.
  for (i64 i = first_global; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];

    if (sym.file == this && should_write_to_global_symtab(sym)) {
      strtab_size += sym.name.size() + 1;
      sym.write_to_symtab = true;
      num_global_symtab++;
    }
  }
}

void ObjectFile::write_symtab(Context &ctx) {
  u8 *symtab_base = ctx.buf + ctx.symtab->shdr.sh_offset;
  u8 *strtab_base = ctx.buf + ctx.strtab->shdr.sh_offset;
  i64 strtab_off = strtab_offset;
  i64 symtab_off;

  auto write_sym = [&](i64 i) {
    Symbol &sym = *symbols[i];
    ElfSym &esym = *(ElfSym *)(symtab_base + symtab_off);
    symtab_off += sizeof(ElfSym);

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
    if (symbols[i]->write_to_symtab)
      write_sym(i);

  symtab_off = global_symtab_offset;
  for (i64 i = first_global; i < elf_syms.size(); i++)
    if (symbols[i]->file == this && symbols[i]->write_to_symtab)
      write_sym(i);
}

bool is_c_identifier(std::string_view name) {
  static std::regex re("[a-zA-Z_][a-zA-Z0-9_]*");
  return std::regex_match(name.begin(), name.end(), re);
}

ObjectFile::ObjectFile(Context &ctx) {
  // Create linker-synthesized symbols.
  auto *esyms = new std::vector<ElfSym>(1);
  symbols.push_back(new Symbol);
  first_global = 1;
  is_alive = true;
  priority = 1;

  auto add = [&](std::string_view name, u8 visibility = STV_DEFAULT) {
    ElfSym esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = visibility;
    esyms->push_back(esym);

    Symbol *sym = Symbol::intern(name);
    symbols.push_back(sym);
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

  for (OutputChunk *chunk : ctx.chunks) {
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

std::ostream &operator<<(std::ostream &out, const InputFile &file) {
  if (file.is_dso) {
    out << path_clean(file.name);
    return out;
  }

  ObjectFile *obj = (ObjectFile *)&file;
  if (obj->archive_name == "")
    out << path_clean(obj->name);
  else
    out << path_clean(obj->archive_name) << "(" << obj->name + ")";
  return out;
}

SharedFile::SharedFile(Context &ctx, MemoryMappedFile *mb) : InputFile(mb) {
  is_alive = !ctx.as_needed;
}

std::string_view SharedFile::get_soname() {
  if (ElfShdr *sec = find_section(SHT_DYNAMIC))
    for (ElfDyn &dyn : get_data<ElfDyn>(*sec))
      if (dyn.d_tag == DT_SONAME)
        return symbol_strtab.data() + dyn.d_val;
  return name;
}

void SharedFile::parse(Context &ctx) {
  symtab_sec = find_section(SHT_DYNSYM);
  if (!symtab_sec)
    return;

  symbol_strtab = get_string(symtab_sec->sh_link);
  soname = get_soname();
  version_strings = read_verdef();

  // Read a symbol table.
  i64 first_global = symtab_sec->sh_info;
  std::span<ElfSym> esyms = get_data<ElfSym>(*symtab_sec);

  std::span<u16> vers;
  if (ElfShdr *sec = find_section(SHT_GNU_VERSYM))
    vers = get_data<u16>(*sec);

  for (i64 i = first_global; i < esyms.size(); i++) {
    std::string_view name = symbol_strtab.data() + esyms[i].st_name;

    if (!esyms[i].is_defined()) {
      undefs.push_back(Symbol::intern(name));
      continue;
    }

    if (vers.empty()) {
      elf_syms.push_back(&esyms[i]);
      versyms.push_back(VER_NDX_GLOBAL);
      symbols.push_back(Symbol::intern(name));
    } else {
      u16 ver = vers[i] & ~VERSYM_HIDDEN;
      if (ver == VER_NDX_LOCAL)
        continue;

      std::string verstr(version_strings[ver]);
      std::string_view mangled =
        *new std::string(std::string(name) + "@" + verstr);

      elf_syms.push_back(&esyms[i]);
      versyms.push_back(ver);
      symbols.push_back(Symbol::intern(mangled, name));

      if (!(vers[i] & VERSYM_HIDDEN)) {
        elf_syms.push_back(&esyms[i]);
        versyms.push_back(ver);
        symbols.push_back(Symbol::intern(name));
      }
    }
  }

  static Counter counter("dso_syms");
  counter += elf_syms.size();
}

std::vector<std::string_view> SharedFile::read_verdef() {
  std::vector<std::string_view> ret(VER_NDX_LAST_RESERVED + 1);

  ElfShdr *verdef_sec = find_section(SHT_GNU_VERDEF);
  if (!verdef_sec)
    return ret;

  std::string_view verdef = get_string(*verdef_sec);
  std::string_view strtab = get_string(verdef_sec->sh_link);

  ElfVerdef *ver = (ElfVerdef *)verdef.data();

  for (;;) {
    if (ret.size() <= ver->vd_ndx)
      ret.resize(ver->vd_ndx + 1);

    ElfVerdaux *aux = (ElfVerdaux *)((u8 *)ver + ver->vd_aux);
    ret[ver->vd_ndx] = strtab.data() + aux->vda_name;
    if (!ver->vd_next)
      break;

    ver = (ElfVerdef *)((u8 *)ver + ver->vd_next);
  }
  return ret;
}

void SharedFile::resolve_symbols() {
  for (i64 i = 0; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];
    const ElfSym &esym = *elf_syms[i];

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
        SyncOut() << "trace-symbol: " << *sym.file << ": definition of "
                  << sym;
    }
  }
}

std::vector<Symbol *> SharedFile::find_aliases(Symbol *sym) {
  assert(sym->file == this);
  std::vector<Symbol *> vec;
  for (Symbol *sym2 : symbols)
    if (sym2->file == this && sym != sym2 &&
        sym->esym->st_value == sym2->esym->st_value)
      vec.push_back(sym2);
  return vec;
}

bool SharedFile::is_readonly(Symbol *sym) {
  ElfEhdr *ehdr = (ElfEhdr *)mb->data();
  ElfPhdr *phdr = (ElfPhdr *)(mb->data() + ehdr->e_phoff);
  u64 val = sym->esym->st_value;

  for (i64 i = 0; i < ehdr->e_phnum; i++)
    if (phdr[i].p_type == PT_LOAD && !(phdr[i].p_flags & PF_W) &&
        phdr[i].p_vaddr <= val && val < phdr[i].p_vaddr + phdr[i].p_memsz)
      return true;
  return false;
}
