#include "mold.h"

#include <sstream>
#include <sys/stat.h>
#include <sys/types.h>
#include <regex>
#include <unistd.h>
#include <unordered_set>

namespace mold::macho {

static const char helpmsg[] = R"(
Options:
  -L<PATH>                    Add DIR to library search path
  -adhoc_codesign             Add ad-hoc code signature to the output file
    -no_adhoc_codesign
  -arch <ARCH_NAME>           Specify target architecture
  -dead_strip                 Remove unreachable functions and data
  -demangle                   Demangle C++ symbols in log messages (default)
  -dynamic                    Link against dylibs (default)
  -e <SYMBOL>                 Specify the entry point of a main executable
  -headerpad <SIZE>           Allocate the size of padding after load commands
  -help                       Report usage information
  -l<LIB>                     Search for a given library
  -lto_library <FILE>         Ignored
  -map <FILE>                 Write map file to a given file
  -no_deduplicate             Ignored
  -o <FILE>                   Set output filename
  -platform_version <PLATFORM> <MIN_VERSION> <SDK_VERSION>
                              Set platform, platform version and SDK version
  -syslibroot <DIR>           Prepend DIR to library search paths
  -t                          Print out each file the linker loads
  -v                          Report version information)";

static i64 parse_platform(Context &ctx, std::string_view arg) {
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

static i64 parse_version(Context &ctx, std::string_view arg) {
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

void parse_nonpositional_args(Context &ctx,
                              std::vector<std::string> &remaining) {
  std::span<std::string_view> args = ctx.cmdline_args;
  args = args.subspan(1);

  bool version_shown = false;

  while (!args.empty()) {
    std::string_view arg;
    std::string_view arg2;
    std::string_view arg3;

    auto read_arg = [&](std::string name) {
      if (args[0] == name) {
        if (args.size() == 1)
          Fatal(ctx) << "option -" << name << ": argument missing";
        arg = args[1];
        args = args.subspan(2);
        return true;
      }
      return false;
    };

    auto read_arg3 = [&](std::string name) {
      if (args[0] == name) {
        if (args.size() == 3)
          Fatal(ctx) << "option -" << name << ": argument missing";
        arg = args[1];
        arg2 = args[2];
        arg3 = args[3];
        args = args.subspan(4);
        return true;
      }
      return false;
    };

    auto read_joined = [&](std::string name) {
      if (read_arg(name))
        return true;
      if (args[0].starts_with(name)) {
        arg = args[0].substr(2);
        args = args.subspan(1);
        return true;
      }
      return false;
    };

    auto read_flag = [&](std::string name) {
      if (args[0] == name) {
        args = args.subspan(1);
        return true;
      }
      return false;
    };

    if (read_flag("-help") || read_flag("--help")) {
      SyncOut(ctx) << "Usage: " << ctx.cmdline_args[0]
                   << " [options] file...\n" << helpmsg;
      exit(0);
    }

    if (read_joined("-L")) {
      ctx.arg.library_paths.push_back(std::string(arg));
    } else if (read_flag("-adhoc_codesign")) {
      ctx.arg.adhoc_codesign = true;
    } else if (read_flag("-no_adhoc_codesign")) {
      ctx.arg.adhoc_codesign = false;
    } else if (read_arg("-arch")) {
      if (arg != "x86_64")
        Fatal(ctx) << "unknown -arch: " << arg;
    } else if (read_flag("-dead_strip")) {
      ctx.arg.dead_strip = true;
    } else if (read_flag("-demangle")) {
      ctx.arg.demangle = true;
    } else if (read_arg("-headerpad")) {
      size_t pos;
      ctx.arg.headerpad = std::stoi(std::string(arg), &pos, 16);
      if (pos != arg.size())
        Fatal(ctx) << "malformed -headerpad: " << arg;
    } else if (read_flag("-dynamic")) {
      ctx.arg.dynamic = true;
    } else if (read_arg("-e")) {
      ctx.arg.entry = arg;
    } else if (read_arg("-lto_library")) {
    } else if (read_joined("-l")) {
      remaining.push_back("-l" + std::string(arg));
    } else if (read_arg("-map")) {
      ctx.arg.map = arg;
    } else if (read_flag("-no_deduplicate")) {
    } else if (read_arg("-o")) {
      ctx.arg.output = arg;
    } else if (read_arg3("-platform_version")) {
      ctx.arg.platform = parse_platform(ctx, arg);
      ctx.arg.platform_min_version = parse_version(ctx, arg2);
      ctx.arg.platform_sdk_version = parse_version(ctx, arg3);
    } else if (read_arg("-syslibroot")) {
      ctx.arg.syslibroot.push_back(std::string(arg));
    } else if (read_flag("-t")) {
      ctx.arg.trace = true;
    } else if (read_flag("-v")) {
      SyncOut(ctx) << mold_version;
    } else {
      if (args[0][0] == '-')
        Fatal(ctx) << "unknown command line option: " << args[0];
      remaining.push_back(std::string(args[0]));
      args = args.subspan(1);
    }
  }

  if (ctx.arg.output.empty())
    ctx.arg.output = "a.out";

  if (!ctx.arg.syslibroot.empty()) {
    std::vector<std::string> vec;

    for (std::string &dir : ctx.arg.syslibroot) {
      for (std::string &path : ctx.arg.library_paths)
        vec.push_back(path_clean(dir + "/" + path));

      vec.push_back(path_clean(dir + "/usr/lib"));
      vec.push_back(path_clean(dir + "/usr/lcoal/lib"));
    }

    ctx.arg.library_paths = vec;
  }

  ctx.arg.library_paths.push_back("/usr/lib");
  ctx.arg.library_paths.push_back("/usr/local/lib");
}

} // namespace mold::macho
