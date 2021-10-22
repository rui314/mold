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
  -arch <ARCH_NAME>           Specify target architecture
  -demangle                   Demangle C++ symbols in log messages (default)
  -dynamic                    Link against dylibs (default)
  -help                       Report usage information
  -lto_library <FILE>         Ignored
  -o FILE                     Set output filename
  -platform_version <PLATFORM> <MIN_VERSION> <SDK_VERSION>
                              Set platform, platform version and SDK version
  -syslibroot <DIR>           Prepend DIR to library search paths
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
  static std::regex re(R"(\d+(?:\.\d+(?:\.\d+)))",
                       std::regex_constants::ECMAScript);
  std::cmatch m;
  if (!std::regex_match(arg.begin(), arg.end(), m, re))
    Fatal(ctx) << "malformed version number: " << arg;
  return (stoi(m[1]) << 16) | (stoi(m[2]) << 8) | stoi(m[3]);
}

void parse_nonpositional_args(Context &ctx,
                              std::vector<std::string_view> &remaining) {
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

    if (read_arg("-L")) {
      ctx.arg.library_paths.push_back(std::string(arg));
    } else if (args[0].starts_with("-L")) {
      ctx.arg.library_paths.push_back(std::string(args[0].substr(2)));
      args = args.subspan(1);
    } else if (read_arg("-arch")) {
      if (arg != "x86_64")
        Fatal(ctx) << "unknown -arch: " << arg;
    } else if (read_flag("-demangle")) {
      ctx.arg.demangle = true;
    } else if (read_flag("-dynamic")) {
      ctx.arg.dynamic = true;
    } else if (read_flag("-lto_library")) {
    } else if (read_arg("-o")) {
      ctx.arg.output = arg;
    } else if (read_arg3("-platform_version")) {
      ctx.arg.platform = parse_platform(ctx, arg);
      ctx.arg.platform_min_version = parse_version(ctx, arg2);
      ctx.arg.platform_sdk_version = parse_version(ctx, arg3);
    } else if (read_arg("-syslibroot")) {
      ctx.arg.syslibroot = arg;
    } else if (read_flag("-v")) {
      SyncOut(ctx) << mold_version;
    } else {
      if (args[0][0] == '-')
        Fatal(ctx) << "unknown command line option: " << args[0];
      remaining.push_back(args[0]);
      args = args.subspan(1);
    }
  }

  if (ctx.arg.output.empty())
    ctx.arg.output = "a.out";
}

} // namespace mold::macho
