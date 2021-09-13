#pragma once

#include <cstdint>

namespace mold::macho {

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

typedef int8_t i8;
typedef int16_t i16;
typedef int32_t i32;
typedef int64_t i64;

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

static constexpr u32 CPU_TYPE_X86_64 = 0x1000007;

static constexpr u32 CPU_SUBTYPE_X86_64_ALL = 3;

struct MachHeader {
  u32 magic;
  u32 cputype;
  u32 cpusubtype;
  u32 filetype;
  u32 ncmds;
  u32 sizeofcmds;
  u32 flags;
  u32 reserved;
};

struct LoadCommand {
  u32 cmd;
  u32 cmdsize;
};

struct SegmentCommand {
  u32 cmd;
  u32 cmdsize;
  char segname[16];
  u64 vmaddr;
  u64 vmsize;
  u64 fileoff;
  u64 filesize;
  u32 maxprot;
  u32 initprot;
  u32 nsects;
  u32 flags;
};

struct Section {
  char sectname[16];
  char segname[16];
  u64 addr;
  u64 size;
  u32 offset;
  u32 align;
  u32 reloff;
  u32 nreloc;
  u32 flags;
  u32 reserved1;
  u32 reserved2;
  u32 reserved3;
};

struct Dylib {
  u32 nameoff;
  u32 timestamp;
  u32 current_version;
  u32 compatibility_version;
};

struct DylibCommand {
  u32 cmd;
  u32 cmdsize;
  Dylib dylib;
};

struct DylinkerCommand {
  u32 cmd;
  u32 cmdsize;
  u32 nameoff;
};

struct SymtabCommand {
  u32 cmd;
  u32 cmdsize;
  u32 symoff;
  u32 nsyms;
  u32 stroff;
  u32 strsize;
};

} // namespace mold::macho
