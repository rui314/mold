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

  int fd = ::open(name.c_str(), O_RDONLY);
  if (fd == -1)
    Fatal() << name << ": cannot open: " << strerror(errno);

  data_ = (u8 *)mmap(nullptr, size_, PROT_READ, MAP_PRIVATE, fd, 0);
  if (data_ == MAP_FAILED)
    Fatal() << name << ": mmap failed: " << strerror(errno);
  close(fd);
  return data_;
}

MemoryMappedFile *MemoryMappedFile::slice(std::string name, u64 start, u64 size) {
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

  u8 *sh_begin = mb->data() + ehdr.e_shoff;
  u8 *sh_end = sh_begin + ehdr.e_shnum * sizeof(ElfShdr);
  if (mb->data() + mb->size() < sh_end)
    Fatal() << *this << ": e_shoff or e_shnum corrupted: "
            << mb->size() << " " << ehdr.e_shnum;
  elf_sections = {(ElfShdr *)sh_begin, (ElfShdr *)sh_end};
  shstrtab = get_string(ehdr.e_shstrndx);
}

std::string_view InputFile::get_string(const ElfShdr &shdr) {
  u8 *begin = mb->data() + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb->data() + mb->size() < end)
    Fatal() << *this << ": shdr corrupted";
  return {(char *)begin, (char *)end};
}

std::string_view InputFile::get_string(u32 idx) {
  if (elf_sections.size() <= idx)
    Fatal() << *this << ": invalid section index";
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
std::span<T> InputFile::get_data(u32 idx) {
  if (elf_sections.size() <= idx)
    Fatal() << *this << ": invalid section index";
  return get_data<T>(elf_sections[idx]);
}

ElfShdr *InputFile::find_section(u32 type) {
  for (ElfShdr &sec : elf_sections)
    if (sec.sh_type == type)
      return &sec;
  return nullptr;
}

ObjectFile::ObjectFile(MemoryMappedFile *mb, std::string archive_name)
  : InputFile(mb), archive_name(archive_name),
    is_in_archive(archive_name != "") {
  is_alive = (archive_name == "");
}

void ObjectFile::initialize_sections() {
  // Read sections
  for (int i = 0; i < elf_sections.size(); i++) {
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
      ComdatGroup *group = map.insert(signature, ComdatGroup(nullptr, 0));
      comdat_groups.push_back({group, entries});

      static Counter counter("comdats");
      counter.inc();
      break;
    }
    case SHT_SYMTAB_SHNDX:
      Fatal() << *this << ": SHT_SYMTAB_SHNDX section is not supported";
      break;
    case SHT_SYMTAB:
    case SHT_STRTAB:
    case SHT_REL:
    case SHT_RELA:
    case SHT_NULL:
      break;
    default: {
      static Counter counter("regular_sections");
      counter.inc();

      std::string_view name = shstrtab.data() + shdr.sh_name;
      this->sections[i] = new InputSection(this, shdr, name);
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
      target->has_rel_frag.resize(target->rels.size());
    }
  }

  // Set is_comdat_member bits.
  for (auto &pair : comdat_groups) {
    std::span<u32> entries = pair.second;
    for (u32 i : entries)
      if (this->sections[i])
        this->sections[i]->is_comdat_member = true;
  }
}

void ObjectFile::initialize_ehframe_sections() {
  for (int i = 0; i < sections.size(); i++) {
    InputSection *isec = sections[i];
    if (isec && isec->name == ".eh_frame") {
      read_ehframe(*isec);
      isec->is_ehframe = true;
    }
  }
}

void ObjectFile::read_ehframe(InputSection &isec) {
  std::span<ElfRela> rels = isec.rels;
  std::string_view data = get_string(isec.shdr);
  const char *begin = data.data();

  if (data.empty()) {
    cies.push_back({data});
    return;
  }

  std::unordered_map<u32, u32> offset_to_cie;
  u32 cur_cie = -1;
  u32 cur_cie_offset = -1;

  for (ElfRela rel : rels)
    if (rel.r_type != R_X86_64_32 && rel.r_type != R_X86_64_PC32)
      Fatal() << *isec.file << ": .eh_frame: unsupported relocation type: "
              << rel.r_type;

  while (!data.empty()) {
    u32 size = *(u32 *)data.data();
    if (size == 0) {
      if (data.size() != 4)
        Fatal() << *isec.file << ": .eh_frame: garbage at end of section";
      cies.push_back({data});
      return;
    }

    u32 begin_offset = data.data() - begin;
    u32 end_offset = begin_offset + size + 4;

    if (!rels.empty() && rels[0].r_offset < begin_offset)
      Fatal() << *isec.file << ": .eh_frame: unsupported relocation order";

    std::string_view contents = data.substr(0, size + 4);
    data = data.substr(size + 4);

    std::vector<EhReloc> eh_rels;
    while (!rels.empty() && rels[0].r_offset < end_offset) {
      Symbol *sym = symbols[rels[0].r_sym];
      eh_rels.push_back({sym, rels[0].r_type,
                         (u32)(rels[0].r_offset - begin_offset),
                         rels[0].r_addend});
      rels = rels.subspan(1);
    }

    u32 id = *(u32 *)(contents.data() + 4);
    if (id == 0) {
      // CIE
      cur_cie = cies.size();
      offset_to_cie[begin_offset] = cies.size();
      cies.push_back({contents, std::move(eh_rels)});
    } else {
      // FDE
      u32 cie_offset = begin_offset + 4 - id;
      if (cie_offset != cur_cie_offset) {
        auto it = offset_to_cie.find(cie_offset);
        if (it == offset_to_cie.end())
          Fatal() << *isec.file << ": .eh_frame: bad FDE pointer";
        cur_cie = it->second;
        cur_cie_offset = cie_offset;
      }

      if (eh_rels.empty())
        Fatal() << *isec.file << ": .eh_frame: FDE has no relocations";
      if (eh_rels[0].offset != 8)
        Fatal() << *isec.file << ": .eh_frame: FDE's first location "
                << "should have offset 8";

      cies[cur_cie].fdes.push_back({contents, std::move(eh_rels)});
    }
  }
}

static bool should_write_symtab(Symbol &sym) {
  if (config.discard_all || config.strip_all)
    return false;
  if (sym.esym->st_type == STT_SECTION)
    return false;

  // Local symbols are discarded if --discard-local is given or they
  // are not in a mergeable section. I *believe* we exclude symbols in
  // mergeable sections because (1) they are too many and (2) they are
  // merged, so their origins shouldn't matter, but I dont' really
  // know the rationale. Anyway, this is the behavior of the
  // traditional linkers.
  if (sym.name.starts_with(".L")) {
    if (config.discard_locals)
      return false;

    if (InputSection *isec = sym.input_section)
      if (isec->shdr.sh_flags & SHF_MERGE)
        return false;
  }

  return true;
}

void ObjectFile::initialize_symbols() {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter.inc(elf_syms.size());

  // Initialize local symbols
  Symbol *locals = new Symbol[first_global];

  for (int i = 1; i < first_global; i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = locals[i];

    sym.name = symbol_strtab.data() + esym.st_name;
    sym.file = this;
    sym.st_type = esym.st_type;
    sym.value = esym.st_value;
    sym.esym = &esym;

    if (!esym.is_abs()) {
      if (esym.is_common())
        Fatal() << *this << ": common local symbol?";
      sym.input_section = sections[esym.st_shndx];
    }

    if (should_write_symtab(sym)) {
      sym.write_symtab = true;
      strtab_size += sym.name.size() + 1;
      local_symtab_size += sizeof(ElfSym);
    }
  }

  symbols.resize(elf_syms.size());
  sym_fragments.resize(elf_syms.size() - first_global);

  for (int i = 0; i < first_global; i++)
    symbols[i] = &locals[i];

  // Initialize global symbols
  for (int i = first_global; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    std::string_view name = symbol_strtab.data() + esym.st_name;
    int pos = name.find('@');
    if (pos != std::string_view::npos)
      name = name.substr(0, pos);

    symbols[i] = Symbol::intern(name);

    if (esym.is_common())
      has_common_symbol = true;
  }
}

static int binary_search(std::span<u32> span, u32 val) {
  if (val < span[0])
    return -1;

  int ret = 0;
  while (span.size() > 1) {
    u32 mid = span.size() / 2;
    if (val < span[mid]) {
      span = span.subspan(0, mid);
    } else {
      span = span.subspan(mid);
      ret += mid;
    }
  }
  return ret;
}

void ObjectFile::initialize_mergeable_sections() {
  mergeable_sections.resize(sections.size());

  for (int i = 0; i < sections.size(); i++) {
    if (InputSection *isec = sections[i]) {
      if (isec->shdr.sh_flags & SHF_MERGE) {
        mergeable_sections[i] = new MergeableSection(isec);
        sections[i] = nullptr;
      }
    }
  }

  // Initialize rel_fragments
  for (InputSection *isec : sections) {
    if (!isec || isec->rels.empty())
      continue;

    for (int i = 0; i < isec->rels.size(); i++) {
      const ElfRela &rel = isec->rels[i];
      const ElfSym &esym = elf_syms[rel.r_sym];
      if (esym.st_type != STT_SECTION)
        continue;

      MergeableSection *m = mergeable_sections[esym.st_shndx];
      if (!m)
        continue;

      u32 offset = esym.st_value + rel.r_addend;
      int idx = binary_search(m->frag_offsets, offset);
      if (idx == -1)
        Fatal() << *this << ": bad relocation at " << rel.r_sym;

      SectionFragmentRef ref{m->fragments[idx], (i32)(offset - m->frag_offsets[idx])};
      isec->rel_fragments.push_back(ref);
      isec->has_rel_frag[i] = true;
    }
  }

  // Initialize sym_fragments
  for (int i = 0; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    if (esym.is_abs() || esym.is_common())
      continue;

    MergeableSection *m = mergeable_sections[esym.st_shndx];
    if (!m)
      continue;

    int idx = binary_search(m->frag_offsets, esym.st_value);
    if (idx == -1)
      Fatal() << *this << ": bad symbol value";

    if (i < first_global) {
      symbols[i]->frag_ref.frag = m->fragments[idx];
      symbols[i]->frag_ref.addend = esym.st_value - m->frag_offsets[idx];
    } else {
      sym_fragments[i - first_global].frag = m->fragments[idx];
      sym_fragments[i - first_global].addend = esym.st_value - m->frag_offsets[idx];
    }
  }

  erase(mergeable_sections, [](MergeableSection *m) { return !m; });
}

void ObjectFile::parse() {
  sections.resize(elf_sections.size());
  symtab_sec = find_section(SHT_SYMTAB);

  if (symtab_sec) {
    first_global = symtab_sec->sh_info;
    elf_syms = get_data<ElfSym>(*symtab_sec);
    symbol_strtab = get_string(symtab_sec->sh_link);
  }

  initialize_sections();
  initialize_symbols();
  initialize_mergeable_sections();
  initialize_ehframe_sections();
}

// Symbols with higher priorities overwrites symbols with lower priorities.
// Here is the list of priorities, from the highest to the lowest.
//
//  1. Strong defined symbol
//  2. Weak defined symbol
//  3. Defined symbol in an archive member
//  4. Unclaimed (nonexistent) symbol
//
// Ties are broken by file priority.
static u64 get_rank(InputFile *file, const ElfSym &esym, InputSection *isec) {
  if (isec && isec->is_comdat_member)
    return file->priority;
  if (esym.is_undef()) {
    assert(esym.st_bind == STB_WEAK);
    return ((u64)2 << 32) + file->priority;
  }
  if (esym.st_bind == STB_WEAK)
    return ((u64)1 << 32) + file->priority;
  return file->priority;
}

static u64 get_rank(const Symbol &sym) {
  if (!sym.file)
    return (u64)4 << 32;
  if (sym.is_placeholder)
    return ((u64)3 << 32) + sym.file->priority;
  return get_rank(sym.file, *sym.esym, sym.input_section);
}

void ObjectFile::maybe_override_symbol(Symbol &sym, int symidx) {
  InputSection *isec = nullptr;
  const ElfSym &esym = elf_syms[symidx];
  if (!esym.is_abs() && !esym.is_common())
    isec = sections[esym.st_shndx];

  std::lock_guard lock(sym.mu);

  u64 new_rank = get_rank(this, esym, isec);
  u64 existing_rank = get_rank(sym);

  if (new_rank < existing_rank) {
    sym.file = this;
    sym.input_section = isec;
    sym.frag_ref = sym_fragments[symidx - first_global];
    sym.value = esym.st_value;
    sym.ver_idx = 0;
    sym.st_type = esym.st_type;
    sym.esym = &esym;
    sym.is_placeholder = false;
    sym.is_weak = (esym.st_bind == STB_WEAK);
    sym.is_imported = false;

    if (sym.traced)
      SyncOut() << "trace: " << *sym.file
                << (sym.is_weak ? ": weak definition of " : ": definition of ")
                << sym.name;
  }
}

void ObjectFile::resolve_symbols() {
  for (int i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    if (!esym.is_defined())
      continue;

    Symbol &sym = *symbols[i];

    if (is_in_archive) {
      std::lock_guard lock(sym.mu);
      bool is_new = !sym.file;
      bool tie_but_higher_priority =
        sym.is_placeholder && this->priority < sym.file->priority;

      if (is_new || tie_but_higher_priority) {
        sym.file = this;
        sym.is_placeholder = true;

        if (sym.traced)
          SyncOut() << "trace: " << sym.file << ": lazy definition of " << sym.name;
      }
    } else {
      maybe_override_symbol(sym, i);
    }
  }
}

std::vector<ObjectFile *> ObjectFile::mark_live_objects() {
  std::vector<ObjectFile *> vec;
  assert(is_alive);

  for (int i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.is_defined()) {
      if (is_in_archive)
        maybe_override_symbol(sym, i);
      continue;
    }

    if (sym.traced)
      SyncOut() << "trace: " <<  *this << ": reference to " << sym.name;

    if (esym.st_bind != STB_WEAK && sym.file && !sym.file->is_alive.exchange(true)) {
      if (!sym.file->is_dso)
        vec.push_back((ObjectFile *)sym.file);

      if (sym.traced)
        SyncOut() << "trace: " << *this << " keeps " << *sym.file
                  << " for " << sym.name;
    }
  }
  return vec;
}

void ObjectFile::handle_undefined_weak_symbols() {
  for (int i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];

    if (esym.is_undef() && esym.st_bind == STB_WEAK) {
      Symbol &sym = *symbols[i];
      std::lock_guard lock(sym.mu);

      bool is_new = !sym.file || sym.is_placeholder;
      bool tie_but_higher_priority =
        !is_new && sym.is_undef_weak && this->priority < sym.file->priority;

      if (is_new || tie_but_higher_priority) {
        sym.file = this;
        sym.input_section = nullptr;
        sym.value = 0;
        sym.esym = &esym;
        sym.is_placeholder = false;
        sym.is_undef_weak = true;
        sym.is_imported = false;

        if (sym.traced)
          SyncOut() << "trace: " << *this << ": unresolved weak symbol "
                    << sym.name;
      }
    }
  }
}

void ObjectFile::resolve_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    ObjectFile *cur = group->file;
    while (!cur || cur->priority > this->priority)
      if (group->file.compare_exchange_weak(cur, this))
        break;
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    if (group->file == this)
      continue;

    std::span<u32> entries = pair.second;
    for (u32 i : entries) {
      if (sections[i])
        sections[i]->is_alive = false;
      sections[i] = nullptr;
    }

    static Counter counter("removed_comdat_mem");
    counter.inc(entries.size());
  }
}

void ObjectFile::convert_common_symbols() {
  if (!has_common_symbol)
    return;

  static OutputSection *bss =
    OutputSection::get_instance(".bss", SHT_NOBITS, SHF_WRITE | SHF_ALLOC);

  for (int i = first_global; i < elf_syms.size(); i++) {
    if (!elf_syms[i].is_common())
      continue;

    Symbol *sym = symbols[i];
    if (sym->file != this)
      continue;

    auto *shdr = new ElfShdr;
    memset(shdr, 0, sizeof(*shdr));
    shdr->sh_flags = SHF_ALLOC;
    shdr->sh_type = SHT_NOBITS;
    shdr->sh_size = elf_syms[i].st_size;
    shdr->sh_addralign = 1;

    auto *isec = new InputSection(this, *shdr, ".bss");
    isec->output_section = bss;
    sections.push_back(isec);

    sym->input_section = isec;
    sym->value = 0;
  }
}

static bool should_write_global_symtab(Symbol &sym) {
  return !config.strip_all && sym.esym->st_type != STT_SECTION;
}

void ObjectFile::compute_symtab() {
  for (int i = first_global; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (sym.file == this && should_write_global_symtab(sym)) {
      global_symtab_size += sizeof(ElfSym);
      strtab_size += sym.name.size() + 1;
    }
  }
}

void ObjectFile::write_symtab() {
  u8 *symtab_base = out::buf + out::symtab->shdr.sh_offset;
  u8 *strtab_base = out::buf + out::strtab->shdr.sh_offset;
  u32 symtab_off;
  u32 strtab_off = strtab_offset;

  auto write_sym = [&](u32 i) {
    Symbol &sym = *symbols[i];
    ElfSym &esym = *(ElfSym *)(symtab_base + symtab_off);
    symtab_off += sizeof(ElfSym);

    esym = elf_syms[i];
    esym.st_name = strtab_off;

    if (sym.st_type == STT_TLS)
      esym.st_value = sym.get_addr() - sym.input_section->output_section->shdr.sh_addr;
    else
      esym.st_value = sym.get_addr();

    if (sym.input_section)
      esym.st_shndx = sym.input_section->output_section->shndx;
    else if (sym.shndx)
      esym.st_shndx = sym.shndx;
    else
      esym.st_shndx = SHN_ABS;

    write_string(strtab_base + strtab_off, sym.name);
    strtab_off += sym.name.size() + 1;
  };

  symtab_off = local_symtab_offset;
  for (int i = 1; i < first_global; i++)
    if (symbols[i]->write_symtab)
      write_sym(i);

  symtab_off = global_symtab_offset;
  for (int i = first_global; i < elf_syms.size(); i++)
    if (symbols[i]->file == this && should_write_global_symtab(*symbols[i]))
      write_sym(i);
}

bool is_c_identifier(std::string_view name) {
  static std::regex re("[a-zA-Z_][a-zA-Z0-9_]*");
  return std::regex_match(name.begin(), name.end(), re);
}

ObjectFile::ObjectFile() {
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

  out::__ehdr_start = add("__ehdr_start", STV_HIDDEN);
  out::__rela_iplt_start = add("__rela_iplt_start", STV_HIDDEN);
  out::__rela_iplt_end = add("__rela_iplt_end", STV_HIDDEN);
  out::__init_array_start = add("__init_array_start", STV_HIDDEN);
  out::__init_array_end = add("__init_array_end", STV_HIDDEN);
  out::__fini_array_start = add("__fini_array_start", STV_HIDDEN);
  out::__fini_array_end = add("__fini_array_end", STV_HIDDEN);
  out::__preinit_array_start = add("__preinit_array_start", STV_HIDDEN);
  out::__preinit_array_end = add("__preinit_array_end", STV_HIDDEN);
  out::_DYNAMIC = add("_DYNAMIC", STV_HIDDEN);
  out::_GLOBAL_OFFSET_TABLE_ = add("_GLOBAL_OFFSET_TABLE_", STV_HIDDEN);
  out::__bss_start = add("__bss_start", STV_HIDDEN);
  out::_end = add("_end", STV_HIDDEN);
  out::_etext = add("_etext", STV_HIDDEN);
  out::_edata = add("_edata", STV_HIDDEN);

  for (OutputChunk *chunk : out::chunks) {
    if (!is_c_identifier(chunk->name))
      continue;

    auto *start = new std::string("__start_" + std::string(chunk->name));
    auto *stop = new std::string("__stop_" + std::string(chunk->name));
    add(*start, STV_HIDDEN);
    add(*stop, STV_HIDDEN);
  }

  elf_syms = *esyms;
  sym_fragments.resize(elf_syms.size() - first_global);
}

std::ostream &operator<<(std::ostream &out, const InputFile &file) {
  if (file.is_dso) {
    out << file.name;
    return out;
  }

  ObjectFile *obj = (ObjectFile *)&file;
  if (obj->archive_name == "")
    out << obj->name;
  else
    out << obj->archive_name << ":(" << obj->name + ")";
  return out;
}

std::string_view SharedFile::get_soname() {
  if (ElfShdr *sec = find_section(SHT_DYNAMIC))
    for (ElfDyn &dyn : get_data<ElfDyn>(*sec))
      if (dyn.d_tag == DT_SONAME)
        return std::string_view(symbol_strtab.data() + dyn.d_val);
  return name;
}

void SharedFile::parse() {
  symtab_sec = find_section(SHT_DYNSYM);
  if (!symtab_sec)
    return;

  symbol_strtab = get_string(symtab_sec->sh_link);
  soname = get_soname();
  version_strings = read_verdef();

  // Read a symbol table.
  int first_global = symtab_sec->sh_info;
  std::span<ElfSym> esyms = get_data<ElfSym>(*symtab_sec);

  std::span<u16> vers;
  if (ElfShdr *sec = find_section(SHT_GNU_VERSYM))
    vers = get_data<u16>(*sec);

  std::vector<std::pair<const ElfSym *, u16>> pairs;

  for (int i = first_global; i < esyms.size(); i++) {
    if (!esyms[i].is_defined())
      continue;
    if (!vers.empty() && (vers[i] >> 15) == 1)
      continue;

    if (vers.empty())
      pairs.push_back({&esyms[i], 1});
    else
      pairs.push_back({&esyms[i], vers[i]});
  }

  // Sort symbols by value for find_aliases(), as find_aliases() does
  // binary search on symbols.
  sort(pairs, [](const std::pair<const ElfSym *, u16> &a,
                 const std::pair<const ElfSym *, u16> &b) {
    return a.first->st_value < b.first->st_value;
  });

  elf_syms.reserve(pairs.size());
  versyms.reserve(pairs.size());
  symbols.reserve(pairs.size());

  for (std::pair<const ElfSym *, u16> &x : pairs) {
    elf_syms.push_back(x.first);
    versyms.push_back(x.second);

    std::string_view name = symbol_strtab.data() + x.first->st_name;
    symbols.push_back(Symbol::intern(name));
  }

  static Counter counter("dso_syms");
  counter.inc(elf_syms.size());
}

std::vector<std::string_view> SharedFile::read_verdef() {
  ElfShdr *verdef_sec = find_section(SHT_GNU_VERDEF);
  if (!verdef_sec)
    return {};

  std::string_view verdef = get_string(*verdef_sec);
  std::string_view strtab = get_string(verdef_sec->sh_link);

  std::vector<std::string_view> ret(2);
  auto *ver = (ElfVerdef *)verdef.data();

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
  for (int i = 0; i < symbols.size(); i++) {
    Symbol &sym = *symbols[i];
    const ElfSym &esym = *elf_syms[i];

    std::lock_guard lock(sym.mu);

    u64 new_rank = get_rank(this, esym, nullptr);
    u64 existing_rank = get_rank(sym);

    if (new_rank < existing_rank) {
      sym.file = this;
      sym.input_section = nullptr;
      sym.frag_ref = {};
      sym.value = esym.st_value;
      sym.ver_idx = versyms[i];
      sym.st_type = (esym.st_type == STT_GNU_IFUNC) ? STT_FUNC : esym.st_type;
      sym.esym = &esym;
      sym.is_placeholder = false;
      sym.is_weak = (esym.st_bind == STB_WEAK);
      sym.is_imported = true;

      if (sym.traced)
        SyncOut() << "trace: " << *sym.file
                  << (sym.is_weak ? ": weak definition of " : ": definition of ")
                  << sym.name;
    }
  }
}

std::span<Symbol *> SharedFile::find_aliases(Symbol *sym) {
  assert(sym->file == this);
  auto [begin, end] = std::equal_range(
    symbols.begin(), symbols.end(), sym,
    [&](Symbol *a, Symbol *b) { return a->value < b->value; });
  return {begin, end};
}
