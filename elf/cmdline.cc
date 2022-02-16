#include "mold.h"

#include <sstream>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <unordered_set>

namespace mold::elf {

static const char helpmsg[] = R"(
Options:
  --help                      Report usage information
  -v, --version               Report version information
  -V                          Report version and target information
  -(, --start-group           Ignored
  -), --end-group             Ignored
  -C DIR, --directory DIR     Change to DIR before doing anything
  -E, --export-dynamic        Put symbols in the dynamic symbol table
    --no-export-dynamic
  -F LIBNAME, --filter LIBNAME
                              Set DT_FILTER to the specified value
  -I FILE, --dynamic-linker FILE
                              Set dynamic linker path
    --no-dynamic-linker
  -L DIR, --library-path DIR  Add DIR to library search path
  -M, --print-map             Write map file to stdout
  -N, --omagic                Do not page align data, do not make text readonly
    --no-omagic
  -O NUMBER                   Ignored
  -S, --strip-debug           Strip .debug_* sections
  -T FILE, --script FILE      Read linker script
  -X, --discard-locals        Discard temporary local symbols
  -e SYMBOL, --entry SYMBOL   Set program entry point
  -f SHLIB, --auxiliary SHLIB Set DT_AUXILIARY to the specified value
  -h LIBNAME, --soname LIBNAME
                              Set shared library name
  -l LIBNAME                  Search for a given library
  -m TARGET                   Set target
  -o FILE, --output FILE      Set output filename
  -q, --emit-relocs           Leaves relocation sections in the output
  -r, --relocatable           Generate relocatable output
  -s, --strip-all             Strip .symtab section
  -u SYMBOL, --undefined SYMBOL
                              Force to resolve SYMBOL
  --Bdynamic                  Link against shared libraries (default)
  --Bstatic                   Do not link against shared libraries
  --Bsymbolic                 Bind global symbols locally
  --Bsymbolic-functions       Bind global functions locally
  --Bno-symbolic              Cancel --Bsymbolic and --Bsymbolic-functions
  --Map FILE                  Write map file to a given file
  --allow-multiple-definition Allow multiple definitions
  --as-needed                 Only set DT_NEEDED if used
    --no-as-needed
  --build-id [none,md5,sha1,sha256,uuid,HEXSTRING]
                              Generate build ID
    --no-build-id
  --chroot DIR                Set a given path to root directory
  --color-diagnostics=[auto,always,never]
                              Use colors in diagnostics
  --color-diagnostics         Alias for --color-diagnostics=always
  --compress-debug-sections [none,zlib,zlib-gabi,zlib-gnu]
                              Compress .debug_* sections
  --dc                        Ignored
  --defsym=SYMBOL=VALUE       Define a symbol alias
  --demangle                  Demangle C++ symbols in log messages (default)
    --no-demangle
  --disable-new-dtags         Ignored
  --dp                        Ignored
  --dynamic-list              Read a list of dynamic symbols
  --eh-frame-hdr              Create .eh_frame_hdr section
    --no-eh-frame-hdr
  --enable-new-dtags          Ignored
  --exclude-libs LIB,LIB,..   Mark all symbols in given libraries hidden
  --fatal-warnings            Ignored
    --no-fatal-warnings       Ignored
  --fini SYMBOL               Call SYMBOL at unload-time
  --fork                      Spawn a child process (default)
    --no-fork
  --gc-sections               Remove unreferenced sections
    --no-gc-sections
  --gdb-index                 Ignored
  --hash-style [sysv,gnu,both]
                              Set hash style
  --icf                       Fold identical code
    --no-icf
  --image-base ADDR           Set the base address to a given value
  --init SYMBOL               Call SYMBOl at load-time
  --no-undefined              Report undefined symbols (even with --shared)
  --pack-dyn-relocs=[relr,none]
                              Pack dynamic relocations
  --perf                      Print performance statistics
  --pie, --pic-executable     Create a position independent executable
    --no-pie, --no-pic-executable
  --plugin                    Ignored
  --plugin-opt                Ignored
  --pop-state                 Pop state of flags governing input file handling
  --preload
    --no-preload
  --print-gc-sections         Print removed unreferenced sections
    --no-print-gc-sections
  --print-icf-sections        Print folded identical sections
    --no-print-icf-sections
  --push-state                Pop state of flags governing input file handling
  --quick-exit                Use quick_exit to exit (default)
    --no-quick-exit
  --relax                     Optimize instructions (default)
    --no-relax
  --repro                     Embed input files to .repro section
  --require-defined SYMBOL    Require SYMBOL be defined in the final output
  --retain-symbols-file FILE  Keep only symbols listed in FILE
  --rpath DIR                 Add DIR to runtime search path
  --rpath-link DIR            Ignored
  --run COMMAND ARG...        Run COMMAND with mold as /usr/bin/ld
  --shared, --Bshareable      Create a share library
  --shuffle-sections[=SEED]   Randomize the output by shuffling input sections
  --sort-common               Ignored
  --sort-section              Ignored
  --spare-dynamic-tags NUMBER Reserve give number of tags in .dynamic section
  --start-lib                 Give following object files in-archive-file semantics
    --end-lib                 End the effect of --start-lib
  --static                    Do not link against shared libraries
  --stats                     Print input statistics
  --sysroot DIR               Set target system root directory
  --thread-count COUNT, --threads=COUNT
                              Use COUNT number of threads
  --threads                   Use multiple threads (default)
    --no-threads
  --trace                     Print name of each input file
  --unique PATTERN            Don't merge input sections that match a given pattern
  --unresolved-symbols [report-all,ignore-all,ignore-in-object-files,ignore-in-shared-libs]
                              How to handle unresolved symbols
  --version-script FILE       Read version script
  --warn-common               Warn about common symbols
    --no-warn-common
  --warn-once                 Only warn once for each undefined symbol
  --warn-textrel              Warn if the output file needs text relocations
  --warn-unresolved-symbols   Report unresolved symbols as warnings
    --error-unresolved-symbols
                              Report unresolved symbols as errors (default)
  --whole-archive             Include all objects from static archives
    --no-whole-archive
  --wrap SYMBOL               Use wrapper function for a given symbol
  -z defs                     Report undefined symbols (even with --shared)
    -z nodefs
  -z common-page-size=VALUE   Ignored
  -z execstack                Require executable stack
    -z noexecstack
  -z initfirst                Mark DSO to be initialized first at runtime
  -z interpose                Mark object to interpose all DSOs but executable
  -z keep-text-section-prefix Keep .text.{hot,unknown,unlikely,startup,exit} as separate sections in the final binary
    -z nokeep-text-section-prefix
  -z lazy                     Enable lazy function resolution (default)
  -z max-page-size=VALUE      Use VALUE as the memory page size
  -z nocopyreloc              Do not create copy relocations
  -z nodefaultlib             Make the dynamic loader to ignore default search paths
  -z nodelete                 Mark DSO non-deletable at runtime
  -z nodlopen                 Mark DSO not available to dlopen
  -z nodump                   Mark DSO not available to dldump
  -z now                      Disable lazy function resolution
  -z origin                   Mark object requiring immediate $ORIGIN processing at runtime
  -z separate-loadable-segments
                              Separate all loadable segments to different pages
    -z separate-code          Separate code and data into different pages
    -z noseparate-code        Allow overlap in pages
  -z relro                    Make some sections read-only after relocation (default)
    -z norelro
  -z text                     Report error if DT_TEXTREL is set
    -z notext
    -z textoff

mold: supported targets: elf32-i386 elf64-x86-64 elf64-littleaarch64
mold: supported emulations: elf_i386 elf_x86_64 aarch64linux aarch64elf)";

static std::vector<std::string> add_dashes(std::string name) {
  // Multi-letter linker options can be preceded by either a single
  // dash or double dashes except ones starting with "o", which must
  // be preceded by double dashes. For example, "-omagic" is
  // interpreted as "-o magic". If you really want to specify the
  // "omagic" option, you have to pass "--omagic".
  if (name[0] == 'o')
    return {"--" + name};
  return {"-" + name, "--" + name};
}

template <typename E>
bool read_arg(Context<E> &ctx, std::span<std::string_view> &args,
              std::string_view &arg, std::string name) {
  if (name.size() == 1) {
    if (args[0] == "-" + name) {
      if (args.size() == 1)
        Fatal(ctx) << "option -" << name << ": argument missing";
      arg = args[1];
      args = args.subspan(2);
      return true;
    }

    if (args[0].starts_with("-" + name)) {
      arg = args[0].substr(name.size() + 1);
      args = args.subspan(1);
      return true;
    }
    return false;
  }

  for (std::string opt : add_dashes(name)) {
    if (args[0] == opt) {
      if (args.size() == 1)
        Fatal(ctx) << "option -" << name << ": argument missing";
      arg = args[1];
      args = args.subspan(2);
      return true;
    }

    if (args[0].starts_with(opt + "=")) {
      arg = args[0].substr(opt.size() + 1);
      args = args.subspan(1);
      return true;
    }
  }
  return false;
}

bool read_flag(std::span<std::string_view> &args, std::string name) {
  for (std::string opt : add_dashes(name)) {
    if (args[0] == opt) {
      args = args.subspan(1);
      return true;
    }
  }
  return false;
}

static bool read_z_flag(std::span<std::string_view> &args, std::string name) {
  if (args.size() >= 2 && args[0] == "-z" && args[1] == name) {
    args = args.subspan(2);
    return true;
  }

  if (!args.empty() && args[0] == "-z" + name) {
    args = args.subspan(1);
    return true;
  }

  return false;
}

template <typename E>
bool read_z_arg(Context<E> &ctx, std::span<std::string_view> &args,
                std::string_view &arg, std::string name) {
  if (args.size() >= 2 && args[0] == "-z" && args[1].starts_with(name + "=")) {
    arg = args[1].substr(name.size() + 1);
    args = args.subspan(2);
    return true;
  }

  if (!args.empty() && args[0].starts_with("-z" + name + "=")) {
    arg = args[0].substr(name.size() + 3);
    args = args.subspan(1);
    return true;
  }

  return false;
}

template <typename E>
static i64 parse_hex(Context<E> &ctx, std::string opt, std::string_view value) {
  if (!value.starts_with("0x") && !value.starts_with("0X"))
    Fatal(ctx) << "option -" << opt << ": not a hexadecimal number";
  value = value.substr(2);
  if (value.find_first_not_of("0123456789abcdefABCDEF") != value.npos)
    Fatal(ctx) << "option -" << opt << ": not a hexadecimal number";
  return std::stol(std::string(value), nullptr, 16);
}

template <typename E>
static i64 parse_number(Context<E> &ctx, std::string opt,
                        std::string_view value) {
  size_t nread;
  i64 ret = std::stol(std::string(value), &nread, 0);
  if (value.size() != nread)
    Fatal(ctx) << "option -" << opt << ": not a number: " << value;
  return ret;
}

template <typename E>
static std::vector<u8> parse_hex_build_id(Context<E> &ctx,
                                          std::string_view arg) {
  assert(arg.starts_with("0x") || arg.starts_with("0X"));

  if (arg.size() % 2)
    Fatal(ctx) << "invalid build-id: " << arg;
  if (arg.substr(2).find_first_not_of("0123456789abcdefABCDEF") != arg.npos)
    Fatal(ctx) << "invalid build-id: " << arg;

  arg = arg.substr(2);

  auto fn = [](char c) {
    if ('0' <= c && c <= '9')
      return c - '0';
    if ('a' <= c && c <= 'f')
      return c - 'a' + 10;
    assert('A' <= c && c <= 'F');
    return c - 'A' + 10;
  };

  std::vector<u8> vec(arg.size() / 2);
  for (i64 i = 0; i < vec.size(); i++)
    vec[i] = (fn(arg[i * 2]) << 4) | fn(arg[i * 2 + 1]);
  return vec;
}

static std::vector<std::string_view>
split_by_comma_or_colon(std::string_view str) {
  std::vector<std::string_view> vec;

  for (;;) {
    i64 pos = str.find_first_of(",:");
    if (pos == str.npos) {
      vec.push_back(str);
      break;
    }
    vec.push_back(str.substr(0, pos));
    str = str.substr(pos);
  }
  return vec;
}

static std::string_view trim(std::string_view str) {
  size_t pos = str.find_first_not_of(" \t");
  if (pos == str.npos)
    return "";
  str = str.substr(pos);

  pos = str.find_last_not_of(" \t");
  if (pos == str.npos)
    return str;
  return str.substr(0, pos + 1);
}

template <typename E>
static void read_retain_symbols_file(Context<E> &ctx, std::string_view path) {
  MappedFile<Context<E>> *mf =
    MappedFile<Context<E>>::must_open(ctx, std::string(path));
  std::string_view data((char *)mf->data, mf->size);

  ctx.arg.retain_symbols_file.reset(new std::unordered_set<std::string_view>);

  while (!data.empty()) {
    size_t pos = data.find('\n');
    std::string_view name;

    if (pos == data.npos) {
      name = data;
      data = "";
    } else {
      name = data.substr(0, pos);
      data = data.substr(pos + 1);
    }

    name = trim(name);
    if (!name.empty())
      ctx.arg.retain_symbols_file->insert(name);
  }
}

static bool is_file(std::string_view path) {
  struct stat st;
  return stat(std::string(path).c_str(), &st) == 0 &&
         (st.st_mode & S_IFMT) != S_IFDIR;
}

// Returns a PLT header size and a PLT entry size.
template <typename E>
static std::pair<i64, i64> get_plt_size(Context<E> &ctx) {
  switch (E::e_machine) {
  case EM_X86_64:
    if (ctx.arg.z_now)
      return {0, 8};
    if (ctx.arg.z_ibtplt)
      return {16, 24};
    return {16, 16};
  case EM_386:
    return {16, 16};
  case EM_AARCH64:
    return {32, 16};
  case EM_RISCV:
    return {32, 16};
  }
  unreachable();
}

template <typename E>
void parse_nonpositional_args(Context<E> &ctx,
                              std::vector<std::string_view> &remaining) {
  std::span<std::string_view> args = ctx.cmdline_args;
  args = args.subspan(1);

  ctx.arg.color_diagnostics = isatty(STDERR_FILENO);
  ctx.page_size = E::page_size;

  bool version_shown = false;

  // RISC-V object files contains lots of local symbols, so by default
  // we discard them. This is compatible with GNU ld.
  if (E::e_machine == EM_RISCV)
    ctx.arg.discard_locals = true;

  while (!args.empty()) {
    std::string_view arg;

    if (read_flag(args, "help")) {
      SyncOut(ctx) << "Usage: " << ctx.cmdline_args[0]
                   << " [options] file...\n" << helpmsg;
      exit(0);
    }

    if (read_arg(ctx, args, arg, "o") || read_arg(ctx, args, arg, "output")) {
      ctx.arg.output = arg;
    } else if (read_arg(ctx, args, arg, "dynamic-linker") ||
               read_arg(ctx, args, arg, "I")) {
      ctx.arg.dynamic_linker = arg;
    } else if (read_flag(args, "no-dynamic-linker")) {
      ctx.arg.dynamic_linker = "";
    } else if (read_flag(args, "v")) {
      SyncOut(ctx) << mold_version;
      version_shown = true;
    } else if (read_flag(args, "version")) {
      SyncOut(ctx) << mold_version;
      exit(0);
    } else if (read_flag(args, "V")) {
      SyncOut(ctx) << mold_version
                   << "\n  Supported emulations:\n   elf_x86_64\n   elf_i386";
      version_shown = true;
    } else if (read_arg(ctx, args, arg, "m")) {
      if (arg == "elf_x86_64") {
        ctx.arg.emulation = EM_X86_64;
      } else if (arg == "elf_i386") {
        ctx.arg.emulation = EM_386;
      } else if (arg == "aarch64linux") {
        ctx.arg.emulation = EM_AARCH64;
      } else if (arg == "elf64lriscv") {
        ctx.arg.emulation = EM_RISCV;
      } else {
        Fatal(ctx) << "unknown -m argument: " << arg;
      }
    } else if (read_flag(args, "end-lib")) {
      remaining.push_back("-end-lib");
    } else if (read_flag(args, "export-dynamic") || read_flag(args, "E")) {
      ctx.arg.export_dynamic = true;
    } else if (read_flag(args, "no-export-dynamic")) {
      ctx.arg.export_dynamic = false;
    } else if (read_flag(args, "Bsymbolic")) {
      ctx.arg.Bsymbolic = true;
    } else if (read_flag(args, "Bsymbolic-functions")) {
      ctx.arg.Bsymbolic_functions = true;
    } else if (read_flag(args, "Bno-symbolic")) {
      ctx.arg.Bsymbolic = false;
      ctx.arg.Bsymbolic_functions = false;
    } else if (read_arg(ctx, args, arg, "exclude-libs")) {
      append(ctx.arg.exclude_libs, split_by_comma_or_colon(arg));
    } else if (read_flag(args, "q") || read_flag(args, "emit-relocs")) {
      ctx.arg.emit_relocs = true;
    } else if (read_arg(ctx, args, arg, "e") ||
               read_arg(ctx, args, arg, "entry")) {
      ctx.arg.entry = arg;
    } else if (read_arg(ctx, args, arg, "Map")) {
      ctx.arg.Map = arg;
      ctx.arg.print_map = true;
    } else if (read_flag(args, "print-dependencies")) {
      ctx.arg.print_dependencies = 1;
    } else if (read_flag(args, "print-dependencies=full")) {
      ctx.arg.print_dependencies = 2;
    } else if (read_flag(args, "print-map") || read_flag(args, "M")) {
      ctx.arg.print_map = true;
    } else if (read_flag(args, "static") || read_flag(args, "Bstatic")) {
      ctx.arg.is_static = true;
      remaining.push_back("-Bstatic");
    } else if (read_flag(args, "Bdynamic")) {
      ctx.arg.is_static = false;
      remaining.push_back("-Bdynamic");
    } else if (read_flag(args, "shared") || read_flag(args, "Bshareable")) {
      ctx.arg.shared = true;
    } else if (read_arg(ctx, args, arg, "spare-dynamic-tags")) {
      ctx.arg.spare_dynamic_tags = parse_number(ctx, "spare-dynamic-tags", arg);
    } else if (read_flag(args, "start-lib")) {
      remaining.push_back("-start-lib");
    } else if (read_arg(ctx, args, arg, "defsym")) {
      size_t pos = arg.find('=');
      if (pos == arg.npos || pos == arg.size() - 1)
        Fatal(ctx) << "-defsym: syntax error: " << arg;
      ctx.arg.defsyms.push_back({arg.substr(0, pos), arg.substr(pos + 1)});
    } else if (read_flag(args, "demangle")) {
      ctx.arg.demangle = true;
    } else if (read_flag(args, "no-demangle")) {
      ctx.arg.demangle = false;
    } else if (read_flag(args, "default-symver")) {
      ctx.arg.default_symver = true;
    } else if (read_flag(args, "shuffle-sections")) {
      ctx.arg.shuffle_sections = true;
    } else if (read_arg(ctx, args, arg, "y") ||
               read_arg(ctx, args, arg, "trace-symbol")) {
      ctx.arg.trace_symbol.push_back(arg);
    } else if (read_arg(ctx, args, arg, "filler")) {
      ctx.arg.filler = parse_hex(ctx, "filler", arg);
    } else if (read_arg(ctx, args, arg, "L") ||
               read_arg(ctx, args, arg, "library-path")) {
      ctx.arg.library_paths.push_back(std::string(arg));
    } else if (read_arg(ctx, args, arg, "sysroot")) {
      ctx.arg.sysroot = arg;
    } else if (read_arg(ctx, args, arg, "unique")) {
      std::optional<GlobPattern> pat = GlobPattern::compile(arg);
      if (!pat)
        Fatal(ctx) << "-unique: invalid glob pattern: " << arg;
      ctx.arg.unique = std::move(*pat);
    } else if (read_arg(ctx, args, arg, "unresolved-symbols")) {
      if (arg == "report-all" || arg == "ignore-in-shared-libs")
        ctx.arg.unresolved_symbols = UNRESOLVED_ERROR;
      else if (arg == "ignore-all" || arg == "ignore-in-object-files")
        ctx.arg.unresolved_symbols = UNRESOLVED_IGNORE;
      else
        Fatal(ctx) << "unknown --unresolved-symbols argument: " << arg;
    } else if (read_arg(ctx, args, arg, "u") ||
               read_arg(ctx, args, arg, "undefined")) {
      ctx.arg.undefined.push_back(arg);
    } else if (read_arg(ctx, args, arg, "require-defined")) {
      ctx.arg.require_defined.push_back(arg);
    } else if (read_arg(ctx, args, arg, "init")) {
      ctx.arg.init = arg;
    } else if (read_arg(ctx, args, arg, "fini")) {
      ctx.arg.fini = arg;
    } else if (read_arg(ctx, args, arg, "hash-style")) {
      if (arg == "sysv") {
        ctx.arg.hash_style_sysv = true;
        ctx.arg.hash_style_gnu = false;
      } else if (arg == "gnu") {
        ctx.arg.hash_style_sysv = false;
        ctx.arg.hash_style_gnu = true;
      } else if (arg == "both") {
        ctx.arg.hash_style_sysv = true;
        ctx.arg.hash_style_gnu = true;
      } else {
        Fatal(ctx) << "invalid --hash-style argument: " << arg;
      }
    } else if (read_arg(ctx, args, arg, "soname") ||
               read_arg(ctx, args, arg, "h")) {
      ctx.arg.soname = arg;
    } else if (read_flag(args, "allow-multiple-definition")) {
      ctx.arg.allow_multiple_definition = true;
    } else if (read_flag(args, "trace")) {
      ctx.arg.trace = true;
    } else if (read_flag(args, "eh-frame-hdr")) {
      ctx.arg.eh_frame_hdr = true;
    } else if (read_flag(args, "no-eh-frame-hdr")) {
      ctx.arg.eh_frame_hdr = false;
    } else if (read_flag(args, "pie") || read_flag(args, "pic-executable")) {
      ctx.arg.pic = true;
      ctx.arg.pie = true;
    } else if (read_flag(args, "no-pie") ||
               read_flag(args, "no-pic-executable")) {
      ctx.arg.pic = false;
      ctx.arg.pie = false;
    } else if (read_flag(args, "relax")) {
      ctx.arg.relax = true;
    } else if (read_flag(args, "no-relax")) {
      ctx.arg.relax = false;
    } else if (read_flag(args, "r") || read_flag(args, "relocatable")) {
      ctx.arg.relocatable = true;
    } else if (read_flag(args, "perf")) {
      ctx.arg.perf = true;
    } else if (read_flag(args, "pack-dyn-relocs=relr")) {
      ctx.arg.pack_dyn_relocs_relr = true;
    } else if (read_flag(args, "pack-dyn-relocs=none")) {
      ctx.arg.pack_dyn_relocs_relr = false;
    } else if (read_flag(args, "stats")) {
      ctx.arg.stats = true;
      Counter::enabled = true;
    } else if (read_arg(ctx, args, arg, "C") ||
               read_arg(ctx, args, arg, "directory")) {
      ctx.arg.directory = arg;
    } else if (read_arg(ctx, args, arg, "chroot")) {
      ctx.arg.chroot = arg;
    } else if (args[0] == "-color-diagnostics=auto" ||
               args[0] == "--color-diagnostics=auto") {
      ctx.arg.color_diagnostics = isatty(STDERR_FILENO);
      args = args.subspan(1);
    } else if (args[0] == "-color-diagnostics=always" ||
               args[0] == "--color-diagnostics=always") {
      ctx.arg.color_diagnostics = true;
      args = args.subspan(1);
    } else if (args[0] == "-color-diagnostics=never" ||
               args[0] == "--color-diagnostics=never") {
      ctx.arg.color_diagnostics = false;
      args = args.subspan(1);
    } else if (read_flag(args, "color-diagnostics")) {
      ctx.arg.color_diagnostics = true;
    } else if (read_flag(args, "warn-common")) {
      ctx.arg.warn_common = true;
    } else if (read_flag(args, "no-warn-common")) {
      ctx.arg.warn_common = false;
    } else if (read_flag(args, "warn-once")) {
      ctx.arg.warn_once = true;
    } else if (read_flag(args, "warn-textrel")) {
      ctx.arg.warn_textrel = true;
    } else if (read_arg(ctx, args, arg, "compress-debug-sections")) {
      if (arg == "zlib" || arg == "zlib-gabi")
        ctx.arg.compress_debug_sections = COMPRESS_GABI;
      else if (arg == "zlib-gnu")
        ctx.arg.compress_debug_sections = COMPRESS_GNU;
      else if (arg == "none")
        ctx.arg.compress_debug_sections = COMPRESS_NONE;
      else
        Fatal(ctx) << "invalid --compress-debug-sections argument: " << arg;
    } else if (read_arg(ctx, args, arg, "wrap")) {
      ctx.arg.wrap.insert(arg);
    } else if (read_flag(args, "omagic") || read_flag(args, "N")) {
      ctx.arg.omagic = true;
      ctx.arg.is_static = true;
    } else if (read_flag(args, "no-omagic")) {
      ctx.arg.omagic = false;
    } else if (read_arg(ctx, args, arg, "retain-symbols-file")) {
      read_retain_symbols_file(ctx, arg);
    } else if (read_flag(args, "repro")) {
      ctx.arg.repro = true;
    } else if (read_z_flag(args, "now")) {
      ctx.arg.z_now = true;
    } else if (read_z_flag(args, "lazy")) {
      ctx.arg.z_now = false;
    } else if (read_z_flag(args, "cet-report=none")) {
      ctx.arg.z_cet_report = CET_REPORT_NONE;
    } else if (read_z_flag(args, "cet-report=warning")) {
      ctx.arg.z_cet_report = CET_REPORT_WARNING;
    } else if (read_z_flag(args, "cet-report=error")) {
      ctx.arg.z_cet_report = CET_REPORT_ERROR;
    } else if (read_z_flag(args, "execstack")) {
      ctx.arg.z_execstack = true;
    } else if (read_z_arg(ctx, args, arg, "max-page-size")) {
      ctx.page_size = parse_number(ctx, "-z max-page-size", arg);
      if (std::popcount<u64>(ctx.page_size) != 1)
        Fatal(ctx) << "-z max-page-size " << arg << ": value must be a power of 2";
    } else if (read_z_flag(args, "noexecstack")) {
      ctx.arg.z_execstack = false;
    } else if (read_z_flag(args, "relro")) {
      ctx.arg.z_relro = true;
    } else if (read_z_flag(args, "norelro")) {
      ctx.arg.z_relro = false;
    } else if (read_z_flag(args, "defs")) {
      ctx.arg.z_defs = true;
    } else if (read_z_flag(args, "nodefs")) {
      ctx.arg.z_defs = false;
    } else if (read_z_flag(args, "nodlopen")) {
      ctx.arg.z_dlopen = false;
    } else if (read_z_flag(args, "nodelete")) {
      ctx.arg.z_delete = false;
    } else if (read_z_flag(args, "nocopyreloc")) {
      ctx.arg.z_copyreloc = false;
    } else if (read_z_flag(args, "nodump")) {
      ctx.arg.z_dump = false;
    } else if (read_z_flag(args, "initfirst")) {
      ctx.arg.z_initfirst = true;
    } else if (read_z_flag(args, "interpose")) {
      ctx.arg.z_interpose = true;
    } else if (read_z_flag(args, "ibt")) {
      ctx.arg.z_ibt = true;
      ctx.arg.z_ibtplt = true;
    } else if (read_z_flag(args, "ibtplt")) {
      ctx.arg.z_ibtplt = true;
    } else if (read_z_flag(args, "muldefs")) {
      ctx.arg.allow_multiple_definition = true;
    } else if (read_z_flag(args, "keep-text-section-prefix")) {
      ctx.arg.z_keep_text_section_prefix = true;
    } else if (read_z_flag(args, "nokeep-text-section-prefix")) {
      ctx.arg.z_keep_text_section_prefix = false;
    } else if (read_z_flag(args, "shstk")) {
      ctx.arg.z_shstk = true;
    } else if (read_z_flag(args, "text")) {
      ctx.arg.z_text = true;
    } else if (read_z_flag(args, "notext") || read_z_flag(args, "textoff")) {
      ctx.arg.z_text = false;
    } else if (read_z_flag(args, "origin")) {
      ctx.arg.z_origin = true;
    } else if (read_z_flag(args, "nodefaultlib")) {
      ctx.arg.z_nodefaultlib = true;
    } else if (read_z_flag(args, "separate-loadable-segments")) {
      ctx.arg.z_separate_code = SEPARATE_LOADABLE_SEGMENTS;
    } else if (read_z_flag(args, "separate-code")) {
      ctx.arg.z_separate_code = SEPARATE_CODE;
    } else if (read_z_flag(args, "noseparate-code")) {
      ctx.arg.z_separate_code = NOSEPARATE_CODE;
    } else if (read_flag(args, "no-undefined")) {
      ctx.arg.z_defs = true;
    } else if (read_flag(args, "fatal-warnings")) {
      ctx.arg.fatal_warnings = true;
    } else if (read_flag(args, "no-fatal-warnings")) {
      ctx.arg.fatal_warnings = false;
    } else if (read_flag(args, "fork")) {
      ctx.arg.fork = true;
    } else if (read_flag(args, "no-fork")) {
      ctx.arg.fork = false;
    } else if (read_flag(args, "gc-sections")) {
      ctx.arg.gc_sections = true;
    } else if (read_flag(args, "no-gc-sections")) {
      ctx.arg.gc_sections = false;
    } else if (read_flag(args, "print-gc-sections")) {
      ctx.arg.print_gc_sections = true;
    } else if (read_flag(args, "no-print-gc-sections")) {
      ctx.arg.print_gc_sections = false;
    } else if (read_arg(ctx, args, arg, "icf")) {
      if (arg == "all")
        ctx.arg.icf = true;
      else if (arg == "none")
        ctx.arg.icf = false;
      else
        Fatal(ctx) << "unknown --icf argument: " << arg;
    } else if (read_flag(args, "no-icf")) {
      ctx.arg.icf = false;
    } else if (read_arg(ctx, args, arg, "image-base")) {
      ctx.arg.image_base = parse_number(ctx, "image-base", arg);
    } else if (read_flag(args, "print-icf-sections")) {
      ctx.arg.print_icf_sections = true;
    } else if (read_flag(args, "no-print-icf-sections")) {
      ctx.arg.print_icf_sections = false;
    } else if (read_flag(args, "quick-exit")) {
      ctx.arg.quick_exit = true;
    } else if (read_flag(args, "no-quick-exit")) {
      ctx.arg.quick_exit = false;
    } else if (read_arg(ctx, args, arg, "plugin")) {
      ctx.arg.plugin = arg;
    } else if (read_arg(ctx, args, arg, "plugin-opt")) {
      ctx.arg.plugin_opt.push_back(arg);
    } else if (read_arg(ctx, args, arg, "thread-count")) {
      ctx.arg.thread_count = parse_number(ctx, "thread-count", arg);
    } else if (read_flag(args, "threads")) {
      ctx.arg.thread_count = 0;
    } else if (read_flag(args, "no-threads")) {
      ctx.arg.thread_count = 1;
    } else if (args[0].starts_with("-threads=")) {
      ctx.arg.thread_count = parse_number(ctx, "threads=", args[0].substr(9));
      args = args.subspan(1);
    } else if (args[0].starts_with("--threads=")) {
      ctx.arg.thread_count = parse_number(ctx, "threads=", args[0].substr(10));
      args = args.subspan(1);
    } else if (read_flag(args, "discard-all") || read_flag(args, "x")) {
      ctx.arg.discard_all = true;
    } else if (read_flag(args, "discard-locals") || read_flag(args, "X")) {
      ctx.arg.discard_locals = true;
    } else if (read_flag(args, "strip-all") || read_flag(args, "s")) {
      ctx.arg.strip_all = true;
    } else if (read_flag(args, "strip-debug") || read_flag(args, "S")) {
      ctx.arg.strip_all = true;
    } else if (read_flag(args, "warn-unresolved-symbols")) {
      ctx.arg.unresolved_symbols = UNRESOLVED_WARN;
    } else if (read_flag(args, "error-unresolved-symbols")) {
      ctx.arg.unresolved_symbols = UNRESOLVED_ERROR;
    } else if (read_arg(ctx, args, arg, "rpath")) {
      if (!ctx.arg.rpaths.empty())
        ctx.arg.rpaths += ":";
      ctx.arg.rpaths += arg;
    } else if (read_arg(ctx, args, arg, "R")) {
      if (is_file(arg))
        Fatal(ctx) << "-R" << arg
                   << ": -R as an alias for --just-symbols is not supported";

      if (!ctx.arg.rpaths.empty())
        ctx.arg.rpaths += ":";
      ctx.arg.rpaths += arg;
    } else if (read_flag(args, "build-id")) {
      ctx.arg.build_id.kind = BuildId::HASH;
      ctx.arg.build_id.hash_size = 20;
    } else if (read_arg(ctx, args, arg, "build-id")) {
      if (arg == "none") {
        ctx.arg.build_id.kind = BuildId::NONE;
      } else if (arg == "uuid") {
        ctx.arg.build_id.kind = BuildId::UUID;
      } else if (arg == "md5") {
        ctx.arg.build_id.kind = BuildId::HASH;
        ctx.arg.build_id.hash_size = 16;
      } else if (arg == "sha1") {
        ctx.arg.build_id.kind = BuildId::HASH;
        ctx.arg.build_id.hash_size = 20;
      } else if (arg == "sha256") {
        ctx.arg.build_id.kind = BuildId::HASH;
        ctx.arg.build_id.hash_size = 32;
      } else if (arg.starts_with("0x") || arg.starts_with("0X")) {
        ctx.arg.build_id.kind = BuildId::HEX;
        ctx.arg.build_id.value = parse_hex_build_id(ctx, arg);
      } else {
        Fatal(ctx) << "invalid --build-id argument: " << arg;
      }
    } else if (read_flag(args, "no-build-id")) {
      ctx.arg.build_id.kind = BuildId::NONE;
    } else if (read_arg(ctx, args, arg, "format") ||
               read_arg(ctx, args, arg, "b")) {
      if (arg == "binary")
        Fatal(ctx)
          << "mold does not suppor `-b binary`. If you want to convert a binary"
          << " file into an object file, use `objcopy -I binary -O default"
          << " <input-file> <output-file.o>` instead.";
      Fatal(ctx) << "unknown command line option: -b " << arg;
    } else if (read_arg(ctx, args, arg, "auxiliary") ||
               read_arg(ctx, args, arg, "f")) {
      ctx.arg.auxiliary.push_back(arg);
    } else if (read_arg(ctx, args, arg, "filter") ||
               read_arg(ctx, args, arg, "F")) {
      ctx.arg.filter.push_back(arg);
    } else if (read_flag(args, "preload")) {
      ctx.arg.preload = true;
    } else if (read_flag(args, "no-preload")) {
      ctx.arg.preload = false;
    } else if (read_flag(args, "apply-dynamic-relocs")) {
    } else if (read_arg(ctx, args, arg, "O")) {
    } else if (read_flag(args, "O0")) {
    } else if (read_flag(args, "O1")) {
    } else if (read_flag(args, "O2")) {
    } else if (read_flag(args, "verbose")) {
    } else if (read_flag(args, "color-diagnostics")) {
    } else if (read_flag(args, "gdb-index")) {
    } else if (read_flag(args, "eh-frame-hdr")) {
    } else if (read_flag(args, "start-group")) {
    } else if (read_flag(args, "end-group")) {
    } else if (read_flag(args, "(")) {
    } else if (read_flag(args, ")")) {
    } else if (read_flag(args, "fatal-warnings")) {
    } else if (read_flag(args, "enable-new-dtags")) {
    } else if (read_flag(args, "disable-new-dtags")) {
    } else if (read_flag(args, "nostdlib")) {
    } else if (read_flag(args, "allow-shlib-undefined")) {
    } else if (read_flag(args, "no-allow-shlib-undefined")) {
    } else if (read_flag(args, "no-add-needed")) {
    } else if (read_flag(args, "no-call-graph-profile-sort")) {
    } else if (read_flag(args, "no-copy-dt-needed-entries")) {
    } else if (read_flag(args, "no-undefined-version")) {
    } else if (read_arg(ctx, args, arg, "sort-section")) {
    } else if (read_flag(args, "sort-common")) {
    } else if (read_flag(args, "dc")) {
    } else if (read_flag(args, "dp")) {
    } else if (read_flag(args, "fix-cortex-a53-835769")) {
    } else if (read_flag(args, "fix-cortex-a53-843419")) {
    } else if (read_flag(args, "EL")) {
    } else if (read_flag(args, "warn-once")) {
    } else if (read_flag(args, "nodefaultlibs")) {
    } else if (read_flag(args, "warn-constructors")) {
    } else if (read_flag(args, "warn-execstack")) {
    } else if (read_flag(args, "no-warn-execstack")) {
    } else if (read_arg(ctx, args, arg, "rpath-link")) {
    } else if (read_z_flag(args, "combreloc")) {
    } else if (read_z_flag(args, "nocombreloc")) {
    } else if (read_z_arg(ctx, args, arg, "common-page-size")) {
    } else if (read_arg(ctx, args, arg, "version-script")) {
      remaining.push_back("--version-script");
      remaining.push_back(arg);
    } else if (read_arg(ctx, args, arg, "dynamic-list")) {
      remaining.push_back("--dynamic-list");
      remaining.push_back(arg);
    } else if (read_flag(args, "as-needed")) {
      remaining.push_back("-as-needed");
    } else if (read_flag(args, "no-as-needed")) {
      remaining.push_back("-no-as-needed");
    } else if (read_flag(args, "whole-archive")) {
      remaining.push_back("-whole-archive");
    } else if (read_flag(args, "no-whole-archive")) {
      remaining.push_back("-no-whole-archive");
    } else if (read_arg(ctx, args, arg, "l")) {
      remaining.push_back("-l");
      remaining.push_back(arg);
    } else if (read_arg(ctx, args, arg, "script") ||
               read_arg(ctx, args, arg, "T")) {
      remaining.push_back(arg);
    } else if (read_flag(args, "push-state")) {
      remaining.push_back("-push-state");
    } else if (read_flag(args, "pop-state")) {
      remaining.push_back("-pop-state");
    } else if (args[0].starts_with("-z") && args[0].size() > 2) {
      Warn(ctx) << "unknown command line option: " << args[0];
      args = args.subspan(1);
    } else if (args[0] == "-z" && args.size() >= 2) {
      Warn(ctx) << "unknown command line option: -z " << args[1];
      args = args.subspan(2);
    } else {
      if (args[0][0] == '-')
        Fatal(ctx) << "unknown command line option: " << args[0];
      remaining.push_back(args[0]);
      args = args.subspan(1);
    }
  }

  if (!ctx.arg.sysroot.empty()) {
    for (std::string &path : ctx.arg.library_paths) {
      if (std::string_view(path).starts_with('='))
        path = ctx.arg.sysroot + path.substr(1);
      else if (std::string_view(path).starts_with("$SYSROOT"))
        path = ctx.arg.sysroot + path.substr(8);
    }
  }

  if (ctx.arg.shared) {
    ctx.arg.pic = true;
    ctx.arg.dynamic_linker = "";
  }

  if (ctx.arg.pic)
    ctx.arg.image_base = 0;

  if (ctx.arg.retain_symbols_file) {
    ctx.arg.strip_all = false;
    ctx.arg.discard_all = false;
  }

  if (ctx.arg.relocatable)
    ctx.arg.is_static = true;

  if (!ctx.arg.shared) {
    if (!ctx.arg.filter.empty())
      Fatal(ctx) << "-filter may not be used without -shared";
    if (!ctx.arg.auxiliary.empty())
      Fatal(ctx) << "-auxiliary may not be used without -shared";
  }

  if (ctx.arg.image_base % ctx.page_size)
    Fatal(ctx) << "-image-base msut be a multiple of -max-page-size";

  if (char *env = getenv("MOLD_REPRO"); env && env[0])
    ctx.arg.repro = true;

  if (ctx.arg.output.empty())
    ctx.arg.output = "a.out";

  if (ctx.arg.shared || ctx.arg.export_dynamic)
    ctx.default_version = VER_NDX_GLOBAL;
  else
    ctx.default_version = VER_NDX_LOCAL;

  if (ctx.arg.default_symver) {
    std::string ver = ctx.arg.soname.empty() ?
      filepath(ctx.arg.output).filename().string() : std::string(ctx.arg.soname);
    ctx.arg.version_definitions.push_back(ver);
    ctx.default_version = VER_NDX_LAST_RESERVED + 1;
  }

  std::tie(ctx.plt_hdr_size, ctx.plt_size) = get_plt_size(ctx);

  ctx.arg.undefined.push_back(ctx.arg.entry);

  // TLSDESC relocs must be always relaxed for statically-linked
  // executables even if -no-relax is given. It is because a
  // statically-linked executable doesn't contain a tranpoline
  // function needed for TLSDESC.
  ctx.relax_tlsdesc = ctx.arg.is_static || (ctx.arg.relax && !ctx.arg.shared);

  if (version_shown && remaining.empty())
    exit(0);
}

#define INSTANTIATE(E)                                                  \
  template                                                              \
  bool read_arg(Context<E> &ctx, std::span<std::string_view> &args,     \
                std::string_view &arg,                                  \
                std::string name);                                      \
                                                                        \
  template                                                              \
  void parse_nonpositional_args(Context<E> &ctx,                        \
                                std::vector<std::string_view> &remaining)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
INSTANTIATE(ARM64);
INSTANTIATE(RISCV64);

} // namespace mold::elf
