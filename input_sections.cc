#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;

std::atomic_int num_relocs;

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
  buf = buf + output_section->shdr.sh_offset + offset;
  memcpy_nontemporal(buf, &data[0], data.size());
}

std::tuple<u64, u64> InputSection::scan_relocations() {
  uint64_t num_got = 0;
  uint64_t num_plt = 0;

  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->get_symbol(rel.getSymbol(false));
    if (!sym || !sym->file)
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
    case R_X86_64_GOTTPOFF:
    case R_X86_64_REX_GOTPCRELX:
      num_got += !sym->needs_got.exchange(true);
      break;
#if 0
    case R_X86_64_PLT32:
      num_got += !sym->needs_got.exchange(true);
      num_plt += !sym->needs_plt.exchange(true);
      break;
#endif
    }
  }

  return {num_got, num_plt};
}

void InputSection::relocate(u8 *buf) {
  for (const ELF64LE::Rela &rel : rels) {
    u32 sym_idx = rel.getSymbol(false);
    Symbol *sym = file->get_symbol(sym_idx);
    u8 *loc = buf + output_section->shdr.sh_offset + offset + rel.r_offset;

    u64 P = output_section->shdr.sh_addr + offset + rel.r_offset;
    u64 S = sym ? sym->addr : file->get_symbol_addr(sym_idx);
    u64 A = rel.r_addend;
    u64 G = sym ? sym->got_addr : 0;
    u64 GOT = out::got->shdr.sh_addr;

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
      break; // todo
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
    case R_X86_64_GOTTPOFF:
    case R_X86_64_TPOFF32:
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
      error(toString(this) + ": unknown relocation: " + std::to_string(rel.getType(false)));
    }
    // num_relocs++;
  }
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
