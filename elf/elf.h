#pragma once

#include "../inttypes.h"

#include <cstdint>
#include <ostream>
#include <string>

namespace mold::elf {

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

struct X86_64;
struct I386;
struct ARM64;
struct ARM32;
struct RISCV64;
struct RISCV32;

template <typename E> struct ElfSym;
template <typename E> struct ElfShdr;
template <typename E> struct ElfEhdr;
template <typename E> struct ElfPhdr;
template <typename E> struct ElfRel;
template <typename E> struct ElfDyn;
template <typename E> struct ElfChdr;

enum class MachineType { NONE, X86_64, I386, ARM64, ARM32, RISCV64, RISCV32 };

inline std::ostream &operator<<(std::ostream &out, MachineType mt) {
  switch (mt) {
  case MachineType::NONE:    out << "none";    break;
  case MachineType::X86_64:  out << "x86_64";  break;
  case MachineType::I386:    out << "i386";    break;
  case MachineType::ARM64:   out << "arm64";   break;
  case MachineType::ARM32:   out << "arm32";   break;
  case MachineType::RISCV64: out << "riscv64"; break;
  case MachineType::RISCV32: out << "riscv32"; break;
  }
  return out;
}

template <typename E>
std::string rel_to_string(u32 r_type);

template <typename E>
std::ostream &operator<<(std::ostream &out, const ElfRel<E> &rel) {
  out << rel_to_string<E>(rel.r_type);
  return out;
}

static constexpr u32 SHN_UNDEF = 0;
static constexpr u32 SHN_ABS = 0xfff1;
static constexpr u32 SHN_COMMON = 0xfff2;
static constexpr u32 SHN_XINDEX = 0xffff;

static constexpr u32 SHT_NULL = 0;
static constexpr u32 SHT_PROGBITS = 1;
static constexpr u32 SHT_SYMTAB = 2;
static constexpr u32 SHT_STRTAB = 3;
static constexpr u32 SHT_RELA = 4;
static constexpr u32 SHT_HASH = 5;
static constexpr u32 SHT_DYNAMIC = 6;
static constexpr u32 SHT_NOTE = 7;
static constexpr u32 SHT_NOBITS = 8;
static constexpr u32 SHT_REL = 9;
static constexpr u32 SHT_SHLIB = 10;
static constexpr u32 SHT_DYNSYM = 11;
static constexpr u32 SHT_INIT_ARRAY = 14;
static constexpr u32 SHT_FINI_ARRAY = 15;
static constexpr u32 SHT_PREINIT_ARRAY = 16;
static constexpr u32 SHT_GROUP = 17;
static constexpr u32 SHT_SYMTAB_SHNDX = 18;
static constexpr u32 SHT_RELR = 19;
static constexpr u32 SHT_LLVM_ADDRSIG = 0x6fff4c03;
static constexpr u32 SHT_GNU_HASH = 0x6ffffff6;
static constexpr u32 SHT_GNU_VERDEF = 0x6ffffffd;
static constexpr u32 SHT_GNU_VERNEED = 0x6ffffffe;
static constexpr u32 SHT_GNU_VERSYM = 0x6fffffff;
static constexpr u32 SHT_X86_64_UNWIND = 0x70000001;
static constexpr u32 SHT_ARM_EXIDX = 0x70000001;
static constexpr u32 SHT_ARM_ATTRIBUTES = 0x70000003;

static constexpr u32 SHF_WRITE = 0x1;
static constexpr u32 SHF_ALLOC = 0x2;
static constexpr u32 SHF_EXECINSTR = 0x4;
static constexpr u32 SHF_MERGE = 0x10;
static constexpr u32 SHF_STRINGS = 0x20;
static constexpr u32 SHF_INFO_LINK = 0x40;
static constexpr u32 SHF_LINK_ORDER = 0x80;
static constexpr u32 SHF_GROUP = 0x200;
static constexpr u32 SHF_TLS = 0x400;
static constexpr u32 SHF_COMPRESSED = 0x800;
static constexpr u32 SHF_GNU_RETAIN = 0x200000;
static constexpr u32 SHF_EXCLUDE = 0x80000000;

static constexpr u32 GRP_COMDAT = 1;

static constexpr u32 STT_NOTYPE = 0;
static constexpr u32 STT_OBJECT = 1;
static constexpr u32 STT_FUNC = 2;
static constexpr u32 STT_SECTION = 3;
static constexpr u32 STT_FILE = 4;
static constexpr u32 STT_COMMON = 5;
static constexpr u32 STT_TLS = 6;
static constexpr u32 STT_GNU_IFUNC = 10;

static constexpr u32 STB_LOCAL = 0;
static constexpr u32 STB_GLOBAL = 1;
static constexpr u32 STB_WEAK = 2;
static constexpr u32 STB_GNU_UNIQUE = 10;

static constexpr u32 STV_DEFAULT = 0;
static constexpr u32 STV_INTERNAL = 1;
static constexpr u32 STV_HIDDEN = 2;
static constexpr u32 STV_PROTECTED = 3;

static constexpr u32 VER_NDX_LOCAL = 0;
static constexpr u32 VER_NDX_GLOBAL = 1;
static constexpr u32 VER_NDX_LAST_RESERVED = 1;

static constexpr u32 VER_FLG_BASE = 1;
static constexpr u32 VER_FLG_WEAK = 2;
static constexpr u32 VER_FLG_INFO = 4;

static constexpr u32 VERSYM_HIDDEN = 0x8000;

static constexpr u32 PT_NULL = 0;
static constexpr u32 PT_LOAD = 1;
static constexpr u32 PT_DYNAMIC = 2;
static constexpr u32 PT_INTERP = 3;
static constexpr u32 PT_NOTE = 4;
static constexpr u32 PT_SHLIB = 5;
static constexpr u32 PT_PHDR = 6;
static constexpr u32 PT_TLS = 7;
static constexpr u32 PT_GNU_EH_FRAME = 0x6474e550;
static constexpr u32 PT_GNU_STACK = 0x6474e551;
static constexpr u32 PT_GNU_RELRO = 0x6474e552;
static constexpr u32 PT_ARM_EXIDX = 0x70000001;

static constexpr u32 PF_X = 1;
static constexpr u32 PF_W = 2;
static constexpr u32 PF_R = 4;

static constexpr u32 ET_NONE = 0;
static constexpr u32 ET_REL = 1;
static constexpr u32 ET_EXEC = 2;
static constexpr u32 ET_DYN = 3;

static constexpr u32 ELFDATA2LSB = 1;
static constexpr u32 ELFDATA2MSB = 2;

static constexpr u32 ELFCLASS32 = 1;
static constexpr u32 ELFCLASS64 = 2;

static constexpr u32 EV_CURRENT = 1;

static constexpr u32 EM_NONE = 0;
static constexpr u32 EM_386 = 3;
static constexpr u32 EM_ARM = 40;
static constexpr u32 EM_X86_64 = 62;
static constexpr u32 EM_AARCH64 = 183;
static constexpr u32 EM_RISCV = 243;

static constexpr u32 EI_CLASS = 4;
static constexpr u32 EI_DATA = 5;
static constexpr u32 EI_VERSION = 6;
static constexpr u32 EI_OSABI = 7;
static constexpr u32 EI_ABIVERSION = 8;

static constexpr u32 ELFOSABI_NONE = 0;
static constexpr u32 ELFOSABI_GNU = 3;

static constexpr u32 DT_NULL = 0;
static constexpr u32 DT_NEEDED = 1;
static constexpr u32 DT_PLTRELSZ = 2;
static constexpr u32 DT_PLTGOT = 3;
static constexpr u32 DT_HASH = 4;
static constexpr u32 DT_STRTAB = 5;
static constexpr u32 DT_SYMTAB = 6;
static constexpr u32 DT_RELA = 7;
static constexpr u32 DT_RELASZ = 8;
static constexpr u32 DT_RELAENT = 9;
static constexpr u32 DT_STRSZ = 10;
static constexpr u32 DT_SYMENT = 11;
static constexpr u32 DT_INIT = 12;
static constexpr u32 DT_FINI = 13;
static constexpr u32 DT_SONAME = 14;
static constexpr u32 DT_RPATH = 15;
static constexpr u32 DT_SYMBOLIC = 16;
static constexpr u32 DT_REL = 17;
static constexpr u32 DT_RELSZ = 18;
static constexpr u32 DT_RELENT = 19;
static constexpr u32 DT_PLTREL = 20;
static constexpr u32 DT_DEBUG = 21;
static constexpr u32 DT_TEXTREL = 22;
static constexpr u32 DT_JMPREL = 23;
static constexpr u32 DT_BIND_NOW = 24;
static constexpr u32 DT_INIT_ARRAY = 25;
static constexpr u32 DT_FINI_ARRAY = 26;
static constexpr u32 DT_INIT_ARRAYSZ = 27;
static constexpr u32 DT_FINI_ARRAYSZ = 28;
static constexpr u32 DT_RUNPATH = 29;
static constexpr u32 DT_FLAGS = 30;
static constexpr u32 DT_PREINIT_ARRAY = 32;
static constexpr u32 DT_PREINIT_ARRAYSZ = 33;
static constexpr u32 DT_RELRSZ = 35;
static constexpr u32 DT_RELR = 36;
static constexpr u32 DT_RELRENT = 37;
static constexpr u32 DT_GNU_HASH = 0x6ffffef5;
static constexpr u32 DT_VERSYM = 0x6ffffff0;
static constexpr u32 DT_RELACOUNT = 0x6ffffff9;
static constexpr u32 DT_RELCOUNT = 0x6ffffffa;
static constexpr u32 DT_FLAGS_1 = 0x6ffffffb;
static constexpr u32 DT_VERDEF = 0x6ffffffc;
static constexpr u32 DT_VERDEFNUM = 0x6ffffffd;
static constexpr u32 DT_VERNEED = 0x6ffffffe;
static constexpr u32 DT_VERNEEDNUM = 0x6fffffff;
static constexpr u32 DT_AUXILIARY = 0x7ffffffd;
static constexpr u32 DT_FILTER = 0x7fffffff;

static constexpr u32 DF_ORIGIN = 0x01;
static constexpr u32 DF_SYMBOLIC = 0x02;
static constexpr u32 DF_TEXTREL = 0x04;
static constexpr u32 DF_BIND_NOW = 0x08;
static constexpr u32 DF_STATIC_TLS = 0x10;

static constexpr u32 DF_1_NOW = 0x00000001;
static constexpr u32 DF_1_NODELETE = 0x00000008;
static constexpr u32 DF_1_INITFIRST = 0x00000020;
static constexpr u32 DF_1_NOOPEN = 0x00000040;
static constexpr u32 DF_1_ORIGIN = 0x00000080;
static constexpr u32 DF_1_INTERPOSE = 0x00000400;
static constexpr u32 DF_1_NODEFLIB = 0x00000800;
static constexpr u32 DF_1_NODUMP = 0x00001000;
static constexpr u32 DF_1_PIE = 0x08000000;

static constexpr u32 NT_GNU_ABI_TAG = 1;
static constexpr u32 NT_GNU_HWCAP = 2;
static constexpr u32 NT_GNU_BUILD_ID = 3;
static constexpr u32 NT_GNU_GOLD_VERSION = 4;
static constexpr u32 NT_GNU_PROPERTY_TYPE_0 = 5;
static constexpr u32 NT_FDO_PACKAGING_METADATA = 0xcafe1a7e;

static constexpr u32 GNU_PROPERTY_AARCH64_FEATURE_1_AND = 0xc0000000;
static constexpr u32 GNU_PROPERTY_X86_FEATURE_1_AND = 0xc0000002;

static constexpr u32 GNU_PROPERTY_X86_FEATURE_1_IBT = 1;
static constexpr u32 GNU_PROPERTY_X86_FEATURE_1_SHSTK = 2;

static constexpr u32 ELFCOMPRESS_ZLIB = 1;

static constexpr u32 EF_ARM_ABI_FLOAT_SOFT = 0x00000200;
static constexpr u32 EF_ARM_ABI_FLOAT_HARD = 0x00000400;
static constexpr u32 EF_ARM_EABI_VER5 = 0x05000000;

static constexpr u32 EF_RISCV_RVC = 1;
static constexpr u32 EF_RISCV_FLOAT_ABI = 6;
static constexpr u32 EF_RISCV_FLOAT_ABI_SOFT = 0;
static constexpr u32 EF_RISCV_FLOAT_ABI_SINGLE = 2;
static constexpr u32 EF_RISCV_FLOAT_ABI_DOUBLE = 4;
static constexpr u32 EF_RISCV_FLOAT_ABI_QUAD = 6;
static constexpr u32 EF_RISCV_RVE = 8;
static constexpr u32 EF_RISCV_TSO = 16;

static constexpr u32 STO_RISCV_VARIANT_CC = 0x80;

static constexpr u32 R_X86_64_NONE = 0;
static constexpr u32 R_X86_64_64 = 1;
static constexpr u32 R_X86_64_PC32 = 2;
static constexpr u32 R_X86_64_GOT32 = 3;
static constexpr u32 R_X86_64_PLT32 = 4;
static constexpr u32 R_X86_64_COPY = 5;
static constexpr u32 R_X86_64_GLOB_DAT = 6;
static constexpr u32 R_X86_64_JUMP_SLOT = 7;
static constexpr u32 R_X86_64_RELATIVE = 8;
static constexpr u32 R_X86_64_GOTPCREL = 9;
static constexpr u32 R_X86_64_32 = 10;
static constexpr u32 R_X86_64_32S = 11;
static constexpr u32 R_X86_64_16 = 12;
static constexpr u32 R_X86_64_PC16 = 13;
static constexpr u32 R_X86_64_8 = 14;
static constexpr u32 R_X86_64_PC8 = 15;
static constexpr u32 R_X86_64_DTPMOD64 = 16;
static constexpr u32 R_X86_64_DTPOFF64 = 17;
static constexpr u32 R_X86_64_TPOFF64 = 18;
static constexpr u32 R_X86_64_TLSGD = 19;
static constexpr u32 R_X86_64_TLSLD = 20;
static constexpr u32 R_X86_64_DTPOFF32 = 21;
static constexpr u32 R_X86_64_GOTTPOFF = 22;
static constexpr u32 R_X86_64_TPOFF32 = 23;
static constexpr u32 R_X86_64_PC64 = 24;
static constexpr u32 R_X86_64_GOTOFF64 = 25;
static constexpr u32 R_X86_64_GOTPC32 = 26;
static constexpr u32 R_X86_64_GOT64 = 27;
static constexpr u32 R_X86_64_GOTPCREL64 = 28;
static constexpr u32 R_X86_64_GOTPC64 = 29;
static constexpr u32 R_X86_64_GOTPLT64 = 30;
static constexpr u32 R_X86_64_PLTOFF64 = 31;
static constexpr u32 R_X86_64_SIZE32 = 32;
static constexpr u32 R_X86_64_SIZE64 = 33;
static constexpr u32 R_X86_64_GOTPC32_TLSDESC = 34;
static constexpr u32 R_X86_64_TLSDESC_CALL = 35;
static constexpr u32 R_X86_64_TLSDESC = 36;
static constexpr u32 R_X86_64_IRELATIVE = 37;
static constexpr u32 R_X86_64_GOTPCRELX = 41;
static constexpr u32 R_X86_64_REX_GOTPCRELX = 42;

template <>
inline std::string rel_to_string<X86_64>(u32 r_type) {
  switch (r_type) {
  case R_X86_64_NONE: return "R_X86_64_NONE";
  case R_X86_64_64: return "R_X86_64_64";
  case R_X86_64_PC32: return "R_X86_64_PC32";
  case R_X86_64_GOT32: return "R_X86_64_GOT32";
  case R_X86_64_PLT32: return "R_X86_64_PLT32";
  case R_X86_64_COPY: return "R_X86_64_COPY";
  case R_X86_64_GLOB_DAT: return "R_X86_64_GLOB_DAT";
  case R_X86_64_JUMP_SLOT: return "R_X86_64_JUMP_SLOT";
  case R_X86_64_RELATIVE: return "R_X86_64_RELATIVE";
  case R_X86_64_GOTPCREL: return "R_X86_64_GOTPCREL";
  case R_X86_64_32: return "R_X86_64_32";
  case R_X86_64_32S: return "R_X86_64_32S";
  case R_X86_64_16: return "R_X86_64_16";
  case R_X86_64_PC16: return "R_X86_64_PC16";
  case R_X86_64_8: return "R_X86_64_8";
  case R_X86_64_PC8: return "R_X86_64_PC8";
  case R_X86_64_DTPMOD64: return "R_X86_64_DTPMOD64";
  case R_X86_64_DTPOFF64: return "R_X86_64_DTPOFF64";
  case R_X86_64_TPOFF64: return "R_X86_64_TPOFF64";
  case R_X86_64_TLSGD: return "R_X86_64_TLSGD";
  case R_X86_64_TLSLD: return "R_X86_64_TLSLD";
  case R_X86_64_DTPOFF32: return "R_X86_64_DTPOFF32";
  case R_X86_64_GOTTPOFF: return "R_X86_64_GOTTPOFF";
  case R_X86_64_TPOFF32: return "R_X86_64_TPOFF32";
  case R_X86_64_PC64: return "R_X86_64_PC64";
  case R_X86_64_GOTOFF64: return "R_X86_64_GOTOFF64";
  case R_X86_64_GOTPC32: return "R_X86_64_GOTPC32";
  case R_X86_64_GOT64: return "R_X86_64_GOT64";
  case R_X86_64_GOTPCREL64: return "R_X86_64_GOTPCREL64";
  case R_X86_64_GOTPC64: return "R_X86_64_GOTPC64";
  case R_X86_64_GOTPLT64: return "R_X86_64_GOTPLT64";
  case R_X86_64_PLTOFF64: return "R_X86_64_PLTOFF64";
  case R_X86_64_SIZE32: return "R_X86_64_SIZE32";
  case R_X86_64_SIZE64: return "R_X86_64_SIZE64";
  case R_X86_64_GOTPC32_TLSDESC: return "R_X86_64_GOTPC32_TLSDESC";
  case R_X86_64_TLSDESC_CALL: return "R_X86_64_TLSDESC_CALL";
  case R_X86_64_TLSDESC: return "R_X86_64_TLSDESC";
  case R_X86_64_IRELATIVE: return "R_X86_64_IRELATIVE";
  case R_X86_64_GOTPCRELX: return "R_X86_64_GOTPCRELX";
  case R_X86_64_REX_GOTPCRELX: return "R_X86_64_REX_GOTPCRELX";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

static constexpr u32 R_386_NONE = 0;
static constexpr u32 R_386_32 = 1;
static constexpr u32 R_386_PC32 = 2;
static constexpr u32 R_386_GOT32 = 3;
static constexpr u32 R_386_PLT32 = 4;
static constexpr u32 R_386_COPY = 5;
static constexpr u32 R_386_GLOB_DAT = 6;
static constexpr u32 R_386_JUMP_SLOT = 7;
static constexpr u32 R_386_RELATIVE = 8;
static constexpr u32 R_386_GOTOFF = 9;
static constexpr u32 R_386_GOTPC = 10;
static constexpr u32 R_386_32PLT = 11;
static constexpr u32 R_386_TLS_TPOFF = 14;
static constexpr u32 R_386_TLS_IE = 15;
static constexpr u32 R_386_TLS_GOTIE = 16;
static constexpr u32 R_386_TLS_LE = 17;
static constexpr u32 R_386_TLS_GD = 18;
static constexpr u32 R_386_TLS_LDM = 19;
static constexpr u32 R_386_16 = 20;
static constexpr u32 R_386_PC16 = 21;
static constexpr u32 R_386_8 = 22;
static constexpr u32 R_386_PC8 = 23;
static constexpr u32 R_386_TLS_GD_32 = 24;
static constexpr u32 R_386_TLS_GD_PUSH = 25;
static constexpr u32 R_386_TLS_GD_CALL = 26;
static constexpr u32 R_386_TLS_GD_POP = 27;
static constexpr u32 R_386_TLS_LDM_32 = 28;
static constexpr u32 R_386_TLS_LDM_PUSH = 29;
static constexpr u32 R_386_TLS_LDM_CALL = 30;
static constexpr u32 R_386_TLS_LDM_POP = 31;
static constexpr u32 R_386_TLS_LDO_32 = 32;
static constexpr u32 R_386_TLS_IE_32 = 33;
static constexpr u32 R_386_TLS_LE_32 = 34;
static constexpr u32 R_386_TLS_DTPMOD32 = 35;
static constexpr u32 R_386_TLS_DTPOFF32 = 36;
static constexpr u32 R_386_TLS_TPOFF32 = 37;
static constexpr u32 R_386_SIZE32 = 38;
static constexpr u32 R_386_TLS_GOTDESC = 39;
static constexpr u32 R_386_TLS_DESC_CALL = 40;
static constexpr u32 R_386_TLS_DESC = 41;
static constexpr u32 R_386_IRELATIVE = 42;
static constexpr u32 R_386_GOT32X = 43;

template <>
inline std::string rel_to_string<I386>(u32 r_type) {
  switch (r_type) {
  case R_386_NONE: return "R_386_NONE";
  case R_386_32: return "R_386_32";
  case R_386_PC32: return "R_386_PC32";
  case R_386_GOT32: return "R_386_GOT32";
  case R_386_PLT32: return "R_386_PLT32";
  case R_386_COPY: return "R_386_COPY";
  case R_386_GLOB_DAT: return "R_386_GLOB_DAT";
  case R_386_JUMP_SLOT: return "R_386_JUMP_SLOT";
  case R_386_RELATIVE: return "R_386_RELATIVE";
  case R_386_GOTOFF: return "R_386_GOTOFF";
  case R_386_GOTPC: return "R_386_GOTPC";
  case R_386_32PLT: return "R_386_32PLT";
  case R_386_TLS_TPOFF: return "R_386_TLS_TPOFF";
  case R_386_TLS_IE: return "R_386_TLS_IE";
  case R_386_TLS_GOTIE: return "R_386_TLS_GOTIE";
  case R_386_TLS_LE: return "R_386_TLS_LE";
  case R_386_TLS_GD: return "R_386_TLS_GD";
  case R_386_TLS_LDM: return "R_386_TLS_LDM";
  case R_386_16: return "R_386_16";
  case R_386_PC16: return "R_386_PC16";
  case R_386_8: return "R_386_8";
  case R_386_PC8: return "R_386_PC8";
  case R_386_TLS_GD_32: return "R_386_TLS_GD_32";
  case R_386_TLS_GD_PUSH: return "R_386_TLS_GD_PUSH";
  case R_386_TLS_GD_CALL: return "R_386_TLS_GD_CALL";
  case R_386_TLS_GD_POP: return "R_386_TLS_GD_POP";
  case R_386_TLS_LDM_32: return "R_386_TLS_LDM_32";
  case R_386_TLS_LDM_PUSH: return "R_386_TLS_LDM_PUSH";
  case R_386_TLS_LDM_CALL: return "R_386_TLS_LDM_CALL";
  case R_386_TLS_LDM_POP: return "R_386_TLS_LDM_POP";
  case R_386_TLS_LDO_32: return "R_386_TLS_LDO_32";
  case R_386_TLS_IE_32: return "R_386_TLS_IE_32";
  case R_386_TLS_LE_32: return "R_386_TLS_LE_32";
  case R_386_TLS_DTPMOD32: return "R_386_TLS_DTPMOD32";
  case R_386_TLS_DTPOFF32: return "R_386_TLS_DTPOFF32";
  case R_386_TLS_TPOFF32: return "R_386_TLS_TPOFF32";
  case R_386_SIZE32: return "R_386_SIZE32";
  case R_386_TLS_GOTDESC: return "R_386_TLS_GOTDESC";
  case R_386_TLS_DESC_CALL: return "R_386_TLS_DESC_CALL";
  case R_386_TLS_DESC: return "R_386_TLS_DESC";
  case R_386_IRELATIVE: return "R_386_IRELATIVE";
  case R_386_GOT32X: return "R_386_GOT32X";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

static constexpr u32 R_AARCH64_NONE = 0;
static constexpr u32 R_AARCH64_ABS64 = 0x101;
static constexpr u32 R_AARCH64_ABS32 = 0x102;
static constexpr u32 R_AARCH64_ABS16 = 0x103;
static constexpr u32 R_AARCH64_PREL64 = 0x104;
static constexpr u32 R_AARCH64_PREL32 = 0x105;
static constexpr u32 R_AARCH64_PREL16 = 0x106;
static constexpr u32 R_AARCH64_MOVW_UABS_G0 = 0x107;
static constexpr u32 R_AARCH64_MOVW_UABS_G0_NC = 0x108;
static constexpr u32 R_AARCH64_MOVW_UABS_G1 = 0x109;
static constexpr u32 R_AARCH64_MOVW_UABS_G1_NC = 0x10a;
static constexpr u32 R_AARCH64_MOVW_UABS_G2 = 0x10b;
static constexpr u32 R_AARCH64_MOVW_UABS_G2_NC = 0x10c;
static constexpr u32 R_AARCH64_MOVW_UABS_G3 = 0x10d;
static constexpr u32 R_AARCH64_MOVW_SABS_G0 = 0x10e;
static constexpr u32 R_AARCH64_MOVW_SABS_G1 = 0x10f;
static constexpr u32 R_AARCH64_MOVW_SABS_G2 = 0x110;
static constexpr u32 R_AARCH64_LD_PREL_LO19 = 0x111;
static constexpr u32 R_AARCH64_ADR_PREL_LO21 = 0x112;
static constexpr u32 R_AARCH64_ADR_PREL_PG_HI21 = 0x113;
static constexpr u32 R_AARCH64_ADR_PREL_PG_HI21_NC = 0x114;
static constexpr u32 R_AARCH64_ADD_ABS_LO12_NC = 0x115;
static constexpr u32 R_AARCH64_LDST8_ABS_LO12_NC = 0x116;
static constexpr u32 R_AARCH64_TSTBR14 = 0x117;
static constexpr u32 R_AARCH64_CONDBR19 = 0x118;
static constexpr u32 R_AARCH64_JUMP26 = 0x11a;
static constexpr u32 R_AARCH64_CALL26 = 0x11b;
static constexpr u32 R_AARCH64_LDST16_ABS_LO12_NC = 0x11c;
static constexpr u32 R_AARCH64_LDST32_ABS_LO12_NC = 0x11d;
static constexpr u32 R_AARCH64_LDST64_ABS_LO12_NC = 0x11e;
static constexpr u32 R_AARCH64_MOVW_PREL_G0 = 0x11f;
static constexpr u32 R_AARCH64_MOVW_PREL_G0_NC = 0x120;
static constexpr u32 R_AARCH64_MOVW_PREL_G1 = 0x121;
static constexpr u32 R_AARCH64_MOVW_PREL_G1_NC = 0x122;
static constexpr u32 R_AARCH64_MOVW_PREL_G2 = 0x123;
static constexpr u32 R_AARCH64_MOVW_PREL_G2_NC = 0x124;
static constexpr u32 R_AARCH64_MOVW_PREL_G3 = 0x125;
static constexpr u32 R_AARCH64_LDST128_ABS_LO12_NC = 0x12b;
static constexpr u32 R_AARCH64_ADR_GOT_PAGE = 0x137;
static constexpr u32 R_AARCH64_LD64_GOT_LO12_NC = 0x138;
static constexpr u32 R_AARCH64_LD64_GOTPAGE_LO15 = 0x139;
static constexpr u32 R_AARCH64_PLT32 = 0x13a;
static constexpr u32 R_AARCH64_TLSGD_ADR_PREL21 = 0x200;
static constexpr u32 R_AARCH64_TLSGD_ADR_PAGE21 = 0x201;
static constexpr u32 R_AARCH64_TLSGD_ADD_LO12_NC = 0x202;
static constexpr u32 R_AARCH64_TLSGD_MOVW_G1 = 0x203;
static constexpr u32 R_AARCH64_TLSGD_MOVW_G0_NC = 0x204;
static constexpr u32 R_AARCH64_TLSLD_ADR_PREL21 = 0x205;
static constexpr u32 R_AARCH64_TLSLD_ADR_PAGE21 = 0x206;
static constexpr u32 R_AARCH64_TLSLD_ADD_LO12_NC = 0x207;
static constexpr u32 R_AARCH64_TLSLD_MOVW_G1 = 0x208;
static constexpr u32 R_AARCH64_TLSLD_MOVW_G0_NC = 0x209;
static constexpr u32 R_AARCH64_TLSLD_LD_PREL19 = 0x20a;
static constexpr u32 R_AARCH64_TLSLD_MOVW_DTPREL_G2 = 0x20b;
static constexpr u32 R_AARCH64_TLSLD_MOVW_DTPREL_G1 = 0x20c;
static constexpr u32 R_AARCH64_TLSLD_MOVW_DTPREL_G1_NC = 0x20d;
static constexpr u32 R_AARCH64_TLSLD_MOVW_DTPREL_G0 = 0x20e;
static constexpr u32 R_AARCH64_TLSLD_MOVW_DTPREL_G0_NC = 0x20f;
static constexpr u32 R_AARCH64_TLSLD_ADD_DTPREL_HI12 = 0x210;
static constexpr u32 R_AARCH64_TLSLD_ADD_DTPREL_LO12 = 0x211;
static constexpr u32 R_AARCH64_TLSLD_ADD_DTPREL_LO12_NC = 0x212;
static constexpr u32 R_AARCH64_TLSLD_LDST8_DTPREL_LO12 = 0x213;
static constexpr u32 R_AARCH64_TLSLD_LDST8_DTPREL_LO12_NC = 0x214;
static constexpr u32 R_AARCH64_TLSLD_LDST16_DTPREL_LO12 = 0x215;
static constexpr u32 R_AARCH64_TLSLD_LDST16_DTPREL_LO12_NC = 0x216;
static constexpr u32 R_AARCH64_TLSLD_LDST32_DTPREL_LO12 = 0x217;
static constexpr u32 R_AARCH64_TLSLD_LDST32_DTPREL_LO12_NC = 0x218;
static constexpr u32 R_AARCH64_TLSLD_LDST64_DTPREL_LO12 = 0x219;
static constexpr u32 R_AARCH64_TLSLD_LDST64_DTPREL_LO12_NC = 0x21a;
static constexpr u32 R_AARCH64_TLSIE_MOVW_GOTTPREL_G1 = 0x21b;
static constexpr u32 R_AARCH64_TLSIE_MOVW_GOTTPREL_G0_NC = 0x21c;
static constexpr u32 R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21 = 0x21d;
static constexpr u32 R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC = 0x21e;
static constexpr u32 R_AARCH64_TLSIE_LD_GOTTPREL_PREL19 = 0x21f;
static constexpr u32 R_AARCH64_TLSLE_MOVW_TPREL_G2 = 0x220;
static constexpr u32 R_AARCH64_TLSLE_MOVW_TPREL_G1 = 0x221;
static constexpr u32 R_AARCH64_TLSLE_MOVW_TPREL_G1_NC = 0x222;
static constexpr u32 R_AARCH64_TLSLE_MOVW_TPREL_G0 = 0x223;
static constexpr u32 R_AARCH64_TLSLE_MOVW_TPREL_G0_NC = 0x224;
static constexpr u32 R_AARCH64_TLSLE_ADD_TPREL_HI12 = 0x225;
static constexpr u32 R_AARCH64_TLSLE_ADD_TPREL_LO12 = 0x226;
static constexpr u32 R_AARCH64_TLSLE_ADD_TPREL_LO12_NC = 0x227;
static constexpr u32 R_AARCH64_TLSLE_LDST8_TPREL_LO12 = 0x228;
static constexpr u32 R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC = 0x229;
static constexpr u32 R_AARCH64_TLSLE_LDST16_TPREL_LO12 = 0x22a;
static constexpr u32 R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC = 0x22b;
static constexpr u32 R_AARCH64_TLSLE_LDST32_TPREL_LO12 = 0x22c;
static constexpr u32 R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC = 0x22d;
static constexpr u32 R_AARCH64_TLSLE_LDST64_TPREL_LO12 = 0x22e;
static constexpr u32 R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC = 0x22f;
static constexpr u32 R_AARCH64_TLSDESC_ADR_PAGE21 = 0x232;
static constexpr u32 R_AARCH64_TLSDESC_LD64_LO12 = 0x233;
static constexpr u32 R_AARCH64_TLSDESC_ADD_LO12 = 0x234;
static constexpr u32 R_AARCH64_TLSDESC_CALL = 0x239;
static constexpr u32 R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC = 0x23b;
static constexpr u32 R_AARCH64_COPY = 0x400;
static constexpr u32 R_AARCH64_GLOB_DAT = 0x401;
static constexpr u32 R_AARCH64_JUMP_SLOT = 0x402;
static constexpr u32 R_AARCH64_RELATIVE = 0x403;
static constexpr u32 R_AARCH64_TLS_DTPMOD64 = 0x404;
static constexpr u32 R_AARCH64_TLS_DTPREL64 = 0x405;
static constexpr u32 R_AARCH64_TLS_TPREL64 = 0x406;
static constexpr u32 R_AARCH64_TLSDESC = 0x407;
static constexpr u32 R_AARCH64_IRELATIVE = 0x408;

template <>
inline std::string rel_to_string<ARM64>(u32 r_type) {
  switch (r_type) {
  case R_AARCH64_NONE: return "R_AARCH64_NONE";
  case R_AARCH64_ABS64: return "R_AARCH64_ABS64";
  case R_AARCH64_ABS32: return "R_AARCH64_ABS32";
  case R_AARCH64_ABS16: return "R_AARCH64_ABS16";
  case R_AARCH64_PREL64: return "R_AARCH64_PREL64";
  case R_AARCH64_PREL32: return "R_AARCH64_PREL32";
  case R_AARCH64_PREL16: return "R_AARCH64_PREL16";
  case R_AARCH64_MOVW_UABS_G0: return "R_AARCH64_MOVW_UABS_G0";
  case R_AARCH64_MOVW_UABS_G0_NC: return "R_AARCH64_MOVW_UABS_G0_NC";
  case R_AARCH64_MOVW_UABS_G1: return "R_AARCH64_MOVW_UABS_G1";
  case R_AARCH64_MOVW_UABS_G1_NC: return "R_AARCH64_MOVW_UABS_G1_NC";
  case R_AARCH64_MOVW_UABS_G2: return "R_AARCH64_MOVW_UABS_G2";
  case R_AARCH64_MOVW_UABS_G2_NC: return "R_AARCH64_MOVW_UABS_G2_NC";
  case R_AARCH64_MOVW_UABS_G3: return "R_AARCH64_MOVW_UABS_G3";
  case R_AARCH64_MOVW_SABS_G0: return "R_AARCH64_MOVW_SABS_G0";
  case R_AARCH64_MOVW_SABS_G1: return "R_AARCH64_MOVW_SABS_G1";
  case R_AARCH64_MOVW_SABS_G2: return "R_AARCH64_MOVW_SABS_G2";
  case R_AARCH64_LD_PREL_LO19: return "R_AARCH64_LD_PREL_LO19";
  case R_AARCH64_ADR_PREL_LO21: return "R_AARCH64_ADR_PREL_LO21";
  case R_AARCH64_ADR_PREL_PG_HI21: return "R_AARCH64_ADR_PREL_PG_HI21";
  case R_AARCH64_ADR_PREL_PG_HI21_NC: return "R_AARCH64_ADR_PREL_PG_HI21_NC";
  case R_AARCH64_ADD_ABS_LO12_NC: return "R_AARCH64_ADD_ABS_LO12_NC";
  case R_AARCH64_LDST8_ABS_LO12_NC: return "R_AARCH64_LDST8_ABS_LO12_NC";
  case R_AARCH64_TSTBR14: return "R_AARCH64_TSTBR14";
  case R_AARCH64_CONDBR19: return "R_AARCH64_CONDBR19";
  case R_AARCH64_JUMP26: return "R_AARCH64_JUMP26";
  case R_AARCH64_CALL26: return "R_AARCH64_CALL26";
  case R_AARCH64_LDST16_ABS_LO12_NC: return "R_AARCH64_LDST16_ABS_LO12_NC";
  case R_AARCH64_LDST32_ABS_LO12_NC: return "R_AARCH64_LDST32_ABS_LO12_NC";
  case R_AARCH64_LDST64_ABS_LO12_NC: return "R_AARCH64_LDST64_ABS_LO12_NC";
  case R_AARCH64_MOVW_PREL_G0: return "R_AARCH64_MOVW_PREL_G0";
  case R_AARCH64_MOVW_PREL_G0_NC: return "R_AARCH64_MOVW_PREL_G0_NC";
  case R_AARCH64_MOVW_PREL_G1: return "R_AARCH64_MOVW_PREL_G1";
  case R_AARCH64_MOVW_PREL_G1_NC: return "R_AARCH64_MOVW_PREL_G1_NC";
  case R_AARCH64_MOVW_PREL_G2: return "R_AARCH64_MOVW_PREL_G2";
  case R_AARCH64_MOVW_PREL_G2_NC: return "R_AARCH64_MOVW_PREL_G2_NC";
  case R_AARCH64_MOVW_PREL_G3: return "R_AARCH64_MOVW_PREL_G3";
  case R_AARCH64_LDST128_ABS_LO12_NC: return "R_AARCH64_LDST128_ABS_LO12_NC";
  case R_AARCH64_ADR_GOT_PAGE: return "R_AARCH64_ADR_GOT_PAGE";
  case R_AARCH64_LD64_GOT_LO12_NC: return "R_AARCH64_LD64_GOT_LO12_NC";
  case R_AARCH64_LD64_GOTPAGE_LO15: return "R_AARCH64_LD64_GOTPAGE_LO15";
  case R_AARCH64_PLT32: return "R_AARCH64_PLT32";
  case R_AARCH64_TLSGD_ADR_PREL21: return "R_AARCH64_TLSGD_ADR_PREL21";
  case R_AARCH64_TLSGD_ADR_PAGE21: return "R_AARCH64_TLSGD_ADR_PAGE21";
  case R_AARCH64_TLSGD_ADD_LO12_NC: return "R_AARCH64_TLSGD_ADD_LO12_NC";
  case R_AARCH64_TLSGD_MOVW_G1: return "R_AARCH64_TLSGD_MOVW_G1";
  case R_AARCH64_TLSGD_MOVW_G0_NC: return "R_AARCH64_TLSGD_MOVW_G0_NC";
  case R_AARCH64_TLSLD_ADR_PREL21: return "R_AARCH64_TLSLD_ADR_PREL21";
  case R_AARCH64_TLSLD_ADR_PAGE21: return "R_AARCH64_TLSLD_ADR_PAGE21";
  case R_AARCH64_TLSLD_ADD_LO12_NC: return "R_AARCH64_TLSLD_ADD_LO12_NC";
  case R_AARCH64_TLSLD_MOVW_G1: return "R_AARCH64_TLSLD_MOVW_G1";
  case R_AARCH64_TLSLD_MOVW_G0_NC: return "R_AARCH64_TLSLD_MOVW_G0_NC";
  case R_AARCH64_TLSLD_LD_PREL19: return "R_AARCH64_TLSLD_LD_PREL19";
  case R_AARCH64_TLSLD_MOVW_DTPREL_G2: return "R_AARCH64_TLSLD_MOVW_DTPREL_G2";
  case R_AARCH64_TLSLD_MOVW_DTPREL_G1: return "R_AARCH64_TLSLD_MOVW_DTPREL_G1";
  case R_AARCH64_TLSLD_MOVW_DTPREL_G1_NC: return "R_AARCH64_TLSLD_MOVW_DTPREL_G1_NC";
  case R_AARCH64_TLSLD_MOVW_DTPREL_G0: return "R_AARCH64_TLSLD_MOVW_DTPREL_G0";
  case R_AARCH64_TLSLD_MOVW_DTPREL_G0_NC: return "R_AARCH64_TLSLD_MOVW_DTPREL_G0_NC";
  case R_AARCH64_TLSLD_ADD_DTPREL_HI12: return "R_AARCH64_TLSLD_ADD_DTPREL_HI12";
  case R_AARCH64_TLSLD_ADD_DTPREL_LO12: return "R_AARCH64_TLSLD_ADD_DTPREL_LO12";
  case R_AARCH64_TLSLD_ADD_DTPREL_LO12_NC: return "R_AARCH64_TLSLD_ADD_DTPREL_LO12_NC";
  case R_AARCH64_TLSLD_LDST8_DTPREL_LO12: return "R_AARCH64_TLSLD_LDST8_DTPREL_LO12";
  case R_AARCH64_TLSLD_LDST8_DTPREL_LO12_NC: return "R_AARCH64_TLSLD_LDST8_DTPREL_LO12_NC";
  case R_AARCH64_TLSLD_LDST16_DTPREL_LO12: return "R_AARCH64_TLSLD_LDST16_DTPREL_LO12";
  case R_AARCH64_TLSLD_LDST16_DTPREL_LO12_NC: return "R_AARCH64_TLSLD_LDST16_DTPREL_LO12_NC";
  case R_AARCH64_TLSLD_LDST32_DTPREL_LO12: return "R_AARCH64_TLSLD_LDST32_DTPREL_LO12";
  case R_AARCH64_TLSLD_LDST32_DTPREL_LO12_NC: return "R_AARCH64_TLSLD_LDST32_DTPREL_LO12_NC";
  case R_AARCH64_TLSLD_LDST64_DTPREL_LO12: return "R_AARCH64_TLSLD_LDST64_DTPREL_LO12";
  case R_AARCH64_TLSLD_LDST64_DTPREL_LO12_NC: return "R_AARCH64_TLSLD_LDST64_DTPREL_LO12_NC";
  case R_AARCH64_TLSIE_MOVW_GOTTPREL_G1: return "R_AARCH64_TLSIE_MOVW_GOTTPREL_G1";
  case R_AARCH64_TLSIE_MOVW_GOTTPREL_G0_NC: return "R_AARCH64_TLSIE_MOVW_GOTTPREL_G0_NC";
  case R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21: return "R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21";
  case R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC: return "R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC";
  case R_AARCH64_TLSIE_LD_GOTTPREL_PREL19: return "R_AARCH64_TLSIE_LD_GOTTPREL_PREL19";
  case R_AARCH64_TLSLE_MOVW_TPREL_G2: return "R_AARCH64_TLSLE_MOVW_TPREL_G2";
  case R_AARCH64_TLSLE_MOVW_TPREL_G1: return "R_AARCH64_TLSLE_MOVW_TPREL_G1";
  case R_AARCH64_TLSLE_MOVW_TPREL_G1_NC: return "R_AARCH64_TLSLE_MOVW_TPREL_G1_NC";
  case R_AARCH64_TLSLE_MOVW_TPREL_G0: return "R_AARCH64_TLSLE_MOVW_TPREL_G0";
  case R_AARCH64_TLSLE_MOVW_TPREL_G0_NC: return "R_AARCH64_TLSLE_MOVW_TPREL_G0_NC";
  case R_AARCH64_TLSLE_ADD_TPREL_HI12: return "R_AARCH64_TLSLE_ADD_TPREL_HI12";
  case R_AARCH64_TLSLE_ADD_TPREL_LO12: return "R_AARCH64_TLSLE_ADD_TPREL_LO12";
  case R_AARCH64_TLSLE_ADD_TPREL_LO12_NC: return "R_AARCH64_TLSLE_ADD_TPREL_LO12_NC";
  case R_AARCH64_TLSLE_LDST8_TPREL_LO12: return "R_AARCH64_TLSLE_LDST8_TPREL_LO12";
  case R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC: return "R_AARCH64_TLSLE_LDST8_TPREL_LO12_NC";
  case R_AARCH64_TLSLE_LDST16_TPREL_LO12: return "R_AARCH64_TLSLE_LDST16_TPREL_LO12";
  case R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC: return "R_AARCH64_TLSLE_LDST16_TPREL_LO12_NC";
  case R_AARCH64_TLSLE_LDST32_TPREL_LO12: return "R_AARCH64_TLSLE_LDST32_TPREL_LO12";
  case R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC: return "R_AARCH64_TLSLE_LDST32_TPREL_LO12_NC";
  case R_AARCH64_TLSLE_LDST64_TPREL_LO12: return "R_AARCH64_TLSLE_LDST64_TPREL_LO12";
  case R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC: return "R_AARCH64_TLSLE_LDST64_TPREL_LO12_NC";
  case R_AARCH64_TLSDESC_ADR_PAGE21: return "R_AARCH64_TLSDESC_ADR_PAGE21";
  case R_AARCH64_TLSDESC_LD64_LO12: return "R_AARCH64_TLSDESC_LD64_LO12";
  case R_AARCH64_TLSDESC_ADD_LO12: return "R_AARCH64_TLSDESC_ADD_LO12";
  case R_AARCH64_TLSDESC_CALL: return "R_AARCH64_TLSDESC_CALL";
  case R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC: return "R_AARCH64_TLSLE_LDST128_TPREL_LO12_NC";
  case R_AARCH64_COPY: return "R_AARCH64_COPY";
  case R_AARCH64_GLOB_DAT: return "R_AARCH64_GLOB_DAT";
  case R_AARCH64_JUMP_SLOT: return "R_AARCH64_JUMP_SLOT";
  case R_AARCH64_RELATIVE: return "R_AARCH64_RELATIVE";
  case R_AARCH64_TLS_DTPMOD64: return "R_AARCH64_TLS_DTPMOD64";
  case R_AARCH64_TLS_DTPREL64: return "R_AARCH64_TLS_DTPREL64";
  case R_AARCH64_TLS_TPREL64: return "R_AARCH64_TLS_TPREL64";
  case R_AARCH64_TLSDESC: return "R_AARCH64_TLSDESC";
  case R_AARCH64_IRELATIVE: return "R_AARCH64_IRELATIVE";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

static constexpr u32 R_ARM_NONE = 0x0;
static constexpr u32 R_ARM_PC24 = 0x1;
static constexpr u32 R_ARM_ABS32 = 0x2;
static constexpr u32 R_ARM_REL32 = 0x3;
static constexpr u32 R_ARM_LDR_PC_G0 = 0x4;
static constexpr u32 R_ARM_ABS16 = 0x5;
static constexpr u32 R_ARM_ABS12 = 0x6;
static constexpr u32 R_ARM_THM_ABS5 = 0x7;
static constexpr u32 R_ARM_ABS8 = 0x8;
static constexpr u32 R_ARM_SBREL32 = 0x9;
static constexpr u32 R_ARM_THM_CALL = 0xa;
static constexpr u32 R_ARM_THM_PC8 = 0xb;
static constexpr u32 R_ARM_BREL_ADJ = 0xc;
static constexpr u32 R_ARM_TLS_DESC = 0xd;
static constexpr u32 R_ARM_THM_SWI8 = 0xe;
static constexpr u32 R_ARM_XPC25 = 0xf;
static constexpr u32 R_ARM_THM_XPC22 = 0x10;
static constexpr u32 R_ARM_TLS_DTPMOD32 = 0x11;
static constexpr u32 R_ARM_TLS_DTPOFF32 = 0x12;
static constexpr u32 R_ARM_TLS_TPOFF32 = 0x13;
static constexpr u32 R_ARM_COPY = 0x14;
static constexpr u32 R_ARM_GLOB_DAT = 0x15;
static constexpr u32 R_ARM_JUMP_SLOT = 0x16;
static constexpr u32 R_ARM_RELATIVE = 0x17;
static constexpr u32 R_ARM_GOTOFF32 = 0x18;
static constexpr u32 R_ARM_BASE_PREL = 0x19;
static constexpr u32 R_ARM_GOT_BREL = 0x1a;
static constexpr u32 R_ARM_PLT32 = 0x1b;
static constexpr u32 R_ARM_CALL = 0x1c;
static constexpr u32 R_ARM_JUMP24 = 0x1d;
static constexpr u32 R_ARM_THM_JUMP24 = 0x1e;
static constexpr u32 R_ARM_BASE_ABS = 0x1f;
static constexpr u32 R_ARM_ALU_PCREL_7_0 = 0x20;
static constexpr u32 R_ARM_ALU_PCREL_15_8 = 0x21;
static constexpr u32 R_ARM_ALU_PCREL_23_15 = 0x22;
static constexpr u32 R_ARM_LDR_SBREL_11_0_NC = 0x23;
static constexpr u32 R_ARM_ALU_SBREL_19_12_NC = 0x24;
static constexpr u32 R_ARM_ALU_SBREL_27_20_CK = 0x25;
static constexpr u32 R_ARM_TARGET1 = 0x26;
static constexpr u32 R_ARM_SBREL31 = 0x27;
static constexpr u32 R_ARM_V4BX = 0x28;
static constexpr u32 R_ARM_TARGET2 = 0x29;
static constexpr u32 R_ARM_PREL31 = 0x2a;
static constexpr u32 R_ARM_MOVW_ABS_NC = 0x2b;
static constexpr u32 R_ARM_MOVT_ABS = 0x2c;
static constexpr u32 R_ARM_MOVW_PREL_NC = 0x2d;
static constexpr u32 R_ARM_MOVT_PREL = 0x2e;
static constexpr u32 R_ARM_THM_MOVW_ABS_NC = 0x2f;
static constexpr u32 R_ARM_THM_MOVT_ABS = 0x30;
static constexpr u32 R_ARM_THM_MOVW_PREL_NC = 0x31;
static constexpr u32 R_ARM_THM_MOVT_PREL = 0x32;
static constexpr u32 R_ARM_THM_JUMP19 = 0x33;
static constexpr u32 R_ARM_THM_JUMP6 = 0x34;
static constexpr u32 R_ARM_THM_ALU_PREL_11_0 = 0x35;
static constexpr u32 R_ARM_THM_PC12 = 0x36;
static constexpr u32 R_ARM_ABS32_NOI = 0x37;
static constexpr u32 R_ARM_REL32_NOI = 0x38;
static constexpr u32 R_ARM_ALU_PC_G0_NC = 0x39;
static constexpr u32 R_ARM_ALU_PC_G0 = 0x3a;
static constexpr u32 R_ARM_ALU_PC_G1_NC = 0x3b;
static constexpr u32 R_ARM_ALU_PC_G1 = 0x3c;
static constexpr u32 R_ARM_ALU_PC_G2 = 0x3d;
static constexpr u32 R_ARM_LDR_PC_G1 = 0x3e;
static constexpr u32 R_ARM_LDR_PC_G2 = 0x3f;
static constexpr u32 R_ARM_LDRS_PC_G0 = 0x40;
static constexpr u32 R_ARM_LDRS_PC_G1 = 0x41;
static constexpr u32 R_ARM_LDRS_PC_G2 = 0x42;
static constexpr u32 R_ARM_LDC_PC_G0 = 0x43;
static constexpr u32 R_ARM_LDC_PC_G1 = 0x44;
static constexpr u32 R_ARM_LDC_PC_G2 = 0x45;
static constexpr u32 R_ARM_ALU_SB_G0_NC = 0x46;
static constexpr u32 R_ARM_ALU_SB_G0 = 0x47;
static constexpr u32 R_ARM_ALU_SB_G1_NC = 0x48;
static constexpr u32 R_ARM_ALU_SB_G1 = 0x49;
static constexpr u32 R_ARM_ALU_SB_G2 = 0x4a;
static constexpr u32 R_ARM_LDR_SB_G0 = 0x4b;
static constexpr u32 R_ARM_LDR_SB_G1 = 0x4c;
static constexpr u32 R_ARM_LDR_SB_G2 = 0x4d;
static constexpr u32 R_ARM_LDRS_SB_G0 = 0x4e;
static constexpr u32 R_ARM_LDRS_SB_G1 = 0x4f;
static constexpr u32 R_ARM_LDRS_SB_G2 = 0x50;
static constexpr u32 R_ARM_LDC_SB_G0 = 0x51;
static constexpr u32 R_ARM_LDC_SB_G1 = 0x52;
static constexpr u32 R_ARM_LDC_SB_G2 = 0x53;
static constexpr u32 R_ARM_MOVW_BREL_NC = 0x54;
static constexpr u32 R_ARM_MOVT_BREL = 0x55;
static constexpr u32 R_ARM_MOVW_BREL = 0x56;
static constexpr u32 R_ARM_THM_MOVW_BREL_NC = 0x57;
static constexpr u32 R_ARM_THM_MOVT_BREL = 0x58;
static constexpr u32 R_ARM_THM_MOVW_BREL = 0x59;
static constexpr u32 R_ARM_TLS_GOTDESC = 0x5a;
static constexpr u32 R_ARM_TLS_CALL = 0x5b;
static constexpr u32 R_ARM_TLS_DESCSEQ = 0x5c;
static constexpr u32 R_ARM_THM_TLS_CALL = 0x5d;
static constexpr u32 R_ARM_PLT32_ABS = 0x5e;
static constexpr u32 R_ARM_GOT_ABS = 0x5f;
static constexpr u32 R_ARM_GOT_PREL = 0x60;
static constexpr u32 R_ARM_GOT_BREL12 = 0x61;
static constexpr u32 R_ARM_GOTOFF12 = 0x62;
static constexpr u32 R_ARM_GOTRELAX = 0x63;
static constexpr u32 R_ARM_GNU_VTENTRY = 0x64;
static constexpr u32 R_ARM_GNU_VTINHERIT = 0x65;
static constexpr u32 R_ARM_THM_JUMP11 = 0x66;
static constexpr u32 R_ARM_THM_JUMP8 = 0x67;
static constexpr u32 R_ARM_TLS_GD32 = 0x68;
static constexpr u32 R_ARM_TLS_LDM32 = 0x69;
static constexpr u32 R_ARM_TLS_LDO32 = 0x6a;
static constexpr u32 R_ARM_TLS_IE32 = 0x6b;
static constexpr u32 R_ARM_TLS_LE32 = 0x6c;
static constexpr u32 R_ARM_TLS_LDO12 = 0x6d;
static constexpr u32 R_ARM_TLS_LE12 = 0x6e;
static constexpr u32 R_ARM_TLS_IE12GP = 0x6f;
static constexpr u32 R_ARM_PRIVATE_0 = 0x70;
static constexpr u32 R_ARM_PRIVATE_1 = 0x71;
static constexpr u32 R_ARM_PRIVATE_2 = 0x72;
static constexpr u32 R_ARM_PRIVATE_3 = 0x73;
static constexpr u32 R_ARM_PRIVATE_4 = 0x74;
static constexpr u32 R_ARM_PRIVATE_5 = 0x75;
static constexpr u32 R_ARM_PRIVATE_6 = 0x76;
static constexpr u32 R_ARM_PRIVATE_7 = 0x77;
static constexpr u32 R_ARM_PRIVATE_8 = 0x78;
static constexpr u32 R_ARM_PRIVATE_9 = 0x79;
static constexpr u32 R_ARM_PRIVATE_10 = 0x7a;
static constexpr u32 R_ARM_PRIVATE_11 = 0x7b;
static constexpr u32 R_ARM_PRIVATE_12 = 0x7c;
static constexpr u32 R_ARM_PRIVATE_13 = 0x7d;
static constexpr u32 R_ARM_PRIVATE_14 = 0x7e;
static constexpr u32 R_ARM_PRIVATE_15 = 0x7f;
static constexpr u32 R_ARM_ME_TOO = 0x80;
static constexpr u32 R_ARM_THM_TLS_DESCSEQ16 = 0x81;
static constexpr u32 R_ARM_THM_TLS_DESCSEQ32 = 0x82;
static constexpr u32 R_ARM_THM_BF16 = 0x88;
static constexpr u32 R_ARM_THM_BF12 = 0x89;
static constexpr u32 R_ARM_THM_BF18 = 0x8a;
static constexpr u32 R_ARM_IRELATIVE = 0xa0;

template <>
inline std::string rel_to_string<ARM32>(u32 r_type) {
  switch (r_type) {
  case R_ARM_NONE: return "R_ARM_NONE";
  case R_ARM_PC24: return "R_ARM_PC24";
  case R_ARM_ABS32: return "R_ARM_ABS32";
  case R_ARM_REL32: return "R_ARM_REL32";
  case R_ARM_LDR_PC_G0: return "R_ARM_LDR_PC_G0";
  case R_ARM_ABS16: return "R_ARM_ABS16";
  case R_ARM_ABS12: return "R_ARM_ABS12";
  case R_ARM_THM_ABS5: return "R_ARM_THM_ABS5";
  case R_ARM_ABS8: return "R_ARM_ABS8";
  case R_ARM_SBREL32: return "R_ARM_SBREL32";
  case R_ARM_THM_CALL: return "R_ARM_THM_CALL";
  case R_ARM_THM_PC8: return "R_ARM_THM_PC8";
  case R_ARM_BREL_ADJ: return "R_ARM_BREL_ADJ";
  case R_ARM_TLS_DESC: return "R_ARM_TLS_DESC";
  case R_ARM_THM_SWI8: return "R_ARM_THM_SWI8";
  case R_ARM_XPC25: return "R_ARM_XPC25";
  case R_ARM_THM_XPC22: return "R_ARM_THM_XPC22";
  case R_ARM_TLS_DTPMOD32: return "R_ARM_TLS_DTPMOD32";
  case R_ARM_TLS_DTPOFF32: return "R_ARM_TLS_DTPOFF32";
  case R_ARM_TLS_TPOFF32: return "R_ARM_TLS_TPOFF32";
  case R_ARM_COPY: return "R_ARM_COPY";
  case R_ARM_GLOB_DAT: return "R_ARM_GLOB_DAT";
  case R_ARM_JUMP_SLOT: return "R_ARM_JUMP_SLOT";
  case R_ARM_RELATIVE: return "R_ARM_RELATIVE";
  case R_ARM_GOTOFF32: return "R_ARM_GOTOFF32";
  case R_ARM_BASE_PREL: return "R_ARM_BASE_PREL";
  case R_ARM_GOT_BREL: return "R_ARM_GOT_BREL";
  case R_ARM_PLT32: return "R_ARM_PLT32";
  case R_ARM_CALL: return "R_ARM_CALL";
  case R_ARM_JUMP24: return "R_ARM_JUMP24";
  case R_ARM_THM_JUMP24: return "R_ARM_THM_JUMP24";
  case R_ARM_BASE_ABS: return "R_ARM_BASE_ABS";
  case R_ARM_ALU_PCREL_7_0: return "R_ARM_ALU_PCREL_7_0";
  case R_ARM_ALU_PCREL_15_8: return "R_ARM_ALU_PCREL_15_8";
  case R_ARM_ALU_PCREL_23_15: return "R_ARM_ALU_PCREL_23_15";
  case R_ARM_LDR_SBREL_11_0_NC: return "R_ARM_LDR_SBREL_11_0_NC";
  case R_ARM_ALU_SBREL_19_12_NC: return "R_ARM_ALU_SBREL_19_12_NC";
  case R_ARM_ALU_SBREL_27_20_CK: return "R_ARM_ALU_SBREL_27_20_CK";
  case R_ARM_TARGET1: return "R_ARM_TARGET1";
  case R_ARM_SBREL31: return "R_ARM_SBREL31";
  case R_ARM_V4BX: return "R_ARM_V4BX";
  case R_ARM_TARGET2: return "R_ARM_TARGET2";
  case R_ARM_PREL31: return "R_ARM_PREL31";
  case R_ARM_MOVW_ABS_NC: return "R_ARM_MOVW_ABS_NC";
  case R_ARM_MOVT_ABS: return "R_ARM_MOVT_ABS";
  case R_ARM_MOVW_PREL_NC: return "R_ARM_MOVW_PREL_NC";
  case R_ARM_MOVT_PREL: return "R_ARM_MOVT_PREL";
  case R_ARM_THM_MOVW_ABS_NC: return "R_ARM_THM_MOVW_ABS_NC";
  case R_ARM_THM_MOVT_ABS: return "R_ARM_THM_MOVT_ABS";
  case R_ARM_THM_MOVW_PREL_NC: return "R_ARM_THM_MOVW_PREL_NC";
  case R_ARM_THM_MOVT_PREL: return "R_ARM_THM_MOVT_PREL";
  case R_ARM_THM_JUMP19: return "R_ARM_THM_JUMP19";
  case R_ARM_THM_JUMP6: return "R_ARM_THM_JUMP6";
  case R_ARM_THM_ALU_PREL_11_0: return "R_ARM_THM_ALU_PREL_11_0";
  case R_ARM_THM_PC12: return "R_ARM_THM_PC12";
  case R_ARM_ABS32_NOI: return "R_ARM_ABS32_NOI";
  case R_ARM_REL32_NOI: return "R_ARM_REL32_NOI";
  case R_ARM_ALU_PC_G0_NC: return "R_ARM_ALU_PC_G0_NC";
  case R_ARM_ALU_PC_G0: return "R_ARM_ALU_PC_G0";
  case R_ARM_ALU_PC_G1_NC: return "R_ARM_ALU_PC_G1_NC";
  case R_ARM_ALU_PC_G1: return "R_ARM_ALU_PC_G1";
  case R_ARM_ALU_PC_G2: return "R_ARM_ALU_PC_G2";
  case R_ARM_LDR_PC_G1: return "R_ARM_LDR_PC_G1";
  case R_ARM_LDR_PC_G2: return "R_ARM_LDR_PC_G2";
  case R_ARM_LDRS_PC_G0: return "R_ARM_LDRS_PC_G0";
  case R_ARM_LDRS_PC_G1: return "R_ARM_LDRS_PC_G1";
  case R_ARM_LDRS_PC_G2: return "R_ARM_LDRS_PC_G2";
  case R_ARM_LDC_PC_G0: return "R_ARM_LDC_PC_G0";
  case R_ARM_LDC_PC_G1: return "R_ARM_LDC_PC_G1";
  case R_ARM_LDC_PC_G2: return "R_ARM_LDC_PC_G2";
  case R_ARM_ALU_SB_G0_NC: return "R_ARM_ALU_SB_G0_NC";
  case R_ARM_ALU_SB_G0: return "R_ARM_ALU_SB_G0";
  case R_ARM_ALU_SB_G1_NC: return "R_ARM_ALU_SB_G1_NC";
  case R_ARM_ALU_SB_G1: return "R_ARM_ALU_SB_G1";
  case R_ARM_ALU_SB_G2: return "R_ARM_ALU_SB_G2";
  case R_ARM_LDR_SB_G0: return "R_ARM_LDR_SB_G0";
  case R_ARM_LDR_SB_G1: return "R_ARM_LDR_SB_G1";
  case R_ARM_LDR_SB_G2: return "R_ARM_LDR_SB_G2";
  case R_ARM_LDRS_SB_G0: return "R_ARM_LDRS_SB_G0";
  case R_ARM_LDRS_SB_G1: return "R_ARM_LDRS_SB_G1";
  case R_ARM_LDRS_SB_G2: return "R_ARM_LDRS_SB_G2";
  case R_ARM_LDC_SB_G0: return "R_ARM_LDC_SB_G0";
  case R_ARM_LDC_SB_G1: return "R_ARM_LDC_SB_G1";
  case R_ARM_LDC_SB_G2: return "R_ARM_LDC_SB_G2";
  case R_ARM_MOVW_BREL_NC: return "R_ARM_MOVW_BREL_NC";
  case R_ARM_MOVT_BREL: return "R_ARM_MOVT_BREL";
  case R_ARM_MOVW_BREL: return "R_ARM_MOVW_BREL";
  case R_ARM_THM_MOVW_BREL_NC: return "R_ARM_THM_MOVW_BREL_NC";
  case R_ARM_THM_MOVT_BREL: return "R_ARM_THM_MOVT_BREL";
  case R_ARM_THM_MOVW_BREL: return "R_ARM_THM_MOVW_BREL";
  case R_ARM_TLS_GOTDESC: return "R_ARM_TLS_GOTDESC";
  case R_ARM_TLS_CALL: return "R_ARM_TLS_CALL";
  case R_ARM_TLS_DESCSEQ: return "R_ARM_TLS_DESCSEQ";
  case R_ARM_THM_TLS_CALL: return "R_ARM_THM_TLS_CALL";
  case R_ARM_PLT32_ABS: return "R_ARM_PLT32_ABS";
  case R_ARM_GOT_ABS: return "R_ARM_GOT_ABS";
  case R_ARM_GOT_PREL: return "R_ARM_GOT_PREL";
  case R_ARM_GOT_BREL12: return "R_ARM_GOT_BREL12";
  case R_ARM_GOTOFF12: return "R_ARM_GOTOFF12";
  case R_ARM_GOTRELAX: return "R_ARM_GOTRELAX";
  case R_ARM_GNU_VTENTRY: return "R_ARM_GNU_VTENTRY";
  case R_ARM_GNU_VTINHERIT: return "R_ARM_GNU_VTINHERIT";
  case R_ARM_THM_JUMP11: return "R_ARM_THM_JUMP11";
  case R_ARM_THM_JUMP8: return "R_ARM_THM_JUMP8";
  case R_ARM_TLS_GD32: return "R_ARM_TLS_GD32";
  case R_ARM_TLS_LDM32: return "R_ARM_TLS_LDM32";
  case R_ARM_TLS_LDO32: return "R_ARM_TLS_LDO32";
  case R_ARM_TLS_IE32: return "R_ARM_TLS_IE32";
  case R_ARM_TLS_LE32: return "R_ARM_TLS_LE32";
  case R_ARM_TLS_LDO12: return "R_ARM_TLS_LDO12";
  case R_ARM_TLS_LE12: return "R_ARM_TLS_LE12";
  case R_ARM_TLS_IE12GP: return "R_ARM_TLS_IE12GP";
  case R_ARM_PRIVATE_0: return "R_ARM_PRIVATE_0";
  case R_ARM_PRIVATE_1: return "R_ARM_PRIVATE_1";
  case R_ARM_PRIVATE_2: return "R_ARM_PRIVATE_2";
  case R_ARM_PRIVATE_3: return "R_ARM_PRIVATE_3";
  case R_ARM_PRIVATE_4: return "R_ARM_PRIVATE_4";
  case R_ARM_PRIVATE_5: return "R_ARM_PRIVATE_5";
  case R_ARM_PRIVATE_6: return "R_ARM_PRIVATE_6";
  case R_ARM_PRIVATE_7: return "R_ARM_PRIVATE_7";
  case R_ARM_PRIVATE_8: return "R_ARM_PRIVATE_8";
  case R_ARM_PRIVATE_9: return "R_ARM_PRIVATE_9";
  case R_ARM_PRIVATE_10: return "R_ARM_PRIVATE_10";
  case R_ARM_PRIVATE_11: return "R_ARM_PRIVATE_11";
  case R_ARM_PRIVATE_12: return "R_ARM_PRIVATE_12";
  case R_ARM_PRIVATE_13: return "R_ARM_PRIVATE_13";
  case R_ARM_PRIVATE_14: return "R_ARM_PRIVATE_14";
  case R_ARM_PRIVATE_15: return "R_ARM_PRIVATE_15";
  case R_ARM_ME_TOO: return "R_ARM_ME_TOO";
  case R_ARM_THM_TLS_DESCSEQ16: return "R_ARM_THM_TLS_DESCSEQ16";
  case R_ARM_THM_TLS_DESCSEQ32: return "R_ARM_THM_TLS_DESCSEQ32";
  case R_ARM_THM_BF16: return "R_ARM_THM_BF16";
  case R_ARM_THM_BF12: return "R_ARM_THM_BF12";
  case R_ARM_THM_BF18: return "R_ARM_THM_BF18";
  case R_ARM_IRELATIVE: return "R_ARM_IRELATIVE";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

static constexpr u32 R_RISCV_NONE = 0;
static constexpr u32 R_RISCV_32 = 1;
static constexpr u32 R_RISCV_64 = 2;
static constexpr u32 R_RISCV_RELATIVE = 3;
static constexpr u32 R_RISCV_COPY = 4;
static constexpr u32 R_RISCV_JUMP_SLOT = 5;
static constexpr u32 R_RISCV_TLS_DTPMOD32 = 6;
static constexpr u32 R_RISCV_TLS_DTPMOD64 = 7;
static constexpr u32 R_RISCV_TLS_DTPREL32 = 8;
static constexpr u32 R_RISCV_TLS_DTPREL64 = 9;
static constexpr u32 R_RISCV_TLS_TPREL32 = 10;
static constexpr u32 R_RISCV_TLS_TPREL64 = 11;
static constexpr u32 R_RISCV_BRANCH = 16;
static constexpr u32 R_RISCV_JAL = 17;
static constexpr u32 R_RISCV_CALL = 18;
static constexpr u32 R_RISCV_CALL_PLT = 19;
static constexpr u32 R_RISCV_GOT_HI20 = 20;
static constexpr u32 R_RISCV_TLS_GOT_HI20 = 21;
static constexpr u32 R_RISCV_TLS_GD_HI20 = 22;
static constexpr u32 R_RISCV_PCREL_HI20 = 23;
static constexpr u32 R_RISCV_PCREL_LO12_I = 24;
static constexpr u32 R_RISCV_PCREL_LO12_S = 25;
static constexpr u32 R_RISCV_HI20 = 26;
static constexpr u32 R_RISCV_LO12_I = 27;
static constexpr u32 R_RISCV_LO12_S = 28;
static constexpr u32 R_RISCV_TPREL_HI20 = 29;
static constexpr u32 R_RISCV_TPREL_LO12_I = 30;
static constexpr u32 R_RISCV_TPREL_LO12_S = 31;
static constexpr u32 R_RISCV_TPREL_ADD = 32;
static constexpr u32 R_RISCV_ADD8 = 33;
static constexpr u32 R_RISCV_ADD16 = 34;
static constexpr u32 R_RISCV_ADD32 = 35;
static constexpr u32 R_RISCV_ADD64 = 36;
static constexpr u32 R_RISCV_SUB8 = 37;
static constexpr u32 R_RISCV_SUB16 = 38;
static constexpr u32 R_RISCV_SUB32 = 39;
static constexpr u32 R_RISCV_SUB64 = 40;
static constexpr u32 R_RISCV_ALIGN = 43;
static constexpr u32 R_RISCV_RVC_BRANCH = 44;
static constexpr u32 R_RISCV_RVC_JUMP = 45;
static constexpr u32 R_RISCV_RVC_LUI = 46;
static constexpr u32 R_RISCV_RELAX = 51;
static constexpr u32 R_RISCV_SUB6 = 52;
static constexpr u32 R_RISCV_SET6 = 53;
static constexpr u32 R_RISCV_SET8 = 54;
static constexpr u32 R_RISCV_SET16 = 55;
static constexpr u32 R_RISCV_SET32 = 56;
static constexpr u32 R_RISCV_32_PCREL = 57;
static constexpr u32 R_RISCV_IRELATIVE = 58;

template <>
inline std::string rel_to_string<RISCV64>(u32 r_type) {
  switch (r_type) {
  case R_RISCV_NONE: return "R_RISCV_NONE";
  case R_RISCV_32: return "R_RISCV_32";
  case R_RISCV_64: return "R_RISCV_64";
  case R_RISCV_RELATIVE: return "R_RISCV_RELATIVE";
  case R_RISCV_COPY: return "R_RISCV_COPY";
  case R_RISCV_JUMP_SLOT: return "R_RISCV_JUMP_SLOT";
  case R_RISCV_TLS_DTPMOD32: return "R_RISCV_TLS_DTPMOD32";
  case R_RISCV_TLS_DTPMOD64: return "R_RISCV_TLS_DTPMOD64";
  case R_RISCV_TLS_DTPREL32: return "R_RISCV_TLS_DTPREL32";
  case R_RISCV_TLS_DTPREL64: return "R_RISCV_TLS_DTPREL64";
  case R_RISCV_TLS_TPREL32: return "R_RISCV_TLS_TPREL32";
  case R_RISCV_TLS_TPREL64: return "R_RISCV_TLS_TPREL64";
  case R_RISCV_BRANCH: return "R_RISCV_BRANCH";
  case R_RISCV_JAL: return "R_RISCV_JAL";
  case R_RISCV_CALL: return "R_RISCV_CALL";
  case R_RISCV_CALL_PLT: return "R_RISCV_CALL_PLT";
  case R_RISCV_GOT_HI20: return "R_RISCV_GOT_HI20";
  case R_RISCV_TLS_GOT_HI20: return "R_RISCV_TLS_GOT_HI20";
  case R_RISCV_TLS_GD_HI20: return "R_RISCV_TLS_GD_HI20";
  case R_RISCV_PCREL_HI20: return "R_RISCV_PCREL_HI20";
  case R_RISCV_PCREL_LO12_I: return "R_RISCV_PCREL_LO12_I";
  case R_RISCV_PCREL_LO12_S: return "R_RISCV_PCREL_LO12_S";
  case R_RISCV_HI20: return "R_RISCV_HI20";
  case R_RISCV_LO12_I: return "R_RISCV_LO12_I";
  case R_RISCV_LO12_S: return "R_RISCV_LO12_S";
  case R_RISCV_TPREL_HI20: return "R_RISCV_TPREL_HI20";
  case R_RISCV_TPREL_LO12_I: return "R_RISCV_TPREL_LO12_I";
  case R_RISCV_TPREL_LO12_S: return "R_RISCV_TPREL_LO12_S";
  case R_RISCV_TPREL_ADD: return "R_RISCV_TPREL_ADD";
  case R_RISCV_ADD8: return "R_RISCV_ADD8";
  case R_RISCV_ADD16: return "R_RISCV_ADD16";
  case R_RISCV_ADD32: return "R_RISCV_ADD32";
  case R_RISCV_ADD64: return "R_RISCV_ADD64";
  case R_RISCV_SUB8: return "R_RISCV_SUB8";
  case R_RISCV_SUB16: return "R_RISCV_SUB16";
  case R_RISCV_SUB32: return "R_RISCV_SUB32";
  case R_RISCV_SUB64: return "R_RISCV_SUB64";
  case R_RISCV_ALIGN: return "R_RISCV_ALIGN";
  case R_RISCV_RVC_BRANCH: return "R_RISCV_RVC_BRANCH";
  case R_RISCV_RVC_JUMP: return "R_RISCV_RVC_JUMP";
  case R_RISCV_RVC_LUI: return "R_RISCV_RVC_LUI";
  case R_RISCV_RELAX: return "R_RISCV_RELAX";
  case R_RISCV_SUB6: return "R_RISCV_SUB6";
  case R_RISCV_SET6: return "R_RISCV_SET6";
  case R_RISCV_SET8: return "R_RISCV_SET8";
  case R_RISCV_SET16: return "R_RISCV_SET16";
  case R_RISCV_SET32: return "R_RISCV_SET32";
  case R_RISCV_32_PCREL: return "R_RISCV_32_PCREL";
  case R_RISCV_IRELATIVE: return "R_RISCV_IRELATIVE";
  }
  return "unknown (" + std::to_string(r_type) + ")";
}

template <>
inline std::string rel_to_string<RISCV32>(u32 r_type) {
  return rel_to_string<RISCV64>(r_type);
}

static constexpr u32 DW_EH_PE_absptr = 0;
static constexpr u32 DW_EH_PE_omit = 0xff;
static constexpr u32 DW_EH_PE_uleb128 = 0x01;
static constexpr u32 DW_EH_PE_udata2 = 0x02;
static constexpr u32 DW_EH_PE_udata4 = 0x03;
static constexpr u32 DW_EH_PE_udata8 = 0x04;
static constexpr u32 DW_EH_PE_signed = 0x08;
static constexpr u32 DW_EH_PE_sleb128 = 0x09;
static constexpr u32 DW_EH_PE_sdata2 = 0x0a;
static constexpr u32 DW_EH_PE_sdata4 = 0x0b;
static constexpr u32 DW_EH_PE_sdata8 = 0x0c;
static constexpr u32 DW_EH_PE_pcrel = 0x10;
static constexpr u32 DW_EH_PE_textrel = 0x20;
static constexpr u32 DW_EH_PE_datarel = 0x30;
static constexpr u32 DW_EH_PE_funcrel = 0x40;
static constexpr u32 DW_EH_PE_aligned = 0x50;

static constexpr u32 DW_AT_low_pc = 0x11;
static constexpr u32 DW_AT_high_pc = 0x12;
static constexpr u32 DW_AT_producer = 0x25;
static constexpr u32 DW_AT_ranges = 0x55;
static constexpr u32 DW_AT_addr_base = 0x73;
static constexpr u32 DW_AT_rnglists_base = 0x74;

static constexpr u32 DW_TAG_compile_unit = 0x11;
static constexpr u32 DW_TAG_skeleton_unit = 0x4a;

static constexpr u32 DW_UT_compile = 0x01;
static constexpr u32 DW_UT_partial = 0x03;
static constexpr u32 DW_UT_skeleton = 0x04;
static constexpr u32 DW_UT_split_compile = 0x05;

static constexpr u32 DW_FORM_addr = 0x01;
static constexpr u32 DW_FORM_block2 = 0x03;
static constexpr u32 DW_FORM_block4 = 0x04;
static constexpr u32 DW_FORM_data2 = 0x05;
static constexpr u32 DW_FORM_data4 = 0x06;
static constexpr u32 DW_FORM_data8 = 0x07;
static constexpr u32 DW_FORM_string = 0x08;
static constexpr u32 DW_FORM_block = 0x09;
static constexpr u32 DW_FORM_block1 = 0x0a;
static constexpr u32 DW_FORM_data1 = 0x0b;
static constexpr u32 DW_FORM_flag = 0x0c;
static constexpr u32 DW_FORM_sdata = 0x0d;
static constexpr u32 DW_FORM_strp = 0x0e;
static constexpr u32 DW_FORM_udata = 0x0f;
static constexpr u32 DW_FORM_ref_addr = 0x10;
static constexpr u32 DW_FORM_ref1 = 0x11;
static constexpr u32 DW_FORM_ref2 = 0x12;
static constexpr u32 DW_FORM_ref4 = 0x13;
static constexpr u32 DW_FORM_ref8 = 0x14;
static constexpr u32 DW_FORM_ref_udata = 0x15;
static constexpr u32 DW_FORM_indirect = 0x16;
static constexpr u32 DW_FORM_sec_offset = 0x17;
static constexpr u32 DW_FORM_exprloc = 0x18;
static constexpr u32 DW_FORM_flag_present = 0x19;
static constexpr u32 DW_FORM_strx = 0x1a;
static constexpr u32 DW_FORM_addrx = 0x1b;
static constexpr u32 DW_FORM_ref_sup4 = 0x1c;
static constexpr u32 DW_FORM_strp_sup = 0x1d;
static constexpr u32 DW_FORM_data16 = 0x1e;
static constexpr u32 DW_FORM_line_strp = 0x1f;
static constexpr u32 DW_FORM_ref_sig8 = 0x20;
static constexpr u32 DW_FORM_implicit_const = 0x21;
static constexpr u32 DW_FORM_loclistx = 0x22;
static constexpr u32 DW_FORM_rnglistx = 0x23;
static constexpr u32 DW_FORM_ref_sup8 = 0x24;
static constexpr u32 DW_FORM_strx1 = 0x25;
static constexpr u32 DW_FORM_strx2 = 0x26;
static constexpr u32 DW_FORM_strx3 = 0x27;
static constexpr u32 DW_FORM_strx4 = 0x28;
static constexpr u32 DW_FORM_addrx1 = 0x29;
static constexpr u32 DW_FORM_addrx2 = 0x2a;
static constexpr u32 DW_FORM_addrx3 = 0x2b;
static constexpr u32 DW_FORM_addrx4 = 0x2c;

static constexpr u32 DW_RLE_end_of_list = 0x00;
static constexpr u32 DW_RLE_base_addressx = 0x01;
static constexpr u32 DW_RLE_startx_endx = 0x02;
static constexpr u32 DW_RLE_startx_length = 0x03;
static constexpr u32 DW_RLE_offset_pair = 0x04;
static constexpr u32 DW_RLE_base_address = 0x05;
static constexpr u32 DW_RLE_start_end = 0x06;
static constexpr u32 DW_RLE_start_length = 0x07;

struct Elf64Sym {
  bool is_defined() const { return !is_undef(); }
  bool is_undef() const { return st_shndx == SHN_UNDEF; }
  bool is_abs() const { return st_shndx == SHN_ABS; }
  bool is_common() const { return st_shndx == SHN_COMMON; }
  bool is_weak() const { return st_bind == STB_WEAK; }
  bool is_undef_weak() const { return is_undef() && is_weak(); }

  ul32 st_name;
  u8 st_type : 4;
  u8 st_bind : 4;
  u8 st_visibility : 2;
  ul16 st_shndx;
  ul64 st_value;
  ul64 st_size;
};

struct Elf32Sym {
  bool is_defined() const { return !is_undef(); }
  bool is_undef() const { return st_shndx == SHN_UNDEF; }
  bool is_abs() const { return st_shndx == SHN_ABS; }
  bool is_common() const { return st_shndx == SHN_COMMON; }
  bool is_weak() const { return st_bind == STB_WEAK; }
  bool is_undef_weak() const { return is_undef() && is_weak(); }

  ul32 st_name;
  ul32 st_value;
  ul32 st_size;
  u8 st_type : 4;
  u8 st_bind : 4;
  u8 st_visibility : 2;
  ul16 st_shndx;
};

struct Elf64Shdr {
  ul32 sh_name;
  ul32 sh_type;
  ul64 sh_flags;
  ul64 sh_addr;
  ul64 sh_offset;
  ul64 sh_size;
  ul32 sh_link;
  ul32 sh_info;
  ul64 sh_addralign;
  ul64 sh_entsize;
};

struct Elf32Shdr {
  ul32 sh_name;
  ul32 sh_type;
  ul32 sh_flags;
  ul32 sh_addr;
  ul32 sh_offset;
  ul32 sh_size;
  ul32 sh_link;
  ul32 sh_info;
  ul32 sh_addralign;
  ul32 sh_entsize;
};

struct Elf64Ehdr {
  u8 e_ident[16];
  ul16 e_type;
  ul16 e_machine;
  ul32 e_version;
  ul64 e_entry;
  ul64 e_phoff;
  ul64 e_shoff;
  ul32 e_flags;
  ul16 e_ehsize;
  ul16 e_phentsize;
  ul16 e_phnum;
  ul16 e_shentsize;
  ul16 e_shnum;
  ul16 e_shstrndx;
};

struct Elf32Ehdr {
  u8 e_ident[16];
  ul16 e_type;
  ul16 e_machine;
  ul32 e_version;
  ul32 e_entry;
  ul32 e_phoff;
  ul32 e_shoff;
  ul32 e_flags;
  ul16 e_ehsize;
  ul16 e_phentsize;
  ul16 e_phnum;
  ul16 e_shentsize;
  ul16 e_shnum;
  ul16 e_shstrndx;
};

struct Elf64Phdr {
  ul32 p_type;
  ul32 p_flags;
  ul64 p_offset;
  ul64 p_vaddr;
  ul64 p_paddr;
  ul64 p_filesz;
  ul64 p_memsz;
  ul64 p_align;
};

struct Elf32Phdr {
  ul32 p_type;
  ul32 p_offset;
  ul32 p_vaddr;
  ul32 p_paddr;
  ul32 p_filesz;
  ul32 p_memsz;
  ul32 p_flags;
  ul32 p_align;
};

// Depending on the target, ElfRel may or may not contain r_addend member.
// The relocation record containing r_addend is called RELA, and that
// without r_addend is called REL.
//
// If REL, relocation addends are stored as parts of section contents.
// That means we add a computed value to an existing value when writing a
// relocated value if REL. If RELA, we just overwrite an existing value
// with a newly computed value.
//
// We don't want to have too many `if (REL)`s and `if (RELA)`s in our
// codebase, so we write dynamic relocations in the following manner:
//
// - We always create a dynamic relocation with an addend. If it's REL,
//   the addend will be discarded.
//
// - We also always write an addend to the relocated place even though
//   it's redundant for RELA. If RELA, the written value will be
//   ovewritten by the dynamic linker at load-time.
struct Elf64Rel {
  Elf64Rel(u64 r_offset, u32 r_type, u32 r_sym, i64 r_addend = 0)
    : r_offset(r_offset), r_type(r_type), r_sym(r_sym) {}

  ul64 r_offset;
  ul32 r_type;
  ul32 r_sym;
};

struct Elf32Rel {
  Elf32Rel(u64 r_offset, u32 r_type, u32 r_sym, i64 r_addend = 0)
    : r_offset(r_offset), r_type(r_type), r_sym(r_sym) {}

  ul32 r_offset;
  u8 r_type;
  ul24 r_sym;
};

struct Elf64Rela {
  Elf64Rela(u64 r_offset, u32 r_type, u32 r_sym, i64 r_addend)
    : r_offset(r_offset), r_type(r_type), r_sym(r_sym), r_addend(r_addend) {}

  ul64 r_offset;
  ul32 r_type;
  ul32 r_sym;
  il64 r_addend;
};

struct Elf32Rela {
  Elf32Rela(u64 r_offset, u32 r_type, u32 r_sym, i64 r_addend)
    : r_offset(r_offset), r_type(r_type), r_sym(r_sym), r_addend(r_addend) {}

  ul32 r_offset;
  u8 r_type;
  ul24 r_sym;
  il32 r_addend;
};

struct Elf64Dyn {
  ul64 d_tag;
  ul64 d_val;
};

struct Elf32Dyn {
  ul32 d_tag;
  ul32 d_val;
};

struct ElfVerneed {
  ul16 vn_version;
  ul16 vn_cnt;
  ul32 vn_file;
  ul32 vn_aux;
  ul32 vn_next;
};

struct ElfVernaux {
  ul32 vna_hash;
  ul16 vna_flags;
  ul16 vna_other;
  ul32 vna_name;
  ul32 vna_next;
};

struct ElfVerdef {
  ul16 vd_version;
  ul16 vd_flags;
  ul16 vd_ndx;
  ul16 vd_cnt;
  ul32 vd_hash;
  ul32 vd_aux;
  ul32 vd_next;
};

struct ElfVerdaux {
  ul32 vda_name;
  ul32 vda_next;
};

struct Elf64Chdr {
  ul32 ch_type;
  ul32 ch_reserved;
  ul64 ch_size;
  ul64 ch_addralign;
};

struct Elf32Chdr {
  ul32 ch_type;
  ul32 ch_size;
  ul32 ch_addralign;
};

struct ElfNhdr {
  ul32 n_namesz;
  ul32 n_descsz;
  ul32 n_type;
};

template <typename E>
inline constexpr bool is_rela = requires(ElfRel<E> r) { r.r_addend; };

template <typename E>
inline constexpr bool supports_tlsdesc = requires { E::R_TLSDESC; };

template <typename E>
inline constexpr bool needs_thunk = requires { E::thunk_size; };

template <typename E>
using Word = typename E::Word;

struct X86_64 {
  using Word = ul64;

  static constexpr u32 R_NONE = R_X86_64_NONE;
  static constexpr u32 R_COPY = R_X86_64_COPY;
  static constexpr u32 R_GLOB_DAT = R_X86_64_GLOB_DAT;
  static constexpr u32 R_JUMP_SLOT = R_X86_64_JUMP_SLOT;
  static constexpr u32 R_ABS = R_X86_64_64;
  static constexpr u32 R_RELATIVE = R_X86_64_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_X86_64_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_X86_64_DTPOFF64;
  static constexpr u32 R_TPOFF = R_X86_64_TPOFF64;
  static constexpr u32 R_DTPMOD = R_X86_64_DTPMOD64;
  static constexpr u32 R_TLSDESC = R_X86_64_TLSDESC;

  static constexpr MachineType machine_type = MachineType::X86_64;
  static constexpr u32 page_size = 4096;
  static constexpr u32 e_machine = EM_X86_64;
  static constexpr u32 plt_hdr_size = 32;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 16;
};

template <> struct ElfSym<X86_64> : public Elf64Sym {};
template <> struct ElfShdr<X86_64> : public Elf64Shdr {};
template <> struct ElfEhdr<X86_64> : public Elf64Ehdr {};
template <> struct ElfPhdr<X86_64> : public Elf64Phdr {};
template <> struct ElfRel<X86_64> : public Elf64Rela { using Elf64Rela::Elf64Rela; };
template <> struct ElfDyn<X86_64> : public Elf64Dyn {};
template <> struct ElfChdr<X86_64> : public Elf64Chdr {};

struct I386 {
  using Word = ul32;

  static constexpr u32 R_NONE = R_386_NONE;
  static constexpr u32 R_COPY = R_386_COPY;
  static constexpr u32 R_GLOB_DAT = R_386_GLOB_DAT;
  static constexpr u32 R_JUMP_SLOT = R_386_JUMP_SLOT;
  static constexpr u32 R_ABS = R_386_32;
  static constexpr u32 R_RELATIVE = R_386_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_386_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_386_TLS_DTPOFF32;
  static constexpr u32 R_TPOFF = R_386_TLS_TPOFF;
  static constexpr u32 R_DTPMOD = R_386_TLS_DTPMOD32;
  static constexpr u32 R_TLSDESC = R_386_TLS_DESC;

  static constexpr MachineType machine_type = MachineType::I386;
  static constexpr u32 page_size = 4096;
  static constexpr u32 e_machine = EM_386;
  static constexpr u32 plt_hdr_size = 16;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 8;
};

template <> struct ElfSym<I386> : public Elf32Sym {};
template <> struct ElfShdr<I386> : public Elf32Shdr {};
template <> struct ElfEhdr<I386> : public Elf32Ehdr {};
template <> struct ElfPhdr<I386> : public Elf32Phdr {};
template <> struct ElfRel<I386> : public Elf32Rel { using Elf32Rel::Elf32Rel; };
template <> struct ElfDyn<I386> : public Elf32Dyn {};
template <> struct ElfChdr<I386> : public Elf32Chdr {};

struct ARM64 {
  using Word = ul64;

  static constexpr u32 R_NONE = R_AARCH64_NONE;
  static constexpr u32 R_COPY = R_AARCH64_COPY;
  static constexpr u32 R_GLOB_DAT = R_AARCH64_GLOB_DAT;
  static constexpr u32 R_JUMP_SLOT = R_AARCH64_JUMP_SLOT;
  static constexpr u32 R_ABS = R_AARCH64_ABS64;
  static constexpr u32 R_RELATIVE = R_AARCH64_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_AARCH64_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_AARCH64_TLS_DTPREL64;
  static constexpr u32 R_TPOFF = R_AARCH64_TLS_TPREL64;
  static constexpr u32 R_DTPMOD = R_AARCH64_TLS_DTPMOD64;
  static constexpr u32 R_TLSDESC = R_AARCH64_TLSDESC;

  static constexpr MachineType machine_type = MachineType::ARM64;
  static constexpr u32 page_size = 65536;
  static constexpr u32 e_machine = EM_AARCH64;
  static constexpr u32 plt_hdr_size = 32;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 16;
  static constexpr u32 tls_tp_offset = 16;

  static constexpr u32 thunk_size = 12;
  static constexpr u32 thunk_max_distance = 100 * 1024 * 1024;
  static constexpr u32 thunk_group_size = 10 * 1024 * 1024;
};

template <> struct ElfSym<ARM64> : public Elf64Sym {};
template <> struct ElfShdr<ARM64> : public Elf64Shdr {};
template <> struct ElfEhdr<ARM64> : public Elf64Ehdr {};
template <> struct ElfPhdr<ARM64> : public Elf64Phdr {};
template <> struct ElfRel<ARM64> : public Elf64Rela { using Elf64Rela::Elf64Rela; };
template <> struct ElfDyn<ARM64> : public Elf64Dyn {};
template <> struct ElfChdr<ARM64> : public Elf64Chdr {};

struct ARM32 {
  using Word = ul32;

  static constexpr u32 R_NONE = R_ARM_NONE;
  static constexpr u32 R_COPY = R_ARM_COPY;
  static constexpr u32 R_GLOB_DAT = R_ARM_GLOB_DAT;
  static constexpr u32 R_JUMP_SLOT = R_ARM_JUMP_SLOT;
  static constexpr u32 R_ABS = R_ARM_ABS32;
  static constexpr u32 R_RELATIVE = R_ARM_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_ARM_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_ARM_TLS_DTPOFF32;
  static constexpr u32 R_TPOFF = R_ARM_TLS_TPOFF32;
  static constexpr u32 R_DTPMOD = R_ARM_TLS_DTPMOD32;
  static constexpr u32 R_TLSDESC = R_ARM_TLS_DESC;

  static constexpr MachineType machine_type = MachineType::ARM32;
  static constexpr u32 page_size = 4096;
  static constexpr u32 e_machine = EM_ARM;
  static constexpr u32 plt_hdr_size = 32;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 16;
  static constexpr u32 tls_tp_offset = 8;

  static constexpr u32 thunk_size = 20;
  static constexpr u32 thunk_max_distance = 10 * 1024 * 1024;
  static constexpr u32 thunk_group_size = 2 * 1024 * 1024;
};

template <> struct ElfSym<ARM32> : public Elf32Sym {};
template <> struct ElfShdr<ARM32> : public Elf32Shdr {};
template <> struct ElfEhdr<ARM32> : public Elf32Ehdr {};
template <> struct ElfPhdr<ARM32> : public Elf32Phdr {};
template <> struct ElfRel<ARM32> : public Elf32Rel { using Elf32Rel::Elf32Rel; };
template <> struct ElfDyn<ARM32> : public Elf32Dyn {};
template <> struct ElfChdr<ARM32> : public Elf32Chdr {};

struct RISCV64 {
  using Word = ul64;

  static constexpr u32 R_NONE = R_RISCV_NONE;
  static constexpr u32 R_COPY = R_RISCV_COPY;
  static constexpr u32 R_GLOB_DAT = R_RISCV_64;
  static constexpr u32 R_JUMP_SLOT = R_RISCV_JUMP_SLOT;
  static constexpr u32 R_ABS = R_RISCV_64;
  static constexpr u32 R_RELATIVE = R_RISCV_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_RISCV_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_RISCV_TLS_DTPREL64;
  static constexpr u32 R_TPOFF = R_RISCV_TLS_TPREL64;
  static constexpr u32 R_DTPMOD = R_RISCV_TLS_DTPMOD64;

  static constexpr MachineType machine_type = MachineType::RISCV64;
  static constexpr u32 page_size = 4096;
  static constexpr u32 e_machine = EM_RISCV;
  static constexpr u32 plt_hdr_size = 32;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 16;
  static constexpr u32 tls_tp_offset = 0;
};

template <> struct ElfSym<RISCV64> : public Elf64Sym {};
template <> struct ElfShdr<RISCV64> : public Elf64Shdr {};
template <> struct ElfEhdr<RISCV64> : public Elf64Ehdr {};
template <> struct ElfPhdr<RISCV64> : public Elf64Phdr {};
template <> struct ElfRel<RISCV64> : public Elf64Rela { using Elf64Rela::Elf64Rela; };
template <> struct ElfDyn<RISCV64> : public Elf64Dyn {};
template <> struct ElfChdr<RISCV64> : public Elf64Chdr {};

struct RISCV32 {
  using Word = ul32;

  static constexpr u32 R_NONE = R_RISCV_NONE;
  static constexpr u32 R_COPY = R_RISCV_COPY;
  static constexpr u32 R_GLOB_DAT = R_RISCV_32;
  static constexpr u32 R_JUMP_SLOT = R_RISCV_JUMP_SLOT;
  static constexpr u32 R_ABS = R_RISCV_32;
  static constexpr u32 R_RELATIVE = R_RISCV_RELATIVE;
  static constexpr u32 R_IRELATIVE = R_RISCV_IRELATIVE;
  static constexpr u32 R_DTPOFF = R_RISCV_TLS_DTPREL32;
  static constexpr u32 R_TPOFF = R_RISCV_TLS_TPREL32;
  static constexpr u32 R_DTPMOD = R_RISCV_TLS_DTPMOD32;

  static constexpr MachineType machine_type = MachineType::RISCV32;
  static constexpr u32 page_size = 4096;
  static constexpr u32 e_machine = EM_RISCV;
  static constexpr u32 plt_hdr_size = 32;
  static constexpr u32 plt_size = 16;
  static constexpr u32 pltgot_size = 16;
  static constexpr u32 tls_tp_offset = 0;
};

template <> struct ElfSym<RISCV32> : public Elf32Sym {};
template <> struct ElfShdr<RISCV32> : public Elf32Shdr {};
template <> struct ElfEhdr<RISCV32> : public Elf32Ehdr {};
template <> struct ElfPhdr<RISCV32> : public Elf32Phdr {};
template <> struct ElfRel<RISCV32> : public Elf32Rela { using Elf32Rela::Elf32Rela; };
template <> struct ElfDyn<RISCV32> : public Elf32Dyn {};
template <> struct ElfChdr<RISCV32> : public Elf32Chdr {};

} // namespace mold::elf
