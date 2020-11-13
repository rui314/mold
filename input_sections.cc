#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

InputChunk::InputChunk(Kind kind, ObjectFile *file, const ELF64LE::Shdr &shdr,
                       StringRef name)
  : kind(kind), file(file), shdr(shdr), name(name) {
  output_section = OutputSection::get_instance(name, shdr.sh_flags, shdr.sh_type);

  u64 align = (shdr.sh_addralign == 0) ? 1 : shdr.sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
}

void InputSection::copy_to(u8 *buf) {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  // Copy data
  ArrayRef<u8> data = check(file->obj.getSectionContents(shdr));
  memcpy(buf + output_section->shdr.sh_offset + offset, &data[0], data.size());

  // Apply relocations
  u8 *base = buf + output_section->shdr.sh_offset + offset;
  u64 sh_addr = output_section->shdr.sh_addr + offset;
  u64 GOT = out::got->shdr.sh_addr;

  for (int i = 0; i < rels.size(); i++) {
    const ELF64LE::Rela &rel = rels[i];
    StringPieceRef &ref = rel_pieces[i];

    Symbol &sym = *file->symbols[rel.getSymbol(false)];
    u8 *loc = base + rel.r_offset;

    u64 G = sym.got_offset;
    u64 S = ref.piece ? ref.piece->get_addr() : sym.get_addr();
    u64 A = ref.piece ? ref.addend : rel.r_addend;
    u64 P = sh_addr + rel.r_offset;
    u64 L = out::plt->shdr.sh_addr + sym.plt_offset;

    switch (rel.getType(false)) {
    case R_X86_64_NONE:
      break;
    case R_X86_64_64:
      *(u64 *)loc = S + A;
      break;
    case R_X86_64_PC32:
      *(u32 *)loc = S + A - P;
      break;
    case R_X86_64_GOT32:
      *(u64 *)loc = G + A;
      break;
    case R_X86_64_PLT32:
      if (config.is_static && sym.type != STT_GNU_IFUNC)
        *(u32 *)loc = S + A - P; // todo
      else
        *(u32 *)loc = L + A - P;
      break;
    case R_X86_64_GOTPCREL:
      *(u32 *)loc = G + GOT + A - P;
      break;
    case R_X86_64_32:
    case R_X86_64_32S:
      *(u32 *)loc = S + A;
      break;
    case R_X86_64_16:
      *(u16 *)loc = S + A;
      break;
    case R_X86_64_PC16:
      *(u16 *)loc = S + A - P;
      break;
    case R_X86_64_8:
      *loc = S + A;
      break;
    case R_X86_64_PC8:
      *loc = S + A - P;
      break;
    case R_X86_64_TLSGD:
    case R_X86_64_TLSLD:
    case R_X86_64_DTPOFF32:
      // TODO
      break;
    case R_X86_64_GOTTPOFF:
      *(u32 *)loc = sym.gottp_offset + GOT + A - P;
      break;
    case R_X86_64_TPOFF32:
      *(u32 *)loc = S - out::tls_end;
      break;
    case R_X86_64_PC64:
      *(u64 *)loc = S + A - P;
      break;
    case R_X86_64_GOTPC32:
      *(u32 *)loc = GOT + A - P;
      break;
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      *(u32 *)loc = G + GOT + A - P;
      break;
    default:
      error(toString(this) + ": unknown relocation: " +
            std::to_string(rel.getType(false)));
    }
  }

  static Counter counter("relocs");
  counter.inc(rels.size());
}

static bool set_flag(Symbol *sym, u8 bit) {
  u8 flags = sym->flags;
  while (!(flags & bit))
    if (sym->flags.compare_exchange_strong(flags, flags | bit))
      return true;
  return false;
}

void InputSection::scan_relocations() {
  if (!(shdr.sh_flags & SHF_ALLOC))
    return;

  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
    if (!sym->file || !sym->file->is_alive)
      continue;

    if (sym->file->is_dso && set_flag(sym, Symbol::NEEDS_DYNSYM)) {
      sym->file->num_dynsym++;
      sym->file->dynstr_size += sym->name.size() + 1;
    }

    if (sym->type == STT_GNU_IFUNC && set_flag(sym, Symbol::NEEDS_PLT))
      sym->file->num_plt++;

    switch (rel.getType(false)) {
    case R_X86_64_GOT32:
    case R_X86_64_GOT64:
    case R_X86_64_GOTOFF64:
    case R_X86_64_GOTPC32:
    case R_X86_64_GOTPC32_TLSDESC:
    case R_X86_64_GOTPC64:
    case R_X86_64_GOTPCREL:
    case R_X86_64_GOTPCRELX:
    case R_X86_64_REX_GOTPCRELX:
      if (set_flag(sym, Symbol::NEEDS_GOT))
        sym->file->num_got++;
      break;
    case R_X86_64_GOTTPOFF:
      if (set_flag(sym, Symbol::NEEDS_GOTTP))
        sym->file->num_got++;
      break;
    case R_X86_64_PLT32:
      if (config.is_static)
        break;
      if (set_flag(sym, Symbol::NEEDS_PLT))
        sym->file->num_plt++;
      break;
    }
  }
}

MergeableSection::MergeableSection(InputSection *isec, ArrayRef<u8> contents)
  : InputChunk(MERGEABLE, isec->file, isec->shdr, isec->name),
    parent(*MergedSection::get_instance(isec->name, isec->shdr.sh_flags,
                                        isec->shdr.sh_type)) {
  StringRef data((const char *)&contents[0], contents.size());
  u32 offset = 0;

  while (!data.empty()) {
    size_t end = data.find('\0');
    if (end == StringRef::npos)
      error(toString(this) + ": string is not null terminated");

    StringRef substr = data.substr(0, end + 1);
    data = data.substr(end + 1);

    StringPiece *piece = parent.map.insert(substr, StringPiece(substr));
    pieces.push_back({piece, offset});
    offset += substr.size();
  }

  static Counter counter("string_pieces");
  counter.inc(pieces.size());
}

std::string toString(InputChunk *chunk) {
  return (toString(chunk->file) + ":(" + chunk->name + ")").str();
}
