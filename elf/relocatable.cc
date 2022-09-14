// This file implements the -r or ---relocatable option. If the option
// is given to a linker, the linker creates not an executable nor a
// shared library file but instead create another object file by
// combining input object files.
//
// The behavior of a linker with -r is pretty different from the one
// without -r, and adding code for -r to the regular code path could
// severely complicate it. Therefore, we implement -r as an
// independent feature from others. That's why this file share only a
// small amount of code with other files.
//
// Since the -r option is a minor feature, we don't aim for speed in
// this file. This is a "better than nothing" implementation of the -r
// option.
//
// Here is the strategy as to how to combine multiple object files
// into one:
//
//  - Regular sections containing opaque data (e.g. ".text" or
//    ".data") are just copied as-is without merging.
//
//  - .symtab, .strtab and .shstrtab are merged.
//
//  - COMDAT groups are uniquified.
//
//  - Relocation sections are copied one by one, but we need to fix
//    symbol indices.

#include "mold.h"
#include "../archive-file.h"
#include "../output-file.h"

#include <unordered_map>
#include <unordered_set>

namespace mold::elf {

template <typename E> class RObjectFile;

template <typename E>
class RChunk {
public:
  RChunk() {
    out_shdr.sh_addralign = 1;
  }

  virtual ~RChunk() = default;
  virtual void update_shdr(Context<E> &ctx) {}
  virtual void write_to(Context<E> &ctx) = 0;

  std::string_view name;
  i64 shndx = 0;
  ElfShdr<E> in_shdr = {};
  ElfShdr<E> out_shdr = {};
};

template <typename E>
class RInputSection : public RChunk<E> {
public:
  RInputSection(Context<E> &ctx, RObjectFile<E> &file, const ElfShdr<E> &shdr);
  void update_shdr(Context<E> &ctx) override;
  void write_to(Context<E> &ctx) override;

  RObjectFile<E> &file;
};

template <typename E>
class RSymtabSection : public RChunk<E> {
public:
  RSymtabSection() {
    this->name = ".symtab";
    this->out_shdr.sh_type = SHT_SYMTAB;
    this->out_shdr.sh_entsize = sizeof(ElfSym<E>);
    this->out_shdr.sh_addralign = sizeof(Word<E>);
  }

  void add_local_symbol(Context<E> &ctx, RObjectFile<E> &file, i64 idx);
  void add_global_symbol(Context<E> &ctx, RObjectFile<E> &file, i64 idx);
  void update_shdr(Context<E> &ctx) override;
  void write_to(Context<E> &ctx) override;

  std::unordered_map<std::string_view, i64> sym_map;
  std::vector<ElfSym<E>> syms{1};
};

template <typename E>
class RStrtabSection : public RChunk<E> {
public:
  RStrtabSection(std::string_view name) {
    this->name = name;
    this->out_shdr.sh_type = SHT_STRTAB;
    this->out_shdr.sh_size = 1;
  }

  i64 add_string(std::string_view str);
  void write_to(Context<E> &ctx) override;

  std::unordered_map<std::string_view, i64> strings;
};

template <typename E>
class ROutputEhdr : public RChunk<E> {
public:
  ROutputEhdr() {
    this->out_shdr.sh_size = sizeof(ElfEhdr<E>);
  }

  void write_to(Context<E> &ctx) override;
};

template <typename E>
class ROutputShdr : public RChunk<E> {
public:
  ROutputShdr() {
    this->out_shdr.sh_size = sizeof(ElfShdr<E>);
  }

  void update_shdr(Context<E> &ctx) override;
  void write_to(Context<E> &ctx) override;
};

template <typename E>
class RObjectFile {
public:
  RObjectFile(Context<E> &ctx, MappedFile<Context<E>> &mf, bool is_alive);

  void remove_comdats(Context<E> &ctx,
                      std::unordered_set<std::string_view> &groups);

  template <typename T>
  std::span<T> get_data(Context<E> &ctx, const ElfShdr<E> &shdr);

  MappedFile<Context<E>> &mf;
  std::span<ElfShdr<E>> elf_sections;
  std::vector<std::unique_ptr<RInputSection<E>>> sections;
  std::span<const ElfSym<E>> syms;
  std::vector<i64> symidx;
  std::unordered_set<std::string_view> defined_syms;
  std::unordered_set<std::string_view> undef_syms;
  i64 symtab_shndx = 0;
  i64 first_global = 0;
  bool is_alive;
  const char *strtab = nullptr;
  const char *shstrtab = nullptr;
};

template <typename E>
void RSymtabSection<E>::add_local_symbol(Context<E> &ctx, RObjectFile<E> &file,
                                         i64 idx) {
  ElfSym<E> sym = file.syms[idx];
  assert(sym.st_bind == STB_LOCAL);

  if (!sym.is_undef() && !sym.is_abs() && !sym.is_common()) {
    if (!file.sections[sym.st_shndx])
      return;
    sym.st_shndx = file.sections[sym.st_shndx]->shndx;
  }

  std::string_view name = file.strtab + sym.st_name;
  sym.st_name = ctx.r_strtab->add_string(name);

  file.symidx[idx] = syms.size();
  syms.push_back(sym);
}

template <typename E>
void RSymtabSection<E>::add_global_symbol(Context<E> &ctx, RObjectFile<E> &file,
                                          i64 idx) {
  ElfSym<E> sym = file.syms[idx];
  assert(sym.st_bind != STB_LOCAL);

  std::string_view name = file.strtab + sym.st_name;
  auto [it, inserted] = sym_map.insert({name, syms.size()});

  if (inserted) {
    if (!sym.is_undef() && !sym.is_abs() && !sym.is_common())
      sym.st_shndx = file.sections[sym.st_shndx]->shndx;
    sym.st_name = ctx.r_strtab->add_string(name);
    file.symidx[idx] = syms.size();
    syms.push_back(sym);
    return;
  }

  file.symidx[idx] = it->second;

  ElfSym<E> &existing = syms[it->second];
  if (existing.is_undef() && !sym.is_undef()) {
    if (!sym.is_abs() && !sym.is_common())
      sym.st_shndx = file.sections[sym.st_shndx]->shndx;
    sym.st_name = existing.st_name;
    existing = sym;
  }
}

template <typename E>
void RSymtabSection<E>::update_shdr(Context<E> &ctx) {
  this->out_shdr.sh_size = syms.size() * sizeof(ElfSym<E>);
  this->out_shdr.sh_link = ctx.r_strtab->shndx;
}

template <typename E>
void RSymtabSection<E>::write_to(Context<E> &ctx) {
  ElfSym<E> *buf = (ElfSym<E> *)(ctx.buf + this->out_shdr.sh_offset);
  for (i64 i = 1; i < syms.size(); i++)
    buf[i] = syms[i];
}

template <typename E>
RInputSection<E>::RInputSection(Context<E> &ctx, RObjectFile<E> &file,
                                const ElfShdr<E> &shdr)
  : file(file) {
  this->name = file.shstrtab + shdr.sh_name;
  this->in_shdr = shdr;
  this->out_shdr = shdr;
}

template <typename E>
void RInputSection<E>::update_shdr(Context<E> &ctx) {
  switch (this->in_shdr.sh_type) {
  case SHT_GROUP:
    this->out_shdr.sh_link = ctx.r_symtab->shndx;
    this->out_shdr.sh_info = file.symidx[this->in_shdr.sh_info];
    break;
  case SHT_REL:
  case SHT_RELA:
    this->out_shdr.sh_link = ctx.r_symtab->shndx;
    this->out_shdr.sh_info = file.sections[this->in_shdr.sh_info]->shndx;
    break;
  default:
    if (this->in_shdr.sh_link) {
      std::unique_ptr<RInputSection<E>> &sec =
        file.sections[this->in_shdr.sh_info];

      if (sec)
        this->out_shdr.sh_link = sec->shndx;
      else if (this->in_shdr.sh_link == file.symtab_shndx)
        this->out_shdr.sh_link = ctx.r_symtab->shndx;
    }
  }
}

template <typename E>
void RInputSection<E>::write_to(Context<E> &ctx) {
  if (this->in_shdr.sh_type == SHT_NOBITS)
    return;

  std::span<u8> contents = file.template get_data<u8>(ctx, this->in_shdr);
  memcpy(ctx.buf + this->out_shdr.sh_offset, contents.data(), contents.size());

  switch (this->in_shdr.sh_type) {
  case SHT_GROUP: {
    ul32 *mem = (ul32 *)(ctx.buf + this->out_shdr.sh_offset);
    for (i64 i = 1; i < this->out_shdr.sh_size / sizeof(u32); i++)
      mem[i] = file.sections[mem[i]]->shndx;
    break;
  }
  case SHT_REL:
  case SHT_RELA: {
    ElfRel<E> *rel = (ElfRel<E> *)(ctx.buf + this->out_shdr.sh_offset);
    i64 size = this->out_shdr.sh_size / sizeof(ElfRel<E>);

    for (i64 i = 0; i < size; i++) {
      const ElfSym<E> &sym = file.syms[rel[i].r_sym];
      if (sym.is_undef() || sym.is_abs() || sym.is_common() ||
          file.sections[sym.st_shndx])
        rel[i].r_sym = file.symidx[rel[i].r_sym];
      else
        memset(rel + i, 0, sizeof(ElfRel<E>));
    }

    i64 i = 0;
    i64 j = 0;
    for (; j < size; j++)
      if (rel[j].r_type)
        rel[i++] = rel[j];
    for (; i < size; i++)
      memset(rel + i, 0, sizeof(ElfRel<E>));
    break;
  }
  }
}

template <typename E>
i64 RStrtabSection<E>::add_string(std::string_view str) {
  auto [it, inserted] = strings.insert({str, this->out_shdr.sh_size});
  if (inserted)
    this->out_shdr.sh_size += str.size() + 1;
  return it->second;
}

template <typename E>
void RStrtabSection<E>::write_to(Context<E> &ctx) {
  for (auto [str, offset] : strings)
    memcpy(ctx.buf + this->out_shdr.sh_offset + offset, str.data(), str.size());
}

template <typename E>
void ROutputEhdr<E>::write_to(Context<E> &ctx) {
  ElfEhdr<E> &hdr = *(ElfEhdr<E> *)(ctx.buf + this->out_shdr.sh_offset);
  memcpy(&hdr.e_ident, "\177ELF", 4);
  hdr.e_ident[EI_CLASS] = (sizeof(Word<E>) == 8) ? ELFCLASS64 : ELFCLASS32;
  hdr.e_ident[EI_DATA] = ELFDATA2LSB;
  hdr.e_ident[EI_VERSION] = EV_CURRENT;
  hdr.e_type = ET_REL;
  hdr.e_machine = E::e_machine;
  hdr.e_version = EV_CURRENT;
  hdr.e_shoff = ctx.r_shdr->out_shdr.sh_offset;
  hdr.e_ehsize = sizeof(ElfEhdr<E>);
  hdr.e_shentsize = sizeof(ElfShdr<E>);
  hdr.e_shstrndx = ctx.r_shstrtab->shndx;
  hdr.e_shnum = ctx.r_shdr->out_shdr.sh_size / sizeof(ElfShdr<E>);
  hdr.e_shstrndx = ctx.r_shstrtab->shndx;
}

template <typename E>
void ROutputShdr<E>::update_shdr(Context<E> &ctx) {
  for (RChunk<E> *chunk : ctx.r_chunks)
    if (chunk->shndx)
      this->out_shdr.sh_size += sizeof(ElfShdr<E>);
}

template <typename E>
void ROutputShdr<E>::write_to(Context<E> &ctx) {
  ElfShdr<E> *hdr = (ElfShdr<E> *)(ctx.buf + this->out_shdr.sh_offset);
  for (RChunk<E> *chunk : ctx.r_chunks)
    if (chunk->shndx)
      hdr[chunk->shndx] = chunk->out_shdr;
}

template <typename E>
RObjectFile<E>::RObjectFile(Context<E> &ctx, MappedFile<Context<E>> &mf,
                            bool is_alive)
  : mf(mf), is_alive(is_alive) {
  // Read ELF header and section header
  ElfEhdr<E> &ehdr = *(ElfEhdr<E> *)mf.data;
  ElfShdr<E> *sh_begin = (ElfShdr<E> *)(mf.data + ehdr.e_shoff);
  i64 num_sections = (ehdr.e_shnum == 0) ? sh_begin->sh_size : ehdr.e_shnum;
  elf_sections = {sh_begin, sh_begin + num_sections};
  sections.resize(num_sections);

  // Read .shstrtab
  i64 shstrtab_idx = (ehdr.e_shstrndx == SHN_XINDEX)
    ? sh_begin->sh_link : ehdr.e_shstrndx;
  shstrtab = (char *)(mf.data + elf_sections[shstrtab_idx].sh_offset);

  // Read .symtab
  for (i64 i = 1; i < elf_sections.size(); i++) {
    ElfShdr<E> &shdr = elf_sections[i];
    if (shdr.sh_type == SHT_SYMTAB) {
      syms = get_data<const ElfSym<E>>(ctx, shdr);
      strtab = (char *)(mf.data + elf_sections[shdr.sh_link].sh_offset);
      symtab_shndx = i;
      first_global = shdr.sh_info;
      break;
    }
  }

  symidx.resize(syms.size());

  // Read sections
  for (i64 i = 1; i < elf_sections.size(); i++) {
    ElfShdr<E> &shdr = elf_sections[i];
    switch (shdr.sh_type) {
    case SHT_NULL:
    case SHT_SYMTAB:
    case SHT_STRTAB:
      break;
    default:
      sections[i].reset(new RInputSection(ctx, *this, shdr));
    }
  }

  // Read global symbols
  for (i64 i = first_global; i < syms.size(); i++) {
    std::string_view name = strtab + syms[i].st_name;
    if (syms[i].is_defined())
      defined_syms.insert(name);
    else
      undef_syms.insert(name);
  }
}

// Remove duplicate comdat groups
template <typename E>
void RObjectFile<E>::remove_comdats(Context<E> &ctx,
                                    std::unordered_set<std::string_view> &groups) {
  for (i64 i = 1; i < sections.size(); i++) {
    if (!sections[i])
      continue;
    ElfShdr<E> &shdr = sections[i]->in_shdr;
    if (shdr.sh_type != SHT_GROUP)
      continue;

    // Get a comdat group signature and insert it into a set.
    const ElfSym<E> &sym = syms[shdr.sh_info];
    std::string_view signature = strtab + sym.st_name;
    if (groups.insert(signature).second)
      continue;

    // If it is a duplicate, remove it and its members.
    for (i64 j : this->template get_data<u32>(ctx, shdr).subspan(1))
      sections[j] = nullptr;
    sections[i] = nullptr;
  }
}

template <typename E>
template <typename T>
std::span<T> RObjectFile<E>::get_data(Context<E> &ctx, const ElfShdr<E> &shdr) {
  T *begin = (T *)(mf.data + shdr.sh_offset);
  T *end   = (T *)(mf.data + shdr.sh_offset + shdr.sh_size);
  return {begin, end};
}

template <typename E>
static std::vector<std::unique_ptr<RObjectFile<E>>>
open_files(Context<E> &ctx, std::span<std::string> args) {
  std::vector<std::unique_ptr<RObjectFile<E>>> files;
  bool whole_archive = false;

  while (!args.empty()) {
    const std::string &arg = args[0];
    args = args.subspan(1);

    if (arg == "--whole-archive") {
      whole_archive = true;
      continue;
    }

    if (arg == "--no-whole-archive") {
      whole_archive = false;
      continue;
    }

    if (arg.starts_with("--version-script=") ||
        arg.starts_with("--dynamic-list=") ||
        arg.starts_with("--export-dynamic-symbol=") ||
        arg.starts_with("--export-dynamic-symbol-list="))
      continue;

    MappedFile<Context<E>> *mf = nullptr;

    if (arg == "-l") {
      mf = find_library(ctx, arg.substr(2));
    } else {
      if (arg.starts_with('-'))
        continue;
      mf = MappedFile<Context<E>>::must_open(ctx, arg);
    }

    switch (get_file_type(mf)) {
    case FileType::ELF_OBJ:
      files.emplace_back(new RObjectFile<E>(ctx, *mf, true));
      break;
    case FileType::AR:
    case FileType::THIN_AR:
      for (MappedFile<Context<E>> *child : read_archive_members(ctx, mf))
        if (get_file_type(child) == FileType::ELF_OBJ)
          files.emplace_back(new RObjectFile<E>(ctx, *child, whole_archive));
      break;
    default:
      break;
    }
  }
  return files;
}

template <typename E>
static i64 assign_offsets(Context<E> &ctx) {
  i64 offset = 0;
  for (RChunk<E> *chunk : ctx.r_chunks) {
    offset = align_to(offset, chunk->out_shdr.sh_addralign);
    chunk->out_shdr.sh_offset = offset;
    offset += chunk->out_shdr.sh_size;
  }
  return offset;
}

static bool contains(std::unordered_set<std::string_view> &a,
                     std::unordered_set<std::string_view> &b) {
  for (std::string_view x : b)
    if (a.contains(x))
      return true;
  return false;
}

template <typename E>
void combine_objects(Context<E> &ctx, std::span<std::string> file_args) {
  // Read object files
  std::vector<std::unique_ptr<RObjectFile<E>>> files = open_files(ctx, file_args);

  // Identify needed objects
  std::unordered_set<std::string_view> undef_syms;
  auto add_syms = [&](RObjectFile<E> &file) {
    undef_syms.insert(file.undef_syms.begin(), file.undef_syms.end());
    for (std::string_view name : file.defined_syms)
      undef_syms.erase(name);
    file.is_alive = true;
  };

  for (std::unique_ptr<RObjectFile<E>> &file : files)
    if (file->is_alive)
      add_syms(*file);

  for (;;) {
    bool added = false;
    for (std::unique_ptr<RObjectFile<E>> &file : files) {
      if (!file->is_alive && contains(undef_syms, file->defined_syms)) {
        add_syms(*file);
        added = true;
      }
    }
    if (!added)
      break;
  }

  std::erase_if(files, [](std::unique_ptr<RObjectFile<E>> &file) {
    return !file->is_alive;
  });

  // Remove duplicate comdat groups
  std::unordered_set<std::string_view> comdat_groups;
  for (std::unique_ptr<RObjectFile<E>> &file : files)
    file->remove_comdats(ctx, comdat_groups);

  // Create headers and linker-synthesized sections
  ROutputEhdr<E> ehdr;
  ROutputShdr<E> shdr;
  RSymtabSection<E> symtab;
  RStrtabSection<E> shstrtab(".shstrtab");
  RStrtabSection<E> strtab(".strtab");

  ctx.r_chunks.push_back(&ehdr);
  ctx.r_chunks.push_back(&shstrtab);
  ctx.r_chunks.push_back(&strtab);

  ctx.r_ehdr = &ehdr;
  ctx.r_shdr = &shdr;
  ctx.r_shstrtab = &shstrtab;
  ctx.r_strtab = &strtab;
  ctx.r_symtab = &symtab;

  // Add input sections to output sections
  for (std::unique_ptr<RObjectFile<E>> &file : files)
    for (std::unique_ptr<RInputSection<E>> &sec : file->sections)
      if (sec)
        ctx.r_chunks.push_back(sec.get());

  ctx.r_chunks.push_back(&symtab);
  ctx.r_chunks.push_back(&shdr);

  // Assign output section indices
  i64 shndx = 1;
  for (RChunk<E> *chunk : ctx.r_chunks)
    if (chunk != &ehdr && chunk != &shdr)
      chunk->shndx = shndx++;

  // Add section names to .shstrtab
  for (RChunk<E> *chunk : ctx.r_chunks)
    if (chunk->shndx)
      chunk->out_shdr.sh_name = shstrtab.add_string(chunk->name);

  // Copy symbols from input objects to an output object
  for (std::unique_ptr<RObjectFile<E>> &file : files)
    for (i64 i = 1; i < file->first_global; i++)
      symtab.add_local_symbol(ctx, *file, i);

  symtab.out_shdr.sh_info = symtab.syms.size();

  for (std::unique_ptr<RObjectFile<E>> &file : files)
    for (i64 i = file->first_global; i < file->syms.size(); i++)
      symtab.add_global_symbol(ctx, *file, i);

  // Finalize section header
  for (RChunk<E> *chunk : ctx.r_chunks)
    chunk->update_shdr(ctx);

  // Open an output file
  i64 filesize = assign_offsets(ctx);
  std::unique_ptr<OutputFile<Context<E>>> out =
    OutputFile<Context<E>>::open(ctx, ctx.arg.output, filesize, 0666);
  memset(out->buf, 0, filesize);
  ctx.buf = out->buf;

  // Write to the output file
  for (RChunk<E> *chunk : ctx.r_chunks)
    chunk->write_to(ctx);
  out->close(ctx);
}

#define INSTANTIATE(E)                                                  \
  template void combine_objects(Context<E> &, std::span<std::string>);

INSTANTIATE_ALL;

} // namespace mold::elf
