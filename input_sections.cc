#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

InputSection::InputSection(ObjectFile *file, const ELF64LE::Shdr &shdr, StringRef name)
  : file(file), shdr(shdr) {
  this->name = name;
  this->output_section = OutputSection::get_instance(name, shdr.sh_flags, shdr.sh_type);

  u64 align = (shdr.sh_addralign == 0) ? 1 : shdr.sh_addralign;
  if (align > UINT32_MAX)
    error(toString(file) + ": section sh_addralign is too large");
  if (__builtin_popcount(align) != 1)
    error(toString(file) + ": section sh_addralign is not a power of two");
}

void InputSection::copy_to(u8 *buf) {
  if (shdr.sh_type == SHT_NOBITS || shdr.sh_size == 0)
    return;

  ArrayRef<u8> data = check(file->obj.getSectionContents(shdr));
  memcpy(buf + output_section->shdr.sh_offset + offset, &data[0], data.size());

  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
    u8 *loc = buf + output_section->shdr.sh_offset + offset + rel.r_offset;

    u64 G = sym->got_offset;
    u64 GOT = out::got->shdr.sh_addr;
    u64 S = sym->get_addr();
    u64 A = rel.r_addend;
    u64 P = output_section->shdr.sh_addr + offset + rel.r_offset;
    u64 L = out::plt->shdr.sh_addr + sym->plt_offset;

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
      if (!config.is_static || sym->type == STT_GNU_IFUNC)
        *(u32 *)loc = L + A - P;
      else
        *(u32 *)loc = S + A - P; // todo
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
      *(u32 *)loc = sym->gottp_offset + GOT + A - P;
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

    static Counter counter("relocs");
    counter.inc();
  }
}

static bool set_flag(Symbol *sym, u8 bit) {
  for (;;) {
    u8 flags = sym->flags;
    if (flags & bit)
      return false;
    if (sym->flags.compare_exchange_strong(flags, flags | bit))
      return true;
  }
}

void InputSection::scan_relocations() {
  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
    if (!sym->file || !sym->file->is_alive)
      continue;

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
      if (config.is_static && sym->type != STT_GNU_IFUNC)
        break;
      if (set_flag(sym, Symbol::NEEDS_PLT)) {
        sym->file->num_plt++;
        sym->file->num_gotplt++;
        sym->file->num_relplt++;
      }
      break;
    }
  }
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
