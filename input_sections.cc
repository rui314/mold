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

void InputSection::scan_relocations() {
  for (const ELF64LE::Rela &rel : rels) {
    Symbol *sym = file->symbols[rel.getSymbol(false)];
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
    case R_X86_64_REX_GOTPCRELX: {
      std::lock_guard lock(sym->mu);
      if (sym->got_offset == 0) {
        sym->got_offset = -1;
        file->num_got++;
      }
      break;
    }
    case R_X86_64_GOTTPOFF: {
      std::lock_guard lock(sym->mu);
      if (sym->gottp_offset == 0) {
        sym->gottp_offset = -1;
        file->num_got++;
      }
      break;
    }
    case R_X86_64_PLT32: {
      if (sym->type != STT_GNU_IFUNC)
        break;

      std::lock_guard lock(sym->mu);
      if (sym->plt_offset == 0) {
        assert(sym->gotplt_offset == 0);
        sym->plt_offset = -1;
        sym->gotplt_offset = -1;
        file->num_plt++;
        file->num_gotplt++;
        file->num_relplt++;
      }
      break;
    }
    }
  }
}

void InputSection::relocate(u8 *buf) {
  for (const ELF64LE::Rela &rel : rels) {
    u32 sym_idx = rel.getSymbol(false);
    Symbol *sym = file->symbols[sym_idx];
    u8 *loc = buf + output_section->shdr.sh_offset + offset + rel.r_offset;

    u64 G = sym->got_offset;
    u64 GOT = out::got->shdr.sh_addr;
    u64 S = sym->addr;
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
      if (sym->type == STT_GNU_IFUNC)
        *(u32 *)loc = out::plt->shdr.sh_addr + sym->plt_offset + A - P;
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
    case R_X86_64_GOTTPOFF: {
      if (sym) {
        *(u32 *)loc = sym->gottp_offset + GOT + A - P;
        break;
      }

      if (loc[-3] == 0x48 && loc[-2] == 0x8b) {
        // Rewrite `movq foo@gottpoff(%rip), %r[8-15]` to `movq $foo, %r[8-15]`
        loc[-3] = 0x48;
        loc[-2] = 0xc7;
        loc[-1] = 0xc0 | (loc[-1] >> 3);
        *(u32 *)loc = S - out::tls_end;
        break;
      }

      if (loc[-3] == 0x4c && loc[-2] == 0x8b) {
        // Rewrite `movq foo@gottpoff(%rip), %reg` to `movq $foo, %reg`
        loc[-3] = 0x49;
        loc[-2] = 0xc7;
        loc[-1] = 0xc0 | (loc[-1] >> 3);
        *(u32 *)loc = S - out::tls_end;
        break;
      }

      llvm::errs() << format("unsupported GOTTPOFF: 0x%02x 0x%02x 0x%02x\n",
                             loc[-3], loc[-2], loc[-1]);
      error(toString(this));
      break;
    }
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
    // num_relocs++;
  }
}

std::string toString(InputSection *isec) {
  return (toString(isec->file) + ":(" + isec->name + ")").str();
}
