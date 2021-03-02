#include "mold.h"

#include <tbb/global_control.h>
#include <unordered_set>

static std::vector<std::string_view> read_response_file(std::string_view path) {
  std::vector<std::string_view> vec;
  MemoryMappedFile *mb = MemoryMappedFile::must_open(std::string(path));

  auto read_quoted = [&](i64 i, char quote) {
    std::string *buf = new std::string;
    while (i < mb->size() && mb->data()[i] != quote) {
      if (mb->data()[i] == '\\') {
        buf->append(1, mb->data()[i + 1]);
        i += 2;
      } else {
        buf->append(1, mb->data()[i++]);
      }
    }
    if (i >= mb->size())
      Fatal() << path << ": premature end of input";
    vec.push_back(std::string_view(*buf));
    return i + 1;
  };

  auto read_unquoted = [&](i64 i) {
    std::string *buf = new std::string;
    while (i < mb->size() && !isspace(mb->data()[i]))
      buf->append(1, mb->data()[i++]);
    vec.push_back(std::string_view(*buf));
    return i;
  };

  for (i64 i = 0; i < mb->size();) {
    if (isspace(mb->data()[i]))
      i++;
    else if (mb->data()[i] == '\'')
      i = read_quoted(i + 1, '\'');
    else if (mb->data()[i] == '\"')
      i = read_quoted(i + 1, '\"');
    else
      i = read_unquoted(i);
  }
  return vec;
}

std::vector<std::string_view> expand_response_files(char **argv) {
  std::vector<std::string_view> vec;

  for (i64 i = 0; argv[i]; i++) {
    if (argv[i][0] == '@')
      append(vec, read_response_file(argv[i] + 1));
    else
      vec.push_back(argv[i]);
  }
  return vec;
}

static std::vector<std::string> add_dashes(std::string name) {
  // Multi-letter linker options can be preceded by either a single
  // dash or double dashes except ones starting with "o", which must
  // be preceded by double dashes. For example, "-omagic" is
  // interpreted as "-o magic". If you really want to specify the
  // "omagic" option, you have to pass "--omagic".
  if (name[0] == 'o')
    return {"-" + name};
  return {"-" + name, "--" + name};
}

bool read_arg(std::span<std::string_view> &args, std::string_view &arg,
              std::string name) {
  if (name.size() == 1) {
    if (args[0] == "-" + name) {
      if (args.size() == 1)
        Fatal() << "option -" << name << ": argument missing";
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
        Fatal() << "option " << name << ": argument missing";
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

static i64 parse_hex(std::string opt, std::string_view value) {
  if (!value.starts_with("0x") && !value.starts_with("0X"))
    Fatal() << "option -" << opt << ": not a hexadecimal number";
  value = value.substr(2);
  if (value.find_first_not_of("0123456789abcdefABCDEF") != std::string_view::npos)
    Fatal() << "option -" << opt << ": not a hexadecimal number";
  return std::stol(std::string(value), nullptr, 16);
}

static i64 parse_number(std::string opt, std::string_view value) {
  if (value.find_first_not_of("0123456789") != std::string_view::npos)
    Fatal() << "option -" << opt << ": not a number";
  return std::stol(std::string(value));
}

static std::vector<std::string_view> get_input_files(std::span<std::string_view> args) {
  static std::unordered_set<std::string_view> needs_arg({
    "o", "dynamic-linker", "export-dynamic", "e", "entry", "y",
    "trace-symbol", "filler", "sysroot", "thread-count", "z",
    "hash-style", "m", "rpath", "version-script",
  });

  std::vector<std::string_view> vec;
  std::vector<std::string> library_paths;

  while (args.empty()) {
    if (needs_arg.contains(args[0])) {
      if (args.size() == 1)
        Fatal() << args[0] << ": missing argument";
      args = args.subspan(2);
      continue;
    }

    std::string_view arg;

    if (read_arg(args, arg, "L") || read_arg(args, arg, "library-path")) {
      library_paths.push_back(std::string(arg));
    }

    if (read_arg(args, arg, "l")) {
      vec.push_back(arg);
      continue;
    }

    if (args[0].starts_with("-")) {
      args = args.subspan(1);
      continue;
    }

    vec.push_back(args[0]);
    args = args.subspan(1);
  }
  return vec;
}

Config parse_nonpositional_args(std::span<std::string_view> args,
                                std::vector<std::string_view> &remaining) {
  Config conf;
  conf.thread_count =
    tbb::global_control::active_value(tbb::global_control::max_allowed_parallelism);

  while (!args.empty()) {
    std::string_view arg;

    if (read_flag(args, "v") || read_flag(args, "version")) {
      SyncOut() << "mold (compatible with GNU linkers)";
      exit(0);
    }

    if (read_arg(args, arg, "o")) {
      conf.output = arg;
    } else if (read_arg(args, arg, "dynamic-linker")) {
      conf.dynamic_linker = arg;
    } else if (read_flag(args, "export-dynamic") || read_flag(args, "E")) {
      conf.export_dynamic = true;
    } else if (read_flag(args, "Bsymbolic")) {
      conf.Bsymbolic = true;
    } else if (read_flag(args, "Bsymbolic-functions")) {
      conf.Bsymbolic_functions = true;
    } else if (read_flag(args, "no-export-dynamic")) {
      conf.export_dynamic = false;
    } else if (read_arg(args, arg, "e") || read_arg(args, arg, "entry")) {
      conf.entry = arg;
    } else if (read_flag(args, "print-map")) {
      conf.print_map = true;
    } else if (read_flag(args, "static")) {
      conf.is_static = true;
    } else if (read_flag(args, "shared") || read_flag(args, "Bshareable")) {
      conf.shared = true;
    } else if (read_flag(args, "demangle")) {
      conf.demangle = true;
    } else if (read_flag(args, "no-demangle")) {
      conf.demangle = false;
    } else if (read_arg(args, arg, "y") || read_arg(args, arg, "trace-symbol")) {
      conf.trace_symbol.push_back(arg);
    } else if (read_arg(args, arg, "filler")) {
      conf.filler = parse_hex("filler", arg);
    } else if (read_arg(args, arg, "L") || read_arg(args, arg, "library-path")) {
      conf.library_paths.push_back(arg);
    } else if (read_arg(args, arg, "sysroot")) {
      conf.sysroot = arg;
    } else if (read_arg(args, arg, "u") || read_arg(args, arg, "undefined")) {
      conf.undefined.push_back(arg);
    } else if (read_arg(args, arg, "init")) {
      conf.init = arg;
    } else if (read_arg(args, arg, "fini")) {
      conf.fini = arg;
    } else if (read_arg(args, arg, "soname") || read_arg(args, arg, "h")) {
      conf.soname = arg;
    } else if (read_arg(args, arg, "hash-style")) {
      if (arg == "sysv") {
        conf.hash_style_sysv = true;
        conf.hash_style_gnu = false;
      } else if (arg == "gnu") {
        conf.hash_style_sysv = false;
        conf.hash_style_gnu = true;
      } else if (arg == "both") {
        conf.hash_style_sysv = true;
        conf.hash_style_gnu = true;
      } else {
        Fatal() << "invalid --hash-style argument: " << arg;
      }
    } else if (read_flag(args, "allow-multiple-definition")) {
      conf.allow_multiple_definition = true;
    } else if (read_flag(args, "trace")) {
      conf.trace = true;
    } else if (read_flag(args, "eh-frame-hdr")) {
      conf.eh_frame_hdr = true;
    } else if (read_flag(args, "no-eh-frame-hdr")) {
      conf.eh_frame_hdr = false;
    } else if (read_flag(args, "pie") || read_flag(args, "pic-executable")) {
      conf.pic = true;
      conf.pie = true;
    } else if (read_flag(args, "no-pie") || read_flag(args, "no-pic-executable")) {
      conf.pic = false;
      conf.pie = false;
    } else if (read_flag(args, "relax")) {
      conf.relax = true;
    } else if (read_flag(args, "no-relax")) {
      conf.relax = false;
    } else if (read_flag(args, "print-perf")) {
      conf.print_perf = true;
    } else if (read_flag(args, "print-stats")) {
      conf.print_stats = true;
    } else if (read_z_flag(args, "now")) {
      conf.z_now = true;
    } else if (read_flag(args, "fork")) {
      conf.fork = true;
    } else if (read_flag(args, "no-fork")) {
      conf.fork = false;
    } else if (read_flag(args, "gc-sections")) {
      conf.gc_sections = true;
    } else if (read_flag(args, "no-gc-sections")) {
      conf.gc_sections = false;
    } else if (read_flag(args, "print-gc-sections")) {
      conf.print_gc_sections = true;
    } else if (read_flag(args, "no-print-gc-sections")) {
      conf.print_gc_sections = false;
    } else if (read_flag(args, "icf")) {
      conf.icf = true;
    } else if (read_flag(args, "no-icf")) {
      conf.icf = false;
    } else if (read_flag(args, "quick-exit")) {
      conf.quick_exit = true;
    } else if (read_flag(args, "no-quick-exit")) {
      conf.quick_exit = false;
    } else if (read_flag(args, "print-icf-sections")) {
      conf.print_icf_sections = true;
    } else if (read_flag(args, "no-print-icf-sections")) {
      conf.print_icf_sections = false;
    } else if (read_flag(args, "quick-exit")) {
      conf.quick_exit = true;
    } else if (read_flag(args, "no-quick-exit")) {
      conf.quick_exit = false;
    } else if (read_arg(args, arg, "thread-count")) {
      conf.thread_count = parse_number("thread-count", arg);
    } else if (read_flag(args, "no-threads")) {
      conf.thread_count = 1;
    } else if (read_flag(args, "discard-all") || read_flag(args, "x")) {
      conf.discard_all = true;
    } else if (read_flag(args, "discard-locals") || read_flag(args, "X")) {
      conf.discard_locals = true;
    } else if (read_flag(args, "strip-all") || read_flag(args, "s")) {
      conf.strip_all = true;
    } else if (read_arg(args, arg, "rpath")) {
      if (!conf.rpaths.empty())
        conf.rpaths += ":";
      conf.rpaths += arg;
    } else if (read_arg(args, arg, "version-script")) {
      conf.version_script.push_back(arg);
    } else if (read_flag(args, "build-id")) {
      conf.build_id = BuildIdKind::HASH;
      conf.build_id_size = 20;
    } else if (read_arg(args, arg, "build-id")) {
      if (arg == "none") {
        conf.build_id = BuildIdKind::NONE;
      } else if (arg == "uuid") {
        conf.build_id = BuildIdKind::UUID;
        conf.build_id_size = 16;
      } else if (arg == "md5") {
        conf.build_id = BuildIdKind::HASH;
        conf.build_id_size = 16;
      } else if (arg == "sha1") {
        conf.build_id = BuildIdKind::HASH;
        conf.build_id_size = 20;
      } else if (arg == "sha256") {
        conf.build_id = BuildIdKind::HASH;
        conf.build_id_size = 32;
      } else {
        Fatal() << "invalid --build-id argument: " << arg;
      }
    } else if (read_flag(args, "preload")) {
      conf.preload = true;
    } else if (read_arg(args, arg, "z")) {
    } else if (read_arg(args, arg, "O")) {
    } else if (read_flag(args, "O0")) {
    } else if (read_flag(args, "O1")) {
    } else if (read_flag(args, "O2")) {
    } else if (read_arg(args, arg, "m")) {
    } else if (read_flag(args, "eh-frame-hdr")) {
    } else if (read_flag(args, "start-group")) {
    } else if (read_flag(args, "end-group")) {
    } else if (read_flag(args, "(")) {
    } else if (read_flag(args, ")")) {
    } else if (read_flag(args, "fatal-warnings")) {
    } else if (read_flag(args, "disable-new-dtags")) {
    } else if (read_flag(args, "as-needed")) {
      remaining.push_back("-as-needed");
    } else if (read_flag(args, "no-as-needed")) {
      remaining.push_back("-no-as-needed");
    } else if (read_flag(args, "whole-archive")) {
      remaining.push_back("-whole-archive");
    } else if (read_flag(args, "no-whole-archive")) {
      remaining.push_back("-no-whole-archive");
    } else if (read_arg(args, arg, "l")) {
      remaining.push_back("-l");
      remaining.push_back(arg);
    } else if (read_arg(args, arg, "script") || read_arg(args, arg, "T")) {
      remaining.push_back(arg);
    } else {
      if (args[0][0] == '-')
        Fatal() << "mold: unknown command line option: " << args[0];
      remaining.push_back(args[0]);
      args = args.subspan(1);
    }
  }

  if (conf.shared) {
    conf.pic = true;
    conf.dynamic_linker = "";
  }

  return conf;
}

