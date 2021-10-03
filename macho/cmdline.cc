#include "mold.h"

#include <sstream>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <unordered_set>

namespace mold::macho {

static const char helpmsg[] = R"(
Options:
  --help                      Report usage information
  -v                          Report version information
  -o FILE                     Set output filename
)";

bool read_arg(Context &ctx, std::span<std::string_view> &args,
              std::string_view &arg, std::string name) {
  if (args[0] == name) {
    if (args.size() == 1)
      Fatal(ctx) << "option -" << name << ": argument missing";
    arg = args[1];
    args = args.subspan(2);
    return true;
  }
  return false;
}

bool read_flag(std::span<std::string_view> &args, std::string name) {
  if (args[0] == name) {
    args = args.subspan(1);
    return true;
  }
  return false;
}

void parse_nonpositional_args(Context &ctx,
                              std::vector<std::string_view> &remaining) {
  std::span<std::string_view> args = ctx.cmdline_args;
  args = args.subspan(1);

  bool version_shown = false;

  while (!args.empty()) {
    std::string_view arg;

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

    if (read_flag("-v")) {
      SyncOut(ctx) << mold_version;
    } else if (read_arg("-o")) {
      ctx.arg.output = arg;
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
