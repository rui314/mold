#pragma once

#include "../inttypes.h"

#include <cassert>
#include <cstdint>
#include <cstring>
#include <string>
#include <string_view>

namespace mold::macho {

struct ARM64;
struct X86_64;

template <typename E>
std::string rel_to_string(u8 r_type);

static constexpr u32 FAT_MAGIC = 0xcafebabe;

static constexpr u32 MH_OBJECT = 0x1;
static constexpr u32 MH_EXECUTE = 0x2;
static constexpr u32 MH_FVMLIB = 0x3;
static constexpr u32 MH_CORE = 0x4;
static constexpr u32 MH_PRELOAD = 0x5;
static constexpr u32 MH_DYLIB = 0x6;
static constexpr u32 MH_DYLINKER = 0x7;
static constexpr u32 MH_BUNDLE = 0x8;
static constexpr u32 MH_DYLIB_STUB = 0x9;
static constexpr u32 MH_DSYM = 0xa;
static constexpr u32 MH_KEXT_BUNDLE = 0xb;

static constexpr u32 MH_NOUNDEFS = 0x1;
static constexpr u32 MH_INCRLINK = 0x2;
static constexpr u32 MH_DYLDLINK = 0x4;
static constexpr u32 MH_BINDATLOAD = 0x8;
static constexpr u32 MH_PREBOUND = 0x10;
static constexpr u32 MH_SPLIT_SEGS = 0x20;
static constexpr u32 MH_LAZY_INIT = 0x40;
static constexpr u32 MH_TWOLEVEL = 0x80;
static constexpr u32 MH_FORCE_FLAT = 0x100;
static constexpr u32 MH_NOMULTIDEFS = 0x200;
static constexpr u32 MH_NOFIXPREBINDING = 0x400;
static constexpr u32 MH_PREBINDABLE = 0x800;
static constexpr u32 MH_ALLMODSBOUND = 0x1000;
static constexpr u32 MH_SUBSECTIONS_VIA_SYMBOLS = 0x2000;
static constexpr u32 MH_CANONICAL = 0x4000;
static constexpr u32 MH_WEAK_DEFINES = 0x8000;
static constexpr u32 MH_BINDS_TO_WEAK = 0x10000;
static constexpr u32 MH_ALLOW_STACK_EXECUTION = 0x20000;
static constexpr u32 MH_ROOT_SAFE = 0x40000;
static constexpr u32 MH_SETUID_SAFE = 0x80000;
static constexpr u32 MH_NO_REEXPORTED_DYLIBS = 0x100000;
static constexpr u32 MH_PIE = 0x200000;
static constexpr u32 MH_DEAD_STRIPPABLE_DYLIB = 0x400000;
static constexpr u32 MH_HAS_TLV_DESCRIPTORS = 0x800000;
static constexpr u32 MH_NO_HEAP_EXECUTION = 0x1000000;
static constexpr u32 MH_APP_EXTENSION_SAFE = 0x02000000;
static constexpr u32 MH_NLIST_OUTOFSYNC_WITH_DYLDINFO = 0x04000000;
static constexpr u32 MH_SIM_SUPPORT = 0x08000000;

static constexpr u32 VM_PROT_READ = 0x1;
static constexpr u32 VM_PROT_WRITE = 0x2;
static constexpr u32 VM_PROT_EXECUTE = 0x4;
static constexpr u32 VM_PROT_NO_CHANGE = 0x8;
static constexpr u32 VM_PROT_COPY = 0x10;
static constexpr u32 VM_PROT_WANTS_COPY = 0x10;

static constexpr u32 LC_REQ_DYLD = 0x80000000;

static constexpr u32 LC_SEGMENT = 0x1;
static constexpr u32 LC_SYMTAB = 0x2;
static constexpr u32 LC_SYMSEG = 0x3;
static constexpr u32 LC_THREAD = 0x4;
static constexpr u32 LC_UNIXTHREAD = 0x5;
static constexpr u32 LC_LOADFVMLIB = 0x6;
static constexpr u32 LC_IDFVMLIB = 0x7;
static constexpr u32 LC_IDENT = 0x8;
static constexpr u32 LC_FVMFILE = 0x9;
static constexpr u32 LC_PREPAGE = 0xa;
static constexpr u32 LC_DYSYMTAB = 0xb;
static constexpr u32 LC_LOAD_DYLIB = 0xc;
static constexpr u32 LC_ID_DYLIB = 0xd;
static constexpr u32 LC_LOAD_DYLINKER = 0xe;
static constexpr u32 LC_ID_DYLINKER = 0xf;
static constexpr u32 LC_PREBOUND_DYLIB = 0x10;
static constexpr u32 LC_ROUTINES = 0x11;
static constexpr u32 LC_SUB_FRAMEWORK = 0x12;
static constexpr u32 LC_SUB_UMBRELLA = 0x13;
static constexpr u32 LC_SUB_CLIENT = 0x14;
static constexpr u32 LC_SUB_LIBRARY = 0x15;
static constexpr u32 LC_TWOLEVEL_HINTS = 0x16;
static constexpr u32 LC_PREBIND_CKSUM = 0x17;
static constexpr u32 LC_LOAD_WEAK_DYLIB = (0x18 | LC_REQ_DYLD);
static constexpr u32 LC_SEGMENT_64 = 0x19;
static constexpr u32 LC_ROUTINES_64 = 0x1a;
static constexpr u32 LC_UUID = 0x1b;
static constexpr u32 LC_RPATH = (0x1c | LC_REQ_DYLD);
static constexpr u32 LC_CODE_SIGNATURE = 0x1d;
static constexpr u32 LC_SEGMENT_SPLIT_INFO = 0x1e;
static constexpr u32 LC_REEXPORT_DYLIB = (0x1f | LC_REQ_DYLD);
static constexpr u32 LC_LAZY_LOAD_DYLIB = 0x20;
static constexpr u32 LC_ENCRYPTION_INFO = 0x21;
static constexpr u32 LC_DYLD_INFO = 0x22;
static constexpr u32 LC_DYLD_INFO_ONLY = (0x22 | LC_REQ_DYLD);
static constexpr u32 LC_LOAD_UPWARD_DYLIB = (0x23 | LC_REQ_DYLD);
static constexpr u32 LC_VERSION_MIN_MACOSX = 0x24;
static constexpr u32 LC_VERSION_MIN_IPHONEOS = 0x25;
static constexpr u32 LC_FUNCTION_STARTS = 0x26;
static constexpr u32 LC_DYLD_ENVIRONMENT = 0x27;
static constexpr u32 LC_MAIN = (0x28 | LC_REQ_DYLD);
static constexpr u32 LC_DATA_IN_CODE = 0x29;
static constexpr u32 LC_SOURCE_VERSION = 0x2A;
static constexpr u32 LC_DYLIB_CODE_SIGN_DRS = 0x2B;
static constexpr u32 LC_ENCRYPTION_INFO_64 = 0x2C;
static constexpr u32 LC_LINKER_OPTION = 0x2D;
static constexpr u32 LC_LINKER_OPTIMIZATION_HINT = 0x2E;
static constexpr u32 LC_VERSION_MIN_TVOS = 0x2F;
static constexpr u32 LC_VERSION_MIN_WATCHOS = 0x30;
static constexpr u32 LC_NOTE = 0x31;
static constexpr u32 LC_BUILD_VERSION = 0x32;
static constexpr u32 LC_DYLD_EXPORTS_TRIE = (0x33 | LC_REQ_DYLD);
static constexpr u32 LC_DYLD_CHAINED_FIXUPS = (0x34 | LC_REQ_DYLD);

static constexpr u32 SG_HIGHVM = 0x1;
static constexpr u32 SG_FVMLIB = 0x2;
static constexpr u32 SG_NORELOC = 0x4;
static constexpr u32 SG_PROTECTED_VERSION_1 = 0x8;
static constexpr u32 SG_READ_ONLY = 0x10;

static constexpr u32 S_REGULAR = 0x0;
static constexpr u32 S_ZEROFILL = 0x1;
static constexpr u32 S_CSTRING_LITERALS = 0x2;
static constexpr u32 S_4BYTE_LITERALS = 0x3;
static constexpr u32 S_8BYTE_LITERALS = 0x4;
static constexpr u32 S_LITERAL_POINTERS = 0x5;
static constexpr u32 S_NON_LAZY_SYMBOL_POINTERS = 0x6;
static constexpr u32 S_LAZY_SYMBOL_POINTERS = 0x7;
static constexpr u32 S_SYMBOL_STUBS = 0x8;
static constexpr u32 S_MOD_INIT_FUNC_POINTERS = 0x9;
static constexpr u32 S_MOD_TERM_FUNC_POINTERS = 0xa;
static constexpr u32 S_COALESCED = 0xb;
static constexpr u32 S_GB_ZEROFILL = 0xc;
static constexpr u32 S_INTERPOSING = 0xd;
static constexpr u32 S_16BYTE_LITERALS = 0xe;
static constexpr u32 S_DTRACE_DOF = 0xf;
static constexpr u32 S_LAZY_DYLIB_SYMBOL_POINTERS = 0x10;
static constexpr u32 S_THREAD_LOCAL_REGULAR = 0x11;
static constexpr u32 S_THREAD_LOCAL_ZEROFILL = 0x12;
static constexpr u32 S_THREAD_LOCAL_VARIABLES = 0x13;
static constexpr u32 S_THREAD_LOCAL_VARIABLE_POINTERS = 0x14;
static constexpr u32 S_THREAD_LOCAL_INIT_FUNCTION_POINTERS = 0x15;
static constexpr u32 S_INIT_FUNC_OFFSETS = 0x16;

static constexpr u32 S_ATTR_LOC_RELOC = 0x1;
static constexpr u32 S_ATTR_EXT_RELOC = 0x2;
static constexpr u32 S_ATTR_SOME_INSTRUCTIONS = 0x4;

static constexpr u32 S_ATTR_DEBUG = 0x20000;
static constexpr u32 S_ATTR_SELF_MODIFYING_CODE = 0x40000;
static constexpr u32 S_ATTR_LIVE_SUPPORT = 0x80000;
static constexpr u32 S_ATTR_NO_DEAD_STRIP = 0x100000;
static constexpr u32 S_ATTR_STRIP_STATIC_SYMS = 0x200000;
static constexpr u32 S_ATTR_NO_TOC = 0x400000;
static constexpr u32 S_ATTR_PURE_INSTRUCTIONS = 0x800000;

static constexpr u32 CPU_TYPE_X86_64 = 0x1000007;
static constexpr u32 CPU_TYPE_ARM64 = 0x100000c;

static constexpr u32 CPU_SUBTYPE_X86_64_ALL = 3;
static constexpr u32 CPU_SUBTYPE_ARM64_ALL = 0;

static constexpr u32 REBASE_TYPE_POINTER = 1;
static constexpr u32 REBASE_TYPE_TEXT_ABSOLUTE32 = 2;
static constexpr u32 REBASE_TYPE_TEXT_PCREL32 = 3;

static constexpr u32 REBASE_OPCODE_MASK = 0xf0;
static constexpr u32 REBASE_IMMEDIATE_MASK = 0x0f;
static constexpr u32 REBASE_OPCODE_DONE = 0x00;
static constexpr u32 REBASE_OPCODE_SET_TYPE_IMM = 0x10;
static constexpr u32 REBASE_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB = 0x20;
static constexpr u32 REBASE_OPCODE_ADD_ADDR_ULEB = 0x30;
static constexpr u32 REBASE_OPCODE_ADD_ADDR_IMM_SCALED = 0x40;
static constexpr u32 REBASE_OPCODE_DO_REBASE_IMM_TIMES = 0x50;
static constexpr u32 REBASE_OPCODE_DO_REBASE_ULEB_TIMES = 0x60;
static constexpr u32 REBASE_OPCODE_DO_REBASE_ADD_ADDR_ULEB = 0x70;
static constexpr u32 REBASE_OPCODE_DO_REBASE_ULEB_TIMES_SKIPPING_ULEB = 0x80;

static constexpr u32 BIND_TYPE_POINTER = 1;
static constexpr u32 BIND_TYPE_TEXT_ABSOLUTE32 = 2;
static constexpr u32 BIND_TYPE_TEXT_PCREL32 = 3;

static constexpr i32 BIND_SPECIAL_DYLIB_SELF = 0;
static constexpr i32 BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE = -1;
static constexpr i32 BIND_SPECIAL_DYLIB_FLAT_LOOKUP = -2;
static constexpr i32 BIND_SPECIAL_DYLIB_WEAK_LOOKUP = -3;

static constexpr u32 BIND_SYMBOL_FLAGS_WEAK_IMPORT = 0x1;
static constexpr u32 BIND_SYMBOL_FLAGS_NON_WEAK_DEFINITION = 0x8;

static constexpr u32 BIND_OPCODE_MASK = 0xF0;
static constexpr u32 BIND_IMMEDIATE_MASK = 0x0F;
static constexpr u32 BIND_OPCODE_DONE = 0x00;
static constexpr u32 BIND_OPCODE_SET_DYLIB_ORDINAL_IMM = 0x10;
static constexpr u32 BIND_OPCODE_SET_DYLIB_ORDINAL_ULEB = 0x20;
static constexpr u32 BIND_OPCODE_SET_DYLIB_SPECIAL_IMM = 0x30;
static constexpr u32 BIND_OPCODE_SET_SYMBOL_TRAILING_FLAGS_IMM = 0x40;
static constexpr u32 BIND_OPCODE_SET_TYPE_IMM = 0x50;
static constexpr u32 BIND_OPCODE_SET_ADDEND_SLEB = 0x60;
static constexpr u32 BIND_OPCODE_SET_SEGMENT_AND_OFFSET_ULEB = 0x70;
static constexpr u32 BIND_OPCODE_ADD_ADDR_ULEB = 0x80;
static constexpr u32 BIND_OPCODE_DO_BIND = 0x90;
static constexpr u32 BIND_OPCODE_DO_BIND_ADD_ADDR_ULEB = 0xA0;
static constexpr u32 BIND_OPCODE_DO_BIND_ADD_ADDR_IMM_SCALED = 0xB0;
static constexpr u32 BIND_OPCODE_DO_BIND_ULEB_TIMES_SKIPPING_ULEB = 0xC0;
static constexpr u32 BIND_OPCODE_THREADED = 0xD0;
static constexpr u32 BIND_SUBOPCODE_THREADED_SET_BIND_ORDINAL_TABLE_SIZE_ULEB = 0x00;
static constexpr u32 BIND_SUBOPCODE_THREADED_APPLY = 0x01;

static constexpr u32 EXPORT_SYMBOL_FLAGS_KIND_MASK = 0x03;
static constexpr u32 EXPORT_SYMBOL_FLAGS_KIND_REGULAR = 0x00;
static constexpr u32 EXPORT_SYMBOL_FLAGS_KIND_THREAD_LOCAL = 0x01;
static constexpr u32 EXPORT_SYMBOL_FLAGS_KIND_ABSOLUTE = 0x02;
static constexpr u32 EXPORT_SYMBOL_FLAGS_WEAK_DEFINITION = 0x04;
static constexpr u32 EXPORT_SYMBOL_FLAGS_REEXPORT = 0x08;
static constexpr u32 EXPORT_SYMBOL_FLAGS_STUB_AND_RESOLVER = 0x10;

static constexpr u32 DICE_KIND_DATA = 1;
static constexpr u32 DICE_KIND_JUMP_TABLE8 = 2;
static constexpr u32 DICE_KIND_JUMP_TABLE16 = 3;
static constexpr u32 DICE_KIND_JUMP_TABLE32 = 4;
static constexpr u32 DICE_KIND_ABS_JUMP_TABLE32 = 5;

static constexpr u32 N_UNDF = 0;
static constexpr u32 N_ABS = 1;
static constexpr u32 N_INDR = 5;
static constexpr u32 N_PBUD = 6;
static constexpr u32 N_SECT = 7;

static constexpr u32 N_GSYM = 0x20;
static constexpr u32 N_FNAME = 0x22;
static constexpr u32 N_FUN = 0x24;
static constexpr u32 N_STSYM = 0x26;
static constexpr u32 N_LCSYM = 0x28;
static constexpr u32 N_BNSYM = 0x2e;
static constexpr u32 N_AST = 0x32;
static constexpr u32 N_OPT = 0x3c;
static constexpr u32 N_RSYM = 0x40;
static constexpr u32 N_SLINE = 0x44;
static constexpr u32 N_ENSYM = 0x4e;
static constexpr u32 N_SSYM = 0x60;
static constexpr u32 N_SO = 0x64;
static constexpr u32 N_OSO = 0x66;
static constexpr u32 N_LSYM = 0x80;
static constexpr u32 N_BINCL = 0x82;
static constexpr u32 N_SOL = 0x84;
static constexpr u32 N_PARAMS = 0x86;
static constexpr u32 N_VERSION = 0x88;
static constexpr u32 N_OLEVEL = 0x8A;
static constexpr u32 N_PSYM = 0xa0;
static constexpr u32 N_EINCL = 0xa2;
static constexpr u32 N_ENTRY = 0xa4;
static constexpr u32 N_LBRAC = 0xc0;
static constexpr u32 N_EXCL = 0xc2;
static constexpr u32 N_RBRAC = 0xe0;
static constexpr u32 N_BCOMM = 0xe2;
static constexpr u32 N_ECOMM = 0xe4;
static constexpr u32 N_ECOML = 0xe8;
static constexpr u32 N_LENG = 0xfe;
static constexpr u32 N_PC = 0x30;

static constexpr u32 REFERENCE_TYPE = 0xf;
static constexpr u32 REFERENCE_FLAG_UNDEFINED_NON_LAZY = 0;
static constexpr u32 REFERENCE_FLAG_UNDEFINED_LAZY = 1;
static constexpr u32 REFERENCE_FLAG_DEFINED = 2;
static constexpr u32 REFERENCE_FLAG_PRIVATE_DEFINED = 3;
static constexpr u32 REFERENCE_FLAG_PRIVATE_UNDEFINED_NON_LAZY = 4;
static constexpr u32 REFERENCE_FLAG_PRIVATE_UNDEFINED_LAZY = 5;

static constexpr u32 REFERENCED_DYNAMICALLY = 0x0010;

static constexpr u32 SELF_LIBRARY_ORDINAL = 0x0;
static constexpr u32 MAX_LIBRARY_ORDINAL = 0xfd;
static constexpr u32 DYNAMIC_LOOKUP_ORDINAL = 0xfe;
static constexpr u32 EXECUTABLE_ORDINAL = 0xff;

static constexpr u32 N_NO_DEAD_STRIP = 0x0020;
static constexpr u32 N_DESC_DISCARDED = 0x0020;
static constexpr u32 N_WEAK_REF = 0x0040;
static constexpr u32 N_WEAK_DEF = 0x0080;
static constexpr u32 N_REF_TO_WEAK = 0x0080;
static constexpr u32 N_ARM_THUMB_DEF = 0x0008;
static constexpr u32 N_SYMBOL_RESOLVER = 0x0100;
static constexpr u32 N_ALT_ENTRY = 0x0200;

static constexpr u32 PLATFORM_MACOS = 1;
static constexpr u32 PLATFORM_IOS = 2;
static constexpr u32 PLATFORM_TVOS = 3;
static constexpr u32 PLATFORM_WATCHOS = 4;
static constexpr u32 PLATFORM_BRIDGEOS = 5;
static constexpr u32 PLATFORM_MACCATALYST = 6;
static constexpr u32 PLATFORM_IOSSIMULATOR = 7;
static constexpr u32 PLATFORM_TVOSSIMULATOR = 8;
static constexpr u32 PLATFORM_WATCHOSSIMULATOR = 9;
static constexpr u32 PLATFORM_DRIVERKIT = 10;

static constexpr u32 TOOL_CLANG = 1;
static constexpr u32 TOOL_SWIFT = 2;
static constexpr u32 TOOL_LD = 3;
static constexpr u32 TOOL_MOLD = 54321; // Randomly chosen!

static constexpr u32 OBJC_IMAGE_SUPPORTS_GC = 1 << 1;
static constexpr u32 OBJC_IMAGE_REQUIRES_GC = 1 << 2;
static constexpr u32 OBJC_IMAGE_OPTIMIZED_BY_DYLD = 1 << 3;
static constexpr u32 OBJC_IMAGE_SUPPORTS_COMPACTION = 1 << 4;
static constexpr u32 OBJC_IMAGE_IS_SIMULATED = 1 << 5;
static constexpr u32 OBJC_IMAGE_HAS_CATEGORY_CLASS_PROPERTIES = 1 << 6;

static constexpr u32 LOH_ARM64_ADRP_ADRP = 1;
static constexpr u32 LOH_ARM64_ADRP_LDR = 2;
static constexpr u32 LOH_ARM64_ADRP_ADD_LDR = 3;
static constexpr u32 LOH_ARM64_ADRP_LDR_GOT_LDR = 4;
static constexpr u32 LOH_ARM64_ADRP_ADD_STR = 5;
static constexpr u32 LOH_ARM64_ADRP_LDR_GOT_STR = 6;
static constexpr u32 LOH_ARM64_ADRP_ADD = 7;
static constexpr u32 LOH_ARM64_ADRP_LDR_GOT = 8;

static constexpr u32 ARM64_RELOC_UNSIGNED = 0;
static constexpr u32 ARM64_RELOC_SUBTRACTOR = 1;
static constexpr u32 ARM64_RELOC_BRANCH26 = 2;
static constexpr u32 ARM64_RELOC_PAGE21 = 3;
static constexpr u32 ARM64_RELOC_PAGEOFF12 = 4;
static constexpr u32 ARM64_RELOC_GOT_LOAD_PAGE21 = 5;
static constexpr u32 ARM64_RELOC_GOT_LOAD_PAGEOFF12 = 6;
static constexpr u32 ARM64_RELOC_POINTER_TO_GOT = 7;
static constexpr u32 ARM64_RELOC_TLVP_LOAD_PAGE21 = 8;
static constexpr u32 ARM64_RELOC_TLVP_LOAD_PAGEOFF12 = 9;
static constexpr u32 ARM64_RELOC_ADDEND = 10;

template <>
inline std::string rel_to_string<ARM64>(u8 type) {
  switch (type) {
  case ARM64_RELOC_UNSIGNED: return "ARM64_RELOC_UNSIGNED";
  case ARM64_RELOC_SUBTRACTOR: return "ARM64_RELOC_SUBTRACTOR";
  case ARM64_RELOC_BRANCH26: return "ARM64_RELOC_BRANCH26";
  case ARM64_RELOC_PAGE21: return "ARM64_RELOC_PAGE21";
  case ARM64_RELOC_PAGEOFF12: return "ARM64_RELOC_PAGEOFF12";
  case ARM64_RELOC_GOT_LOAD_PAGE21: return "ARM64_RELOC_GOT_LOAD_PAGE21";
  case ARM64_RELOC_GOT_LOAD_PAGEOFF12: return "ARM64_RELOC_GOT_LOAD_PAGEOFF12";
  case ARM64_RELOC_POINTER_TO_GOT: return "ARM64_RELOC_POINTER_TO_GOT";
  case ARM64_RELOC_TLVP_LOAD_PAGE21: return "ARM64_RELOC_TLVP_LOAD_PAGE21";
  case ARM64_RELOC_TLVP_LOAD_PAGEOFF12: return "ARM64_RELOC_TLVP_LOAD_PAGEOFF12";
  case ARM64_RELOC_ADDEND: return "ARM64_RELOC_ADDEND";
  }
  return "unknown (" + std::to_string(type) + ")";
}

static constexpr u32 X86_64_RELOC_UNSIGNED = 0;
static constexpr u32 X86_64_RELOC_SIGNED = 1;
static constexpr u32 X86_64_RELOC_BRANCH = 2;
static constexpr u32 X86_64_RELOC_GOT_LOAD = 3;
static constexpr u32 X86_64_RELOC_GOT = 4;
static constexpr u32 X86_64_RELOC_SUBTRACTOR = 5;
static constexpr u32 X86_64_RELOC_SIGNED_1 = 6;
static constexpr u32 X86_64_RELOC_SIGNED_2 = 7;
static constexpr u32 X86_64_RELOC_SIGNED_4 = 8;
static constexpr u32 X86_64_RELOC_TLV = 9;

template <>
inline std::string rel_to_string<X86_64>(u8 type) {
  switch (type) {
  case X86_64_RELOC_UNSIGNED: return "X86_64_RELOC_UNSIGNED";
  case X86_64_RELOC_SIGNED: return "X86_64_RELOC_SIGNED";
  case X86_64_RELOC_BRANCH: return "X86_64_RELOC_BRANCH";
  case X86_64_RELOC_GOT_LOAD: return "X86_64_RELOC_GOT_LOAD";
  case X86_64_RELOC_GOT: return "X86_64_RELOC_GOT";
  case X86_64_RELOC_SUBTRACTOR: return "X86_64_RELOC_SUBTRACTOR";
  case X86_64_RELOC_SIGNED_1: return "X86_64_RELOC_SIGNED_1";
  case X86_64_RELOC_SIGNED_2: return "X86_64_RELOC_SIGNED_2";
  case X86_64_RELOC_SIGNED_4: return "X86_64_RELOC_SIGNED_4";
  case X86_64_RELOC_TLV: return "X86_64_RELOC_TLV";
  }
  return "unknown (" + std::to_string(type) + ")";
}

struct FatHeader {
  ub32 magic;
  ub32 nfat_arch;
};

struct FatArch {
  ub32 cputype;
  ub32 cpusubtype;
  ub32 offset;
  ub32 size;
  ub32 align;
};

struct MachHeader {
  ul32 magic;
  ul32 cputype;
  ul32 cpusubtype;
  ul32 filetype;
  ul32 ncmds;
  ul32 sizeofcmds;
  ul32 flags;
  ul32 reserved;
};

struct LoadCommand {
  ul32 cmd;
  ul32 cmdsize;
};

struct SegmentCommand {
  std::string_view get_segname() const {
    return {segname, strnlen(segname, sizeof(segname))};
  }

  ul32 cmd;
  ul32 cmdsize;
  char segname[16];
  ul64 vmaddr;
  ul64 vmsize;
  ul64 fileoff;
  ul64 filesize;
  ul32 maxprot;
  ul32 initprot;
  ul32 nsects;
  ul32 flags;
};

struct MachSection {
  void set_segname(std::string_view name) {
    assert(name.size() <= sizeof(segname));
    memcpy(segname, name.data(), name.size());
  }

  std::string_view get_segname() const {
    return {segname, strnlen(segname, sizeof(segname))};
  }

  void set_sectname(std::string_view name) {
    assert(name.size() <= sizeof(sectname));
    memcpy(sectname, name.data(), name.size());
  }

  std::string_view get_sectname() const {
    return {sectname, strnlen(sectname, sizeof(sectname))};
  }

  bool match(std::string_view segname, std::string_view sectname) const {
    return get_segname() == segname && get_sectname() == sectname;
  }

  char sectname[16];
  char segname[16];
  ul64 addr;
  ul64 size;
  ul32 offset;
  ul32 p2align;
  ul32 reloff;
  ul32 nreloc;
  u8 type;
  ul24 attr;
  ul32 reserved1;
  ul32 reserved2;
  ul32 reserved3;
};

struct DylibCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 nameoff;
  ul32 timestamp;
  ul32 current_version;
  ul32 compatibility_version;
};

struct DylinkerCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 nameoff;
};

struct SymtabCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 symoff;
  ul32 nsyms;
  ul32 stroff;
  ul32 strsize;
};

struct DysymtabCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 ilocalsym;
  ul32 nlocalsym;
  ul32 iextdefsym;
  ul32 nextdefsym;
  ul32 iundefsym;
  ul32 nundefsym;
  ul32 tocoff;
  ul32 ntoc;
  ul32 modtaboff;
  ul32 nmodtab;
  ul32 extrefsymoff;
  ul32 nextrefsyms;
  ul32 indirectsymoff;
  ul32 nindirectsyms;
  ul32 extreloff;
  ul32 nextrel;
  ul32 locreloff;
  ul32 nlocrel;
};

struct VersionMinCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 version;
  ul32 sdk;
};

struct DyldInfoCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 rebase_off;
  ul32 rebase_size;
  ul32 bind_off;
  ul32 bind_size;
  ul32 weak_bind_off;
  ul32 weak_bind_size;
  ul32 lazy_bind_off;
  ul32 lazy_bind_size;
  ul32 export_off;
  ul32 export_size;
};

struct UUIDCommand {
  ul32 cmd;
  ul32 cmdsize;
  u8 uuid[16];
};

struct RpathCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 path_off;
};

struct LinkEditDataCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 dataoff;
  ul32 datasize;
};

struct BuildToolVersion {
  ul32 tool;
  ul32 version;
};

struct BuildVersionCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 platform;
  ul32 minos;
  ul32 sdk;
  ul32 ntools;
};

struct EntryPointCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul64 entryoff;
  ul64 stacksize;
};

struct SourceVersionCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul64 version;
};

struct DataInCodeEntry {
  ul32 offset;
  ul16 length;
  ul16 kind;
};

struct LinkerOptionCommand {
  ul32 cmd;
  ul32 cmdsize;
  ul32 count;
};

// This struct is named `n_list` on BSD and macOS.
struct MachSym {
  bool is_undef() const {
    return type == N_UNDF && !is_common();
  }

  bool is_common() const {
    return type == N_UNDF && is_extern && value;
  }

  ul32 stroff;

  union {
    u8 n_type;
    struct {
      u8 is_extern : 1;
      u8 type : 3;
      u8 is_private_extern : 1;
      u8 stab : 3;
    };
  };

  u8 sect;

  union {
    ul16 desc;
    struct {
      u8 padding;
      u8 common_p2align : 4;
    };
  };

  ul64 value;
};

// This struct is named `relocation_info` on BSD and macOS.
struct MachRel {
  ul32 offset;
  ul24 idx;
  u8 is_pcrel : 1;
  u8 p2size : 2;
  u8 is_extern : 1;
  u8 type : 4;
};

// __TEXT,__unwind_info section contents

static constexpr u32 UNWIND_SECTION_VERSION = 1;
static constexpr u32 UNWIND_SECOND_LEVEL_REGULAR = 2;
static constexpr u32 UNWIND_SECOND_LEVEL_COMPRESSED = 3;
static constexpr u32 UNWIND_PERSONALITY_MASK = 0x30000000;

struct UnwindSectionHeader {
  ul32 version;
  ul32 encoding_offset;
  ul32 encoding_count;
  ul32 personality_offset;
  ul32 personality_count;
  ul32 page_offset;
  ul32 page_count;
};

struct UnwindFirstLevelPage {
  ul32 func_addr;
  ul32 page_offset;
  ul32 lsda_offset;
};

struct UnwindSecondLevelPage {
  ul32 kind;
  ul16 page_offset;
  ul16 page_count;
  ul16 encoding_offset;
  ul16 encoding_count;
};

struct UnwindLsdaEntry {
  ul32 func_addr;
  ul32 lsda_addr;
};

struct UnwindPageEntry {
  ul24 func_addr;
  u8 encoding;
};

// __LD,__compact_unwind section contents

struct CompactUnwindEntry {
  ul64 code_start;
  ul32 code_len;
  ul32 encoding;
  ul64 personality;
  ul64 lsda;
};

// __LINKEDIT,__code_signature

static constexpr u32 CSMAGIC_EMBEDDED_SIGNATURE = 0xfade0cc0;
static constexpr u32 CS_SUPPORTSEXECSEG = 0x20400;
static constexpr u32 CSMAGIC_CODEDIRECTORY = 0xfade0c02;
static constexpr u32 CSSLOT_CODEDIRECTORY = 0;
static constexpr u32 CS_ADHOC = 0x00000002;
static constexpr u32 CS_LINKER_SIGNED = 0x00020000;
static constexpr u32 CS_EXECSEG_MAIN_BINARY = 1;
static constexpr u32 CS_HASHTYPE_SHA256 = 2;

struct CodeSignatureHeader {
  ub32 magic;
  ub32 length;
  ub32 count;
};

struct CodeSignatureBlobIndex {
  ub32 type;
  ub32 offset;
  ub32 padding;
};

struct CodeSignatureDirectory {
  ub32 magic;
  ub32 length;
  ub32 version;
  ub32 flags;
  ub32 hash_offset;
  ub32 ident_offset;
  ub32 n_special_slots;
  ub32 n_code_slots;
  ub32 code_limit;
  u8 hash_size;
  u8 hash_type;
  u8 platform;
  u8 page_size;
  ub32 spare2;
  ub32 scatter_offset;
  ub32 team_offset;
  ub32 spare3;
  ub64 code_limit64;
  ub64 exec_seg_base;
  ub64 exec_seg_limit;
  ub64 exec_seg_flags;
};

// __DATA,__objc_imageinfo
struct ObjcImageInfo {
  ul32 version = 0;
  u8 flags = 0;
  u8 swift_version = 0;
  ul16 swift_lang_version = 0;
};

struct ARM64 {
  static constexpr u32 cputype = CPU_TYPE_ARM64;
  static constexpr u32 cpusubtype = CPU_SUBTYPE_ARM64_ALL;
  static constexpr u32 page_size = 16384;
  static constexpr u32 abs_rel = ARM64_RELOC_UNSIGNED;
  static constexpr u32 stub_size = 12;
  static constexpr u32 stub_helper_hdr_size = 24;
  static constexpr u32 stub_helper_size = 12;
};

struct X86_64 {
  static constexpr u32 cputype = CPU_TYPE_X86_64;
  static constexpr u32 cpusubtype = CPU_SUBTYPE_X86_64_ALL;
  static constexpr u32 page_size = 4096;
  static constexpr u32 abs_rel = X86_64_RELOC_UNSIGNED;
  static constexpr u32 stub_size = 6;
  static constexpr u32 stub_helper_hdr_size = 16;
  static constexpr u32 stub_helper_size = 10;
};

static constexpr size_t word_size = 8;

} // namespace mold::macho
