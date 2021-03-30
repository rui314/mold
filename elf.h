#pragma once

#include <cstdint>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

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
static constexpr u32 SHT_GNU_HASH = 0x6ffffff6;
static constexpr u32 SHT_GNU_VERDEF = 0x6ffffffd;
static constexpr u32 SHT_GNU_VERNEED = 0x6ffffffe;
static constexpr u32 SHT_GNU_VERSYM = 0x6fffffff;
static constexpr u32 SHT_X86_64_UNWIND = 0x70000001;

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

static constexpr u32 EM_386 = 3;
static constexpr u32 EM_X86_64 = 62;

static constexpr u32 EI_CLASS = 4;
static constexpr u32 EI_DATA = 5;
static constexpr u32 EI_VERSION = 6;
static constexpr u32 EI_OSABI = 7;
static constexpr u32 EI_ABIVERSION = 8;

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
static constexpr u32 DF_1_NOOPEN = 0x00000040;
static constexpr u32 DF_1_PIE = 0x08000000;

static constexpr u32 NT_GNU_BUILD_ID = 3;

static constexpr u32 ELFCOMPRESS_ZLIB = 1;

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
static constexpr u32 R_386_TLS_GOTDESC = 39;
static constexpr u32 R_386_TLS_DESC_CALL = 40;
static constexpr u32 R_386_TLS_DESC = 41;
static constexpr u32 R_386_IRELATIVE = 42;
static constexpr u32 R_386_GOT32X = 43;

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

struct Elf64Sym {
  bool is_defined() const { return !is_undef(); }
  bool is_undef() const { return st_shndx == SHN_UNDEF; }
  bool is_abs() const { return st_shndx == SHN_ABS; }
  bool is_common() const { return st_shndx == SHN_COMMON; }

  u32 st_name;
  u8 st_type : 4;
  u8 st_bind : 4;
  u8 st_visibility : 2;
  u16 st_shndx;
  u64 st_value;
  u64 st_size;
};

struct Elf32Sym {
  bool is_defined() const { return !is_undef(); }
  bool is_undef() const { return st_shndx == SHN_UNDEF; }
  bool is_abs() const { return st_shndx == SHN_ABS; }
  bool is_common() const { return st_shndx == SHN_COMMON; }

  u32 st_name;
  u32 st_value;
  u32 st_size;
  u8 st_type : 4;
  u8 st_bind : 4;
  u8 st_visibility : 2;
  u16 st_shndx;
};

struct Elf64Shdr {
  u32 sh_name;
  u32 sh_type;
  u64 sh_flags;
  u64 sh_addr;
  u64 sh_offset;
  u64 sh_size;
  u32 sh_link;
  u32 sh_info;
  u64 sh_addralign;
  u64 sh_entsize;
};

struct Elf32Shdr {
  u32 sh_name;
  u32 sh_type;
  u32 sh_flags;
  u32 sh_addr;
  u32 sh_offset;
  u32 sh_size;
  u32 sh_link;
  u32 sh_info;
  u32 sh_addralign;
  u32 sh_entsize;
};

struct Elf64Ehdr {
  u8 e_ident[16];
  u16 e_type;
  u16 e_machine;
  u32 e_version;
  u64 e_entry;
  u64 e_phoff;
  u64 e_shoff;
  u32 e_flags;
  u16 e_ehsize;
  u16 e_phentsize;
  u16 e_phnum;
  u16 e_shentsize;
  u16 e_shnum;
  u16 e_shstrndx;
};

struct Elf32Ehdr {
  u8 e_ident[16]; 
  u16 e_type;
  u16 e_machine;
  u32 e_version;
  u32 e_entry;
  u32 e_phoff;
  u32 e_shoff;
  u32 e_flags;
  u16 e_ehsize;
  u16 e_phentsize;
  u16 e_phnum;
  u16 e_shentsize;
  u16 e_shnum;
  u16 e_shstrndx;
};

struct Elf64Phdr {
  u32 p_type;
  u32 p_flags;
  u64 p_offset;
  u64 p_vaddr;
  u64 p_paddr;
  u64 p_filesz;
  u64 p_memsz;
  u64 p_align;
};

struct Elf32Phdr {
  u32 p_type;
  u32 p_offset;
  u32 p_vaddr;
  u32 p_paddr;
  u32 p_filesz;
  u32 p_memsz;
  u32 p_flags;
  u32 p_align;
};

struct Elf64Rel {
  u64 r_offset;
  u32 r_type;
  u32 r_sym;
};

struct Elf32Rel {
  u32 r_offset;
  u32 r_type : 8;
  u32 r_sym : 24;
};

struct Elf64Rela {
  u64 r_offset;
  u32 r_type;
  u32 r_sym;
  i64 r_addend;
};

struct Elf32Rela {
  u32 r_offset;
  u32 r_type : 8;
  u32 r_sym : 24;
  i32 r_addend;
};

struct Elf64Dyn {
  u64 d_tag;
  u64 d_val;
};

struct Elf32Dyn {
  u32 d_tag;
  u32 d_val;
};

struct Elf64Verneed {
  u16 vn_version;
  u16 vn_cnt;
  u32 vn_file;
  u32 vn_aux;
  u32 vn_next;
};

struct Elf64Vernaux {
  u32 vna_hash;
  u16 vna_flags;
  u16 vna_other;
  u32 vna_name;
  u32 vna_next;
};

struct Elf64Verdef {
  u16 vd_version;
  u16 vd_flags;
  u16 vd_ndx;
  u16 vd_cnt;
  u32 vd_hash;
  u32 vd_aux;
  u32 vd_next;
};

struct Elf64Verdaux {
  u32 vda_name;
  u32 vda_next;
};

struct Elf64Chdr {
  u32 ch_type;
  u32 ch_reserved;
  u64 ch_size;
  u64 ch_addralign;
};

struct Elf32Chdr {
  u32 ch_type;
  u32 ch_size;
  u32 ch_addralign;
};

template <typename E> struct ElfSym;
template <typename E> struct ElfShdr;
template <typename E> struct ElfEhdr;
template <typename E> struct ElfPhdr;
template <typename E> struct ElfRel;
template <typename E> struct ElfDyn;
template <typename E> struct ElfVerneed;
template <typename E> struct ElfVernaux;
template <typename E> struct ElfVerdef;
template <typename E> struct ElfVerdaux;
template <typename E> struct ElfChdr;

struct X86_64 {
  static constexpr bool is_rela = true;
  static constexpr bool is_64 = true;
  static constexpr bool is_le = true;
  static constexpr u32 e_machine = EM_X86_64;
};

template <> struct ElfSym<X86_64> : public Elf64Sym {};
template <> struct ElfShdr<X86_64> : public Elf64Shdr {};
template <> struct ElfEhdr<X86_64> : public Elf64Ehdr {};
template <> struct ElfPhdr<X86_64> : public Elf64Phdr {};
template <> struct ElfRel<X86_64> : public Elf64Rela {};
template <> struct ElfDyn<X86_64> : public Elf64Dyn {};
template <> struct ElfVerneed<X86_64> : public Elf64Verneed {};
template <> struct ElfVernaux<X86_64> : public Elf64Vernaux {};
template <> struct ElfVerdef<X86_64> : public Elf64Verdef {};
template <> struct ElfVerdaux<X86_64> : public Elf64Verdaux {};
template <> struct ElfChdr<X86_64> : public Elf64Chdr {};

struct I386 {
  static constexpr bool is_rela = false;
  static constexpr bool is_64 = false;
  static constexpr bool is_le = true;
  static constexpr u32 e_machine = EM_386;
};

template <> struct ElfSym<I386> : public Elf32Sym {};
template <> struct ElfShdr<I386> : public Elf32Shdr {};
template <> struct ElfEhdr<I386> : public Elf32Ehdr {};
template <> struct ElfPhdr<I386> : public Elf32Phdr {};
template <> struct ElfRel<I386> : public Elf32Rel {};
template <> struct ElfDyn<I386> : public Elf32Dyn {};
template <> struct ElfVerneed<I386> : public Elf64Verneed {};
template <> struct ElfVernaux<I386> : public Elf64Vernaux {};
template <> struct ElfVerdef<I386> : public Elf64Verdef {};
template <> struct ElfVerdaux<I386> : public Elf64Verdaux {};
template <> struct ElfChdr<I386> : public Elf32Chdr {};
