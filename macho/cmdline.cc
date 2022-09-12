#include "mold.h"
#include "../cmdline.h"

#include <optional>
#include <regex>
#include <sstream>
#include <sys/stat.h>
#include <sys/types.h>
#include <unordered_set>

#ifndef _WIN32
# include <unistd.h>
#endif

namespace mold::macho {

static const char helpmsg[] = R"(
Options:
  -F<PATH>                    Add DIR to framework search path
  -L<PATH>                    Add DIR to library search path
  -ObjC                       Load all static archive members that implement
                              an Objective-C class or category
  -U <SYMBOL>                 Allow a symbol to be undefined
  -Z                          Do not search the standard directories when
                              searching for libraries and frameworks
  -add_ast_path <FILE>        Add a N_AST symbol with the given filename
  -add_empty_section <SEGNAME> <SECTNAME>
                              Add an empty section
  -adhoc_codesign             Add ad-hoc code signature to the output file
    -no_adhoc_codesign
  -all_load                   Include all objects from static archives
    -noall_load
  -application_extension      Verify that all dylibs are extension-safe
    -no_application_extension
  -arch <ARCH_NAME>           Specify target architecture
  -bundle                     Produce a mach-o bundle
  -bundle_loader <EXECUTABLE> Resolve undefined symbols using the given executable
  -compatibility_version <VERSION>
                              Specifies the compatibility version number of the library
  -current_version <VERSION>  Specifies the current version number of the library.
  -dead_strip                 Remove unreachable functions and data
  -dead_strip_dylibs          Remove unreachable dylibs from dependencies
  -debug_variant              Ignored
  -demangle                   Demangle C++ symbols in log messages (default)
  -dependency_info <FILE>     Ignored
  -dylib                      Produce a dynamic library
  -dylib_compatibility_version <VERSION>
                              Alias for -compatibility_version
  -dylib_current_version <VERSION>
                              Alias for -current_version
  -dylib_install_name         Alias for -install_name
  -dynamic                    Link against dylibs (default)
  -e <SYMBOL>                 Specify the entry point of a main executable
  -execute                    Produce an executable (default)
  -export_dynamic             Preserves all global symbols in main executables during LTO
  -exported_symbol <SYMBOL>   Export a given symbol
  -exported_symbols_list <FILE>
                              Read a list of exported symbols from a given file
  -filelist <FILE>[,<DIR>]    Specify the list of input file names
  -final_output <NAME>
  -force_load <FILE>          Include all objects from a given static archive
  -framework <NAME>,[,<SUFFIX>]
                              Search for a given framework
  -headerpad <SIZE>           Allocate the size of padding after load commands
  -headerpad_max_install_names
                              Allocate MAXPATHLEN byte padding after load commands
  -help                       Report usage information
  -hidden-l<LIB>
  -ignore_optimization_hints  Do not rewrite instructions as optimization (default)
    -enable_optimization_hints
  -install_name <NAME>
  -l<LIB>                     Search for a given library
  -lto_library <FILE>         Load a LTO linker plugin library
  -macos_version_min <VERSION>
  -map <FILE>                 Write map file to a given file
  -mark_dead_strippable_dylib Mark the output as dead-strippable
  -needed-l<LIB>              Search for a given library
  -needed-framework <NAME>[,<SUFFIX>]
                              Search for a given framework
  -no_deduplicate             Ignored
  -no_function_starts         Do not generate an LC_FUNCTION_STARTS load command
  -no_uuid                    Do not generate an LC_UUID load command
  -o <FILE>                   Set output filename
  -objc_abi_version <VERSION> Ignored
  -object_path_lto <FILE>     Write a LTO temporary file to a given path
  -order_file <FILE>          Layout functions and data according to specification in a given file
  -pagezero_size <SIZE>       Specify the size of the __PAGEZERO segment
  -platform_version <PLATFORM> <MIN_VERSION> <SDK_VERSION>
                              Set platform, platform version and SDK version
  -random_uuid                Generate a random LC_UUID load command
  -reexport-l<LIB>            Search for a given library
  -rpath <PATH>               Add PATH to the runpath search path list
  -search_dylibs_first
  -search_paths_first
  -sectalign <SEGNAME> <SECTNAME> <VALUE>
                              Set a section's alignment to a given value
  -sectcreate <SEGNAME> <SECTNAME> <FILE>
  -stack_size <SIZE>
  -stats                      Show statistics info
  -syslibroot <DIR>           Prepend DIR to library search paths
  -t                          Print out each file the linker loads
  -thread_count <NUMBER>      Use given number of threads
  -u <SYMBOL>                 Force load a given symbol from archive if necessary
  -unexported_symbol <SYMBOL> Export all but a given symbol
  -unexported_symbols_list <FILE>
                              Read a list of unexported symbols from a given file
  -v                          Report version information
  -weak_framework <NAME>[,<SUFFIX>]
                              Search for a given framework
  -weak-l<LIB>                Search for a given library)";

template <typename E>
static i64 parse_platform(Context<E> &ctx, std::string_view arg) {
  static std::regex re(R"(\d+)", std::regex_constants::ECMAScript);
  if (std::regex_match(arg.begin(), arg.end(), re))
    return stoi(std::string(arg));

  if (arg == "macos")
    return PLATFORM_MACOS;
  if (arg == "ios")
    return PLATFORM_IOS;
  if (arg == "tvos")
    return PLATFORM_TVOS;
  if (arg == "watchos")
    return PLATFORM_WATCHOS;
  if (arg == "bridgeos")
    return PLATFORM_BRIDGEOS;
  if (arg == "mac-catalyst")
    return PLATFORM_MACCATALYST;
  if (arg == "ios-simulator")
    return PLATFORM_IOSSIMULATOR;
  if (arg == "tvos-simulator")
    return PLATFORM_TVOSSIMULATOR;
  if (arg == "watchos-simulator")
    return PLATFORM_WATCHOSSIMULATOR;
  if (arg == "driverkit")
    return PLATFORM_DRIVERKIT;
  Fatal(ctx) << "unknown -platform_version name: " << arg;
}

template <typename E>
i64 parse_version(Context<E> &ctx, std::string_view arg) {
  static std::regex re(R"((\d+)(?:\.(\d+))?(?:\.(\d+))?)",
                       std::regex_constants::ECMAScript);
  std::cmatch m;
  if (!std::regex_match(arg.data(), arg.data() + arg.size(), m, re))
    Fatal(ctx) << "malformed version number: " << arg;

  i64 major = (m[1].length() == 0) ? 0 : stoi(m[1]);
  i64 minor = (m[2].length() == 0) ? 0 : stoi(m[2]);
  i64 patch = (m[3].length() == 0) ? 0 : stoi(m[3]);
  return (major << 16) | (minor << 8) | patch;
}

template <typename E>
i64 parse_hex(Context<E> &ctx, std::string_view arg) {
  auto flags = std::regex_constants::ECMAScript | std::regex_constants::icase;
  static std::regex re(R"((?:0x)?[0-9a-f]+)", flags);

  std::cmatch m;
  if (!std::regex_match(arg.begin(), arg.end(), re))
    Fatal(ctx) << "malformed hexadecimal number: " << arg;

  if (arg.starts_with("0x") || arg.starts_with("0X"))
    return std::stoll(std::string(arg.substr(2)), nullptr, 16);
  return std::stoll(std::string(arg), nullptr, 16);
}

static bool is_directory(std::filesystem::path path) {
  std::error_code ec;
  return std::filesystem::is_directory(path, ec) && !ec;
}

template <typename E>
static std::vector<std::string>
read_lines(Context<E> &ctx, std::string_view path) {
  MappedFile<Context<E>> *mf =
    MappedFile<Context<E>>::must_open(ctx, std::string(path));
  std::string_view data((char *)mf->data, mf->size);

  std::vector<std::string> vec;

  while (!data.empty()) {
    size_t pos = data.find('\n');
    std::string_view line;

    if (pos == data.npos) {
      line = data;
      data = "";
    } else {
      line = data.substr(0, pos);
      data = data.substr(pos + 1);
    }

    line = string_trim(line);
    if (!line.empty() && !line.starts_with('#'))
      vec.push_back(std::string(line));
  }
  return vec;
}

template <typename E>
std::vector<std::string> parse_nonpositional_args(Context<E> &ctx) {
  std::vector<std::string_view> &args = ctx.cmdline_args;
  std::vector<std::string> remaining;
  i64 i = 1;

  std::vector<std::string> framework_paths;
  std::vector<std::string> library_paths;
  bool nostdlib = false;
  bool version_shown = false;
  std::optional<i64> pagezero_size;

  while (i < args.size()) {
    std::string_view arg;
    std::string_view arg2;
    std::string_view arg3;
    u64 hex_arg;

    auto read_arg = [&](std::string name) {
      if (args[i] == name) {
        if (args.size() <= i + 1)
          Fatal(ctx) << "option -" << name << ": argument missing";
        arg = args[i + 1];
        i += 2;
        return true;
      }
      return false;
    };

    auto read_arg2 = [&](std::string name) {
      if (args[i] == name) {
        if (args.size() <= i + 2)
          Fatal(ctx) << "option -" << name << ": argument missing";
        arg = args[i + 1];
        arg2 = args[i + 2];
        i += 3;
        return true;
      }
      return false;
    };

    auto read_arg3 = [&](std::string name) {
      if (args[i] == name) {
        if (args.size() <= i + 3)
          Fatal(ctx) << "option -" << name << ": argument missing";
        arg = args[i + 1];
        arg2 = args[i + 2];
        arg3 = args[i + 3];
        i += 4;
        return true;
      }
      return false;
    };

    auto read_joined = [&](std::string name) {
      if (read_arg(name))
        return true;
      if (args[i].starts_with(name)) {
        arg = args[i].substr(name.size());
        i++;
        return true;
      }
      return false;
    };

    auto read_flag = [&](std::string name) {
      if (args[i] == name) {
        i++;
        return true;
      }
      return false;
    };

    auto read_hex = [&](std::string name) {
      if (!read_arg(name))
        return false;

      size_t pos;
      hex_arg = std::stol(std::string(arg), &pos, 16);
      if (pos != arg.size())
        Fatal(ctx) << "malformed " << name << ": " << arg;
      return true;
    };

    if (args[i].starts_with('@')) {
      std::vector<std::string_view> vec =
        read_response_file(ctx, args[i].substr(1));
      args.erase(args.begin() + i);
      args.insert(args.begin() + i, vec.begin(), vec.end());
      continue;
    }

    if (read_flag("-help") || read_flag("--help")) {
      SyncOut(ctx) << "Usage: " << ctx.cmdline_args[0]
                   << " [options] file...\n" << helpmsg;
      exit(0);
    }

    if (read_joined("-F")) {
      framework_paths.push_back(std::string(arg));
    } else if (read_joined("-L")) {
      library_paths.push_back(std::string(arg));
    } else if (read_flag("-Z")) {
      nostdlib = true;
    } else if (read_flag("-ObjC")) {
      ctx.arg.ObjC = true;
    } else if (read_arg("-U")) {
      ctx.arg.U.push_back(std::string(arg));
    } else if (read_arg("-add_ast_path")) {
      ctx.arg.add_ast_path.push_back(std::string(arg));
    } else if (read_arg2("-add_empty_section")) {
      ctx.arg.add_empty_section.push_back({arg, arg2});
    } else if (read_flag("-adhoc_codesign")) {
      ctx.arg.adhoc_codesign = true;
    } else if (read_flag("-no_adhoc_codesign")) {
      ctx.arg.adhoc_codesign = false;
    } else if (read_flag("-all_load")) {
      remaining.push_back("-all_load");
    } else if (read_flag("-noall_load")) {
      remaining.push_back("-noall_load");
    } else if (read_flag("-application_extension")) {
      ctx.arg.application_extension = true;
    } else if (read_flag("-no_application_extension")) {
      ctx.arg.application_extension = false;
    } else if (read_arg("-arch")) {
      if (arg == "x86_64")
        ctx.arg.arch = CPU_TYPE_X86_64;
      else if (arg == "arm64")
        ctx.arg.arch = CPU_TYPE_ARM64;
      else
        Fatal(ctx) << "unknown -arch: " << arg;
    } else if (read_flag("-bundle")) {
      ctx.output_type = MH_BUNDLE;
    } else if (read_arg("-bundle_loader")) {
      ctx.arg.bundle_loader = arg;
    } else if (read_arg("-compatibility_version") ||
               read_arg("-dylib_compatibility_version")) {
      ctx.arg.compatibility_version = parse_version(ctx, arg);
    } else if (read_arg("-current_version") ||
               read_arg("-dylib_current_version")) {
      ctx.arg.current_version = parse_version(ctx, arg);
    } else if (read_flag("-color-diagnostics") ||
               read_flag("--color-diagnostics")) {
      ctx.arg.color_diagnostics = true;
    } else if (read_flag("-dead_strip")) {
      ctx.arg.dead_strip = true;
    } else if (read_flag("-dead_strip_dylibs")) {
      ctx.arg.dead_strip_dylibs = true;
    } else if (read_flag("-debug_variant")) {
    } else if (read_flag("-demangle")) {
      ctx.arg.demangle = true;
    } else if (read_arg("-dependency_info")) {
      ctx.arg.dependency_info = arg;
    } else if (read_flag("-dylib")) {
      ctx.output_type = MH_DYLIB;
    } else if (read_hex("-headerpad")) {
      ctx.arg.headerpad = hex_arg;
    } else if (read_flag("-headerpad_max_install_names")) {
      ctx.arg.headerpad = 1024;
    } else if (read_flag("-dynamic")) {
      ctx.arg.dynamic = true;
    } else if (read_arg("-e")) {
      ctx.arg.entry = get_symbol(ctx, arg);
    } else if (read_flag("-execute")) {
      ctx.output_type = MH_EXECUTE;
    } else if (read_flag("-export_dynamic")) {
      ctx.arg.export_dynamic = true;
    } else if (read_arg("-exported_symbol")) {
      if (!ctx.arg.exported_symbols_list.add(arg, 1))
        Fatal(ctx) << "-exported_symbol: invalid glob pattern: " << arg;
    } else if (read_arg("-exported_symbols_list")) {
      for (std::string_view pat : read_lines(ctx, arg))
        if (!ctx.arg.exported_symbols_list.add(pat, 1))
          Fatal(ctx) << "-exported_symbols_list: " << arg
                     << ": invalid glob pattern: " << pat;
    } else if (read_arg("-fatal_warnings")) {
    } else if (read_arg("-filelist")) {
      remaining.push_back("-filelist");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-final_output")) {
      ctx.arg.final_output = arg;
    } else if (read_arg("-force_load")) {
      remaining.push_back("-force_load");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-framework")) {
      remaining.push_back("-framework");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-lto_library")) {
      ctx.arg.lto_library = arg;
    } else if (read_arg("-macos_version_min")) {
      ctx.arg.platform = PLATFORM_MACOS;
      ctx.arg.platform_min_version = parse_version(ctx, arg);
    } else if (read_joined("-hidden-l")) {
      remaining.push_back("-hidden-l");
      remaining.push_back(std::string(arg));
    } else if (read_flag("-ignore_optimization_hints")) {
      ctx.arg.ignore_optimization_hints = true;
    } else if (read_flag("-enable_optimization_hints")) {
      ctx.arg.ignore_optimization_hints = false;
    } else if (read_arg("-install_name") || read_arg("-dylib_install_name")) {
      ctx.arg.install_name = arg;
    } else if (read_joined("-l")) {
      remaining.push_back("-l");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-map")) {
      ctx.arg.map = arg;
    } else if (read_flag("-mark_dead_strippable_dylib")) {
      ctx.arg.mark_dead_strippable_dylib = true;
    } else if (read_arg("-mllvm")) {
      ctx.arg.mllvm.push_back(std::string(arg));
    } else if (read_joined("-needed-l")) {
      remaining.push_back("-needed-l");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-needed_framework")) {
      remaining.push_back("-needed_framework");
      remaining.push_back(std::string(arg));
    } else if (read_flag("-no_deduplicate")) {
    } else if (read_flag("-no_function_starts")) {
      ctx.arg.function_starts = false;
    } else if (read_flag("-no_uuid")) {
      ctx.arg.uuid = UUID_NONE;
    } else if (read_arg("-o")) {
      ctx.arg.output = arg;
    } else if (read_arg("-objc_abi_version")) {
    } else if (read_arg("-object_path_lto")) {
      ctx.arg.object_path_lto = arg;
    } else if (read_arg("-order_file")) {
      ctx.arg.order_file = read_lines(ctx, arg);
    } else if (read_hex("-pagezero_size")) {
      pagezero_size = hex_arg;
    } else if (read_flag("-perf")) {
      ctx.arg.perf = true;
    } else if (read_arg3("-platform_version")) {
      ctx.arg.platform = parse_platform(ctx, arg);
      ctx.arg.platform_min_version = parse_version(ctx, arg2);
      ctx.arg.platform_sdk_version = parse_version(ctx, arg3);
    } else if (read_flag("-quick_exit")) {
      ctx.arg.quick_exit = true;
    } else if (read_flag("-no_quick_exit")) {
      ctx.arg.quick_exit = false;
    } else if (read_flag("-random_uuid")) {
      ctx.arg.uuid = UUID_RANDOM;
    } else if (read_joined("-reexport-l")) {
      remaining.push_back("-reexport-l");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-rpath")) {
      ctx.arg.rpath.push_back(std::string(arg));
    } else if (read_flag("-search_paths_first")) {
      ctx.arg.search_paths_first = true;
    } else if (read_flag("-search_dylibs_first")) {
      ctx.arg.search_paths_first = false;
    } else if (read_arg3("-sectalign")) {
      u64 val = parse_hex(ctx, arg3);
      std::string key = std::string(arg) + "," + std::string(arg2);
      if (!has_single_bit(val))
        Fatal(ctx) << "-sectalign: invalid alignment value: " << arg3;
      ctx.arg.sectalign.push_back({arg, arg2, (u8)std::countl_zero(val)});
    } else if (read_arg3("-sectcreate")) {
      ctx.arg.sectcreate.push_back({arg, arg2, arg3});
    } else if (read_hex("-stack_size")) {
      ctx.arg.stack_size = hex_arg;
    } else if (read_flag("-stats")) {
      ctx.arg.stats = true;
      Counter::enabled = true;
    } else if (read_arg("-syslibroot")) {
      ctx.arg.syslibroot.push_back(std::string(arg));
    } else if (read_flag("-t")) {
      ctx.arg.trace = true;
    } else if (read_arg("-thread_count")) {
      ctx.arg.thread_count = std::stoi(std::string(arg));
    } else if (read_arg("-u")) {
      ctx.arg.u.push_back(std::string(arg));
    } else if (read_arg("-undefined")) {
      if (arg == "error" || arg == "dynamic_lookup") {
        if (arg == "dynamic_lookup")
          ctx.arg.undefined = DYNAMIC_LOOKUP;
      } else {
        Fatal(ctx) << "-undefined: invalid treatment: " << arg;
      }
    } else if (read_arg("-unexported_symbol")) {
      if (!ctx.arg.unexported_symbols_list.add(arg, 1))
        Fatal(ctx) << "-unexported_symbol: invalid glob pattern: " << arg;
    } else if (read_arg("-unexported_symbols_list")) {
      for (std::string_view pat : read_lines(ctx, arg))
        if (!ctx.arg.unexported_symbols_list.add(pat, 1))
          Fatal(ctx) << "-unexported_symbols_list: " << arg
                     << ": invalid glob pattern: " << pat;
    } else if (read_flag("-v")) {
      SyncOut(ctx) << mold_version;
      version_shown = true;
    } else if (read_arg("-weak_framework")) {
      remaining.push_back("-weak_framework");
      remaining.push_back(std::string(arg));
    } else if (read_joined("-weak-l")) {
      remaining.push_back("-weak-l");
      remaining.push_back(std::string(arg));
    } else {
      if (args[i][0] == '-')
        Fatal(ctx) << "unknown command line option: " << args[i];
      remaining.push_back(std::string(args[i]));
      i++;
    }
  }

  if (!ctx.arg.entry)
    ctx.arg.entry = get_symbol(ctx, "_main");

  if (ctx.arg.thread_count == 0)
    ctx.arg.thread_count = get_default_thread_count();

  if (!ctx.arg.bundle_loader.empty() && ctx.output_type != MH_BUNDLE)
    Fatal(ctx) << "-bundle_loader cannot be specified without -bundle";

  auto add_search_path = [&](std::vector<std::string> &vec, std::string path) {
    if (!path.starts_with('/') || ctx.arg.syslibroot.empty()) {
      if (is_directory(path))
        vec.push_back(path);
      return;
    }

    bool found = false;
    for (std::string &dir : ctx.arg.syslibroot) {
      std::string str = path_clean(dir + "/" + path);
      if (is_directory(str)) {
        vec.push_back(str);
        found = true;
      }
    }
    if (!found && is_directory(path))
      vec.push_back(path);
  };

  for (std::string &path : library_paths)
    add_search_path(ctx.arg.library_paths, path);

  if (!nostdlib) {
    add_search_path(ctx.arg.library_paths, "/usr/lib");
    add_search_path(ctx.arg.library_paths, "/usr/local/lib");
  }

  for (std::string &path : framework_paths)
    add_search_path(ctx.arg.framework_paths, path);

  if (!nostdlib) {
    add_search_path(ctx.arg.framework_paths, "/Library/Frameworks");
    add_search_path(ctx.arg.framework_paths, "/System/Library/Frameworks");
  }

  if (pagezero_size.has_value()) {
    if (ctx.output_type != MH_EXECUTE)
      Fatal(ctx) << "-pagezero_size option can only be used when"
                 << " linking a main executable";
    ctx.arg.pagezero_size = *pagezero_size;
  } else {
    ctx.arg.pagezero_size = (ctx.output_type == MH_EXECUTE) ? 0x100000000 : 0;
  }

  if (ctx.arg.final_output == "") {
    if (ctx.arg.install_name != "")
      ctx.arg.final_output = ctx.arg.install_name;
    else
      ctx.arg.final_output = ctx.arg.output;
  }

  if (ctx.arg.uuid == UUID_RANDOM)
    memcpy(ctx.uuid, get_uuid_v4().data(), 16);

  if (version_shown && remaining.empty())
    exit(0);
  return remaining;
}

#define INSTANTIATE(E)                                                  \
  template i64 parse_version(Context<E> &, std::string_view);           \
  template std::vector<std::string> parse_nonpositional_args(Context<E> &)

INSTANTIATE_ALL;

} // namespace mold::macho
