#include "mold.h"
#include "../cmdline.h"

#include <optional>
#include <sstream>
#include <sys/stat.h>
#include <sys/types.h>
#include <regex>
#include <unistd.h>
#include <unordered_set>

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
  -adhoc_codesign             Add ad-hoc code signature to the output file
    -no_adhoc_codesign
  -all_load                   Include all objects from static archives
    -noall_load
  -arch <ARCH_NAME>           Specify target architecture
  -bundle                     Produce a mach-o bundle
  -dead_strip                 Remove unreachable functions and data
  -dead_strip_dylibs          Remove unreachable dylibs from dependencies
  -demangle                   Demangle C++ symbols in log messages (default)
  -dylib                      Produce a dynamic library
  -dynamic                    Link against dylibs (default)
  -e <SYMBOL>                 Specify the entry point of a main executable
  -execute                    Produce an executable (default)
  -export_dynamic             Preserves all global symbols in main executables during LTO
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
  -install_name <NAME>
  -l<LIB>                     Search for a given library
  -lto_library <FILE>         Ignored
  -macos_version_min <VERSION>
  -map <FILE>                 Write map file to a given file
  -needed-l<LIB>              Search for a given library
  -needed-framework <NAME>[,<SUFFIX>]
                              Search for a given framework
  -no_deduplicate             Ignored
  -no_uuid                    Do not generate an LC_UUID load command
  -o <FILE>                   Set output filename
  -pagezero_size <SIZE>       Specify the size of the __PAGEZERO segment
  -platform_version <PLATFORM> <MIN_VERSION> <SDK_VERSION>
                              Set platform, platform version and SDK version
  -random_uuid                Generate a random LC_UUID load command
  -rpath <PATH>               Add PATH to the runpath search path list
  -search_dylibs_first
  -search_paths_first
  -sectcreate <SEGNAME> <SECTNAME> <FILE>
  -stack_size <SIZE>
  -syslibroot <DIR>           Prepend DIR to library search paths
  -t                          Print out each file the linker loads
  -v                          Report version information)";

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
static i64 parse_version(Context<E> &ctx, std::string_view arg) {
  static std::regex re(R"((\d+)(?:\.(\d+))?(?:\.(\d+))?)",
                       std::regex_constants::ECMAScript);
  std::cmatch m;
  if (!std::regex_match(arg.begin(), arg.end(), m, re))
    Fatal(ctx) << "malformed version number: " << arg;

  i64 major = (m[1].length() == 0) ? 0 : stoi(m[1]);
  i64 minor = (m[2].length() == 0) ? 0 : stoi(m[2]);
  i64 patch = (m[3].length() == 0) ? 0 : stoi(m[3]);
  return (major << 16) | (minor << 8) | patch;
}

static bool is_directory(std::filesystem::path path) {
  std::error_code ec;
  return std::filesystem::is_directory(path, ec) && !ec;
}

template <typename E>
std::vector<std::string> parse_nonpositional_args(Context<E> &ctx) {
  std::vector<std::string_view> &args = ctx.cmdline_args;
  std::vector<std::string> remaining;
  i64 i = 1;

  std::vector<std::string> framework_paths;
  std::vector<std::string> library_paths;
  bool nostdlib = false;
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
    } else if (read_flag("-adhoc_codesign")) {
      ctx.arg.adhoc_codesign = true;
    } else if (read_flag("-no_adhoc_codesign")) {
      ctx.arg.adhoc_codesign = false;
    } else if (read_flag("-all_load")) {
      remaining.push_back("-all_load");
    } else if (read_flag("-noall_load")) {
      remaining.push_back("-noall_load");
    } else if (read_arg("-arch")) {
      if (arg == "x86_64")
        ctx.arg.arch = CPU_TYPE_X86_64;
      else if (arg == "arm64")
        ctx.arg.arch = CPU_TYPE_ARM64;
      else
        Fatal(ctx) << "unknown -arch: " << arg;
    } else if (read_flag("-bundle")) {
      ctx.output_type = MH_BUNDLE;
    } else if (read_flag("-color-diagnostics") ||
               read_flag("--color-diagnostics")) {
      ctx.arg.color_diagnostics = true;
    } else if (read_flag("-dead_strip")) {
      ctx.arg.dead_strip = true;
    } else if (read_flag("-dead_strip_dylibs")) {
      ctx.arg.dead_strip_dylibs = true;
    } else if (read_flag("-demangle")) {
      ctx.arg.demangle = true;
    } else if (read_flag("-dylib")) {
      ctx.output_type = MH_DYLIB;
    } else if (read_hex("-headerpad")) {
      ctx.arg.headerpad = hex_arg;
    } else if (read_flag("-headerpad_max_install_names")) {
      ctx.arg.headerpad = 1024;
    } else if (read_flag("-dynamic")) {
      ctx.arg.dynamic = true;
    } else if (read_arg("-e")) {
      ctx.arg.entry = arg;
    } else if (read_flag("-execute")) {
      ctx.output_type = MH_EXECUTE;
    } else if (read_flag("-export_dynamic")) {
      ctx.arg.export_dynamic = true;
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
    } else if (read_arg("-install_name")) {
      ctx.arg.install_name = arg;
    } else if (read_joined("-l")) {
      remaining.push_back("-l");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-map")) {
      ctx.arg.map = arg;
    } else if (read_arg("-mllvm")) {
      ctx.arg.mllvm.push_back(std::string(arg));
    } else if (read_joined("-needed-l")) {
      remaining.push_back("-needed-l");
      remaining.push_back(std::string(arg));
    } else if (read_arg("-needed_framework")) {
      remaining.push_back("-needed_framework");
      remaining.push_back(std::string(arg));
    } else if (read_flag("-no_deduplicate")) {
    } else if (read_flag("-no_uuid")) {
      ctx.arg.uuid = UUID_NONE;
    } else if (read_arg("-o")) {
      ctx.arg.output = arg;
    } else if (read_hex("-pagezero_size")) {
      pagezero_size = hex_arg;
    } else if (read_arg3("-platform_version")) {
      ctx.arg.platform = parse_platform(ctx, arg);
      ctx.arg.platform_min_version = parse_version(ctx, arg2);
      ctx.arg.platform_sdk_version = parse_version(ctx, arg3);
    } else if (read_flag("-random_uuid")) {
      ctx.arg.uuid = UUID_RANDOM;
    } else if (read_arg("-rpath")) {
      ctx.arg.rpath.push_back(std::string(arg));
    } else if (read_flag("-search_paths_first")) {
      ctx.arg.search_paths_first = true;
    } else if (read_flag("-search_dylibs_first")) {
      ctx.arg.search_paths_first = false;
    } else if (read_arg3("-sectcreate")) {
      ctx.arg.sectcreate.push_back({arg, arg2, arg3});
    } else if (read_hex("-stack_size")) {
      ctx.arg.stack_size = hex_arg;
    } else if (read_arg("-syslibroot")) {
      ctx.arg.syslibroot.push_back(std::string(arg));
    } else if (read_flag("-t")) {
      ctx.arg.trace = true;
    } else if (read_flag("-v")) {
      SyncOut(ctx) << mold_version;
    } else {
      if (args[i][0] == '-')
        Fatal(ctx) << "unknown command line option: " << args[i];
      remaining.push_back(std::string(args[i]));
      i++;
    }
  }

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

  return remaining;
}

#define INSTANTIATE(E) \
  template std::vector<std::string> parse_nonpositional_args(Context<E> &)

INSTANTIATE_ALL;

} // namespace mold::macho
