#include "mold.h"

#include <cstring>
#include <regex>

InputFile::InputFile(MemoryMappedFile mb)
  : mb(mb), name(mb.name), ehdr(*(ElfEhdr *)mb.data), is_dso(ehdr.e_type == ET_DYN) {
  if (mb.size < sizeof(ElfEhdr))
    error(to_string(this) + ": file too small");
  if (memcmp(mb.data, "\177ELF", 4))
    error(to_string(this) + ": not an ELF file");

  u8 *sh_begin = mb.data + ehdr.e_shoff;
  u8 *sh_end = sh_begin + ehdr.e_shnum * sizeof(ElfShdr);
  if (mb.data + mb.size < sh_end)
    error(to_string(this) + ": e_shoff or e_shnum corrupted: " +
          std::to_string(mb.size) + " " + std::to_string(ehdr.e_shnum));
  elf_sections = {(ElfShdr *)sh_begin, (ElfShdr *)sh_end};
}

std::string_view InputFile::get_string(const ElfShdr &shdr) const {
  u8 *begin = mb.data + shdr.sh_offset;
  u8 *end = begin + shdr.sh_size;
  if (mb.data + mb.size < end)
    error(to_string(this) + ": shdr corrupted");
  return {(char *)begin, (char *)end};
}

std::string_view InputFile::get_string(u32 idx) const {
  if (elf_sections.size() <= idx)
    error(to_string(this) + ": invalid section index");
  return get_string(elf_sections[idx]);
}

template<typename T>
std::span<T> InputFile::get_data(const ElfShdr &shdr) const {
  std::string_view view = get_string(shdr);
  if (view.size() % sizeof(T))
    error(to_string(this) + ": corrupted section");
  return {(T *)view.data(), view.size() / sizeof(T)};
}

template<typename T>
std::span<T> InputFile::get_data(u32 idx) const {
  if (elf_sections.size() <= idx)
    error(to_string(this) + ": invalid section index");
  return get_data<T>(elf_sections[idx]);
}

ElfShdr *InputFile::find_section(u32 type) {
  for (ElfShdr &sec : elf_sections)
    if (sec.sh_type == type)
      return &sec;
  return nullptr;
}

ObjectFile::ObjectFile(MemoryMappedFile mb, std::string archive_name)
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
        error(to_string(this) + ": invalid symbol index");
      const ElfSym &sym = elf_syms[shdr.sh_info];
      std::string_view signature = symbol_strtab.data() + sym.st_name;

      // Get comdat group members.
      std::span<u32> entries = get_data<u32>(shdr);

      if (entries.empty())
        error(to_string(this) + ": empty SHT_GROUP");
      if (entries[0] == 0)
        continue;
      if (entries[0] != GRP_COMDAT)
        error(to_string(this) + ": unsupported SHT_GROUP format");

      static ConcurrentMap<ComdatGroup> map;
      ComdatGroup *group = map.insert(signature, ComdatGroup(nullptr, 0));
      comdat_groups.push_back({group, entries});

      static Counter counter("comdats");
      counter.inc();
      break;
    }
    case SHT_SYMTAB_SHNDX:
      error(to_string(this) + ": SHT_SYMTAB_SHNDX section is not supported");
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

      std::string_view shstrtab = get_string(ehdr.e_shstrndx);
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
      error(to_string(this) + ": invalid relocated section index: " +
            std::to_string((u32)shdr.sh_info));

    InputSection *target = sections[shdr.sh_info];
    if (target) {
      target->rels = get_data<ElfRela>(shdr);
      target->rel_types.resize(target->rels.size());
      target->has_rel_piece.resize(target->rels.size());

      if (target->shdr.sh_flags & SHF_ALLOC) {
        static Counter counter("relocs_alloc");
        counter.inc(target->rels.size());
      }
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

static bool should_write_symtab(const ElfSym &esym, std::string_view name) {
  if (config.discard_all || config.strip_all)
    return false;
  if (esym.st_type == STT_SECTION)
    return false;
  if (config.discard_locals && name.starts_with(".L"))
    return false;
  return true;
}

void ObjectFile::initialize_symbols() {
  if (!symtab_sec)
    return;

  static Counter counter("all_syms");
  counter.inc(elf_syms.size());

  symbols.reserve(elf_syms.size());
  local_symbols.reserve(first_global);
  sym_pieces.resize(elf_syms.size() - first_global);

  // First symbol entry is always null
  local_symbols.emplace_back("");
  symbols.push_back(&local_symbols.back());

  // Initialize local symbols
  for (int i = 1; i < first_global; i++) {
    const ElfSym &esym = elf_syms[i];
    std::string_view name = symbol_strtab.data() + esym.st_name;

    local_symbols.emplace_back(name);
    Symbol &sym = local_symbols.back();

    sym.file = this;
    sym.type = esym.st_type;
    sym.value = esym.st_value;
    sym.esym = &esym;
    sym.write_symtab = should_write_symtab(esym, name);

    if (!esym.is_abs()) {
      if (esym.is_common())
        error("common local symbol?");
      sym.input_section = sections[esym.st_shndx];
    }

    symbols.push_back(&local_symbols.back());

    if (sym.write_symtab) {
      strtab_size += name.size() + 1;
      local_symtab_size += sizeof(ElfSym);
    }
  }

  // Initialize global symbols
  for (int i = first_global; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    std::string_view name = symbol_strtab.data() + esym.st_name;
    int pos = name.find('@');
    if (pos != std::string_view::npos)
      name = name.substr(0, pos);

    symbols.push_back(Symbol::intern(name));

    if (esym.is_common())
      has_common_symbol = true;
  }
}

static const StringPieceRef *
binary_search(std::span<StringPieceRef> pieces, u32 offset) {
  if (offset < pieces[0].input_offset)
    return nullptr;

  while (pieces.size() > 1) {
    u32 mid = pieces.size() / 2;
    const StringPieceRef &ref = pieces[mid];

    if (offset < ref.input_offset)
      pieces = pieces.subspan(0, mid);
    else
      pieces = pieces.subspan(mid);
  }
  return &pieces[0];
}

static bool is_mergeable(const ElfShdr &shdr) {
  return (shdr.sh_flags & SHF_MERGE) &&
         (shdr.sh_flags & SHF_STRINGS) &&
         shdr.sh_entsize == 1;
}

void ObjectFile::initialize_mergeable_sections() {
  mergeable_sections.resize(sections.size());

  for (int i = 0; i < sections.size(); i++) {
    InputSection *isec = sections[i];
    if (isec && is_mergeable(isec->shdr)) {
      mergeable_sections[i] = new MergeableSection(isec, get_string(isec->shdr));
      sections[i] = nullptr;
    }
  }

  // Initialize rel_pieces
  for (InputSection *isec : sections) {
    if (!isec || isec->rels.empty())
      continue;

    for (int i = 0; i < isec->rels.size(); i++) {
      const ElfRela &rel = isec->rels[i];

      switch (rel.r_type) {
      case R_X86_64_64:
      case R_X86_64_PC32:
      case R_X86_64_32:
      case R_X86_64_32S:
      case R_X86_64_16:
      case R_X86_64_PC16:
      case R_X86_64_8:
      case R_X86_64_PC8:
        if (rel.r_sym >= this->first_global)
          continue;

        const ElfSym &esym = elf_syms[rel.r_sym];
        if (esym.st_type != STT_SECTION)
          continue;

        MergeableSection *m = mergeable_sections[esym.st_shndx];
        if (!m)
          continue;

        u32 offset = esym.st_value + rel.r_addend;
        const StringPieceRef *ref = binary_search(m->pieces, offset);
        if (!ref)
          error(to_string(this) + ": bad relocation at " + std::to_string(rel.r_sym));

        isec->rel_pieces.push_back(
          {.piece = ref->piece, .addend = (i32)(offset - ref->input_offset)});
        isec->has_rel_piece[i] = true;
      }
    }
  }

  // Initialize sym_pieces
  for (int i = 0; i < elf_syms.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    if (esym.is_abs() || esym.is_common())
      continue;

    MergeableSection *m = mergeable_sections[esym.st_shndx];
    if (!m)
      continue;

    const StringPieceRef *ref = binary_search(m->pieces, esym.st_value);
    if (!ref)
      error(to_string(this) + ": bad symbol value");

    if (i < first_global) {
      local_symbols[i].piece_ref = *ref;
    } else {
      sym_pieces[i - first_global].piece = ref->piece;
      sym_pieces[i - first_global].addend = esym.st_value - ref->input_offset;
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
    sym.piece_ref = sym_pieces[symidx - first_global];
    sym.value = esym.st_value;
    sym.ver_idx = 0;
    sym.type = esym.st_type;
    sym.esym = &esym;
    sym.is_placeholder = false;
    sym.is_weak = (esym.st_bind == STB_WEAK);
    sym.is_imported = false;

    if (UNLIKELY(sym.traced))
      message("trace: " + to_string(sym.file) +
              (sym.is_weak ? ": weak definition of " : ": definition of ") +
              std::string(sym.name));
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

        if (UNLIKELY(sym.traced))
          message("trace: " + to_string(sym.file) + ": lazy definition of " +
                  std::string(sym.name));
      }
    } else {
      maybe_override_symbol(sym, i);
    }
  }
}

void
ObjectFile::mark_live_objects(tbb::parallel_do_feeder<ObjectFile *> &feeder) {
  assert(is_alive);

  for (int i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.is_defined()) {
      if (is_in_archive)
        maybe_override_symbol(sym, i);
      continue;
    }

    if (UNLIKELY(sym.traced))
      message("trace: " + to_string(this) + ": reference to " + std::string(sym.name));

    if (esym.st_bind != STB_WEAK && sym.file &&
        !sym.file->is_alive.exchange(true)) {
      if (!sym.file->is_dso)
        feeder.add((ObjectFile *)sym.file);

      if (UNLIKELY(sym.traced))
        message("trace: " + to_string(this) + " keeps " + to_string(sym.file) +
                " for " + std::string(sym.name));
    }
  }
}

void ObjectFile::handle_undefined_weak_symbols() {
  if (!is_alive)
    return;

  for (int i = first_global; i < symbols.size(); i++) {
    const ElfSym &esym = elf_syms[i];
    Symbol &sym = *symbols[i];

    if (esym.is_undef() && esym.st_bind == STB_WEAK) {
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

        if (UNLIKELY(sym.traced))
          message("trace: " + to_string(this) + ": unresolved weak symbol " +
                  std::string(sym.name));
      }
    }
  }
}

void ObjectFile::resolve_comdat_groups() {
  if (!is_alive)
    return;

  for (auto &pair : comdat_groups) {
    ComdatGroup *group = pair.first;
    ObjectFile *cur = group->file;
    while (!cur || cur->priority > this->priority)
      if (group->file.compare_exchange_weak(cur, this))
        break;
  }
}

void ObjectFile::eliminate_duplicate_comdat_groups() {
  if (!is_alive)
    return;

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

    if (sym.type == STT_TLS)
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

ObjectFile *ObjectFile::create_internal_file() {
  // Create a dummy object file.
  constexpr int bufsz = 256;
  u8 *buf = (u8 *)calloc(1, bufsz);
  memcpy(buf, "\177ELF", 4);
  MemoryMappedFile *mb = new MemoryMappedFile("<internal>", buf, bufsz);
  auto *obj = new ObjectFile(*mb, "");

  // Create linker-synthesized symbols.
  auto *elf_syms = new std::vector<ElfSym>(1);
  obj->symbols.push_back(new Symbol(""));
  obj->first_global = 1;
  obj->is_alive = true;

  auto add = [&](std::string_view name, u8 visibility = STV_DEFAULT) {
    ElfSym esym = {};
    esym.st_type = STT_NOTYPE;
    esym.st_shndx = SHN_ABS;
    esym.st_bind = STB_GLOBAL;
    esym.st_visibility = visibility;
    elf_syms->push_back(esym);

    Symbol *sym = Symbol::intern(name);
    obj->symbols.push_back(sym);
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

  obj->elf_syms = *elf_syms;
  obj->sym_pieces.resize(elf_syms->size() - obj->first_global);
  return obj;
}

std::string to_string(const InputFile *file) {
  if (file->is_dso)
    return file->name;

  ObjectFile *obj = (ObjectFile *)file;
  if (obj->archive_name == "")
    return obj->name;
  return std::string(obj->archive_name) + ":(" + std::string(obj->name) + ")";
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
  std::stable_sort(pairs.begin(), pairs.end(),
                   [](const std::pair<const ElfSym *, u16> &a,
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
      sym.piece_ref = {};
      sym.value = esym.st_value;
      sym.ver_idx = versyms[i];
      sym.type = (esym.st_type == STT_GNU_IFUNC) ? STT_FUNC : esym.st_type;
      sym.esym = &esym;
      sym.is_placeholder = false;
      sym.is_weak = (esym.st_bind == STB_WEAK);
      sym.is_imported = true;

      if (UNLIKELY(sym.traced))
        message("trace: " + to_string(sym.file) +
                (sym.is_weak ? ": weak definition of " : ": definition of ") +
                std::string(sym.name));
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
