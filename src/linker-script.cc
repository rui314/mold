// On Linux, /usr/lib/x86_64-linux-gnu/libc.so is not actually
// a shared object file but an ASCII text file containing a linker
// script to include a "real" libc.so file. Therefore, we need to
// support a (very limited) subset of the linker script language.

#include "mold.h"

#include <cctype>

namespace mold {

static std::string_view get_line(std::string_view input, const char *pos) {
  assert(input.data() <= pos);
  assert(pos < input.data() + input.size());

  i64 start = input.rfind('\n', pos - input.data());
  if (start == input.npos)
    start = 0;
  else
    start++;

  i64 end = input.find('\n', pos - input.data());
  if (end == input.npos)
    end = input.size();

  return input.substr(start, end - start);
}

template <typename E>
void Script<E>::error(std::string_view pos, std::string msg) {
  std::string_view input = mf->get_contents();
  std::string_view line = get_line(input, pos.data());

  i64 lineno = 1;
  for (i64 i = 0; input.data() + i < line.data(); i++)
    if (input[i] == '\n')
      lineno++;

  std::string label = mf->name + ":" + std::to_string(lineno) + ": ";
  i64 indent = strlen("mold: fatal: ") + label.size();
  i64 column = pos.data() - line.data();

  Fatal(ctx) << label << line << "\n"
             << std::string(indent + column, ' ') << "^ " << msg;
}

template <typename E>
void Script<E>::tokenize() {
  std::string_view input = mf->get_contents();

  while (!input.empty()) {
    if (isspace(input[0])) {
      input = input.substr(1);
      continue;
    }

    if (input.starts_with("/*")) {
      i64 pos = input.find("*/", 2);
      if (pos == std::string_view::npos)
        error(input, "unclosed comment");
      input = input.substr(pos + 2);
      continue;
    }

    if (input[0] == '#') {
      i64 pos = input.find("\n", 1);
      if (pos == std::string_view::npos)
        break;
      input = input.substr(pos + 1);
      continue;
    }

    if (input[0] == '"') {
      i64 pos = input.find('"', 1);
      if (pos == std::string_view::npos)
        error(input, "unclosed string literal");
      tokens.push_back(input.substr(0, pos + 1));
      input = input.substr(pos + 1);
      continue;
    }

    i64 pos = input.find_first_not_of(
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
      "0123456789_.$/\\~=+[]*?-!^:");

    if (pos == 0)
      pos = 1;
    else if (pos == input.npos)
      pos = input.size();

    tokens.push_back(input.substr(0, pos));
    input = input.substr(pos);
  }
}

template <typename E>
std::span<std::string_view>
Script<E>::skip(std::span<std::string_view> tok, std::string_view str) {
  if (tok.empty())
    Fatal(ctx) << mf->name << ": expected '" << str << "', but got EOF";
  if (tok[0] != str)
    error(tok[0], "expected '" + std::string(str) + "'");
  return tok.subspan(1);
}

static std::string_view unquote(std::string_view s) {
  if (s.size() > 0 && s[0] == '"') {
    assert(s[s.size() - 1] == '"');
    return s.substr(1, s.size() - 2);
  }
  return s;
}

template <typename E>
std::span<std::string_view>
Script<E>::read_output_format(std::span<std::string_view> tok) {
  tok = skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.subspan(1);
  if (tok.empty())
    Fatal(ctx) << mf->name << ": expected ')', but got EOF";
  return tok.subspan(1);
}

template <typename E>
static bool is_in_sysroot(Context<E> &ctx, std::string path) {
  std::string sysroot = ctx.arg.sysroot;
  if (sysroot.starts_with('/') && !ctx.arg.chroot.empty())
    sysroot = ctx.arg.chroot + "/" + path_clean(sysroot);

  std::string rel = std::filesystem::relative(path, sysroot).string();
  return rel != "." && !rel.starts_with("../");
}

template <typename E>
MappedFile *Script<E>::resolve_path(std::string_view tok, bool check_target) {
  std::string str(unquote(tok));

  auto open = [&](const std::string &path) -> MappedFile * {
    MappedFile *mf = open_file(ctx, path);
    if (!mf)
      return nullptr;

    if (check_target) {
      std::string_view target = get_machine_type(ctx, rctx, mf);
      if (!target.empty() && target != E::name) {
        Warn(ctx) << path << ": skipping incompatible file: " << target
                  << " (e_machine " << (int)E::e_machine << ")";
        return nullptr;
      }
    }
    return mf;
  };

  // GNU ld prepends the sysroot if a pathname starts with '/' and the
  // script being processed is in the sysroot. We do the same.
  if (str.starts_with('/') && is_in_sysroot(ctx, mf->name))
    return must_open_file(ctx, ctx.arg.sysroot + str);

  if (str.starts_with('=')) {
    std::string path;
    if (ctx.arg.sysroot.empty())
      path = str.substr(1);
    else
      path = ctx.arg.sysroot + str.substr(1);
    return must_open_file(ctx, path);
  }

  if (str.starts_with("-l"))
    return find_library(ctx, rctx, str.substr(2));

  if (!str.starts_with('/'))
    if (MappedFile *mf2 = open(path_clean(mf->name + "/../" + str)))
      return mf2;

  if (MappedFile *mf = open(str))
    return mf;

  for (std::string_view dir : ctx.arg.library_paths) {
    std::string path = std::string(dir) + "/" + str;
    if (MappedFile *mf = open(path))
      return mf;
  }

  error(tok, "library not found: " + str);
}

template <typename E>
std::span<std::string_view>
Script<E>::read_group(std::span<std::string_view> tok) {
  tok = skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      bool orig = rctx.as_needed;
      rctx.as_needed = true;
      tok = read_group(tok.subspan(1));
      rctx.as_needed = orig;
      continue;
    }

    MappedFile *mf = resolve_path(tok[0], true);
    read_file(ctx, rctx, mf);
    tok = tok.subspan(1);
  }

  if (tok.empty())
    Fatal(ctx) << mf->name << ": expected ')', but got EOF";
  return tok.subspan(1);
}

template <typename E>
void Script<E>::parse_linker_script() {
  std::call_once(once, [&] { tokenize(); });
  std::span<std::string_view> tok = tokens;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT") {
      tok = read_output_format(tok.subspan(1));
    } else if (tok[0] == "INPUT" || tok[0] == "GROUP") {
      tok = read_group(tok.subspan(1));
    } else if (tok[0] == "VERSION") {
      tok = tok.subspan(1);
      tok = skip(tok, "{");
      tok = read_version_script(tok);
      tok = skip(tok, "}");
    } else if (tok.size() > 3 && tok[1] == "=" && tok[3] == ";") {
      ctx.arg.defsyms.emplace_back(get_symbol(ctx, unquote(tok[0])),
                                   get_symbol(ctx, unquote(tok[2])));
      tok = tok.subspan(4);
    } else if (tok[0] == ";") {
      tok = tok.subspan(1);
    } else {
      error(tok[0], "unknown linker script token");
    }
  }
}

template <typename E>
std::string_view Script<E>::get_script_output_type() {
  std::call_once(once, [&] { tokenize(); });
  std::span<std::string_view> tok = tokens;

  if (tok.size() >= 3 && tok[0] == "OUTPUT_FORMAT" && tok[1] == "(") {
    if (tok[2] == "elf64-x86-64")
      return X86_64::name;
    if (tok[2] == "elf32-i386")
      return I386::name;
  }

  if (tok.size() >= 3 && (tok[0] == "INPUT" || tok[0] == "GROUP") &&
      tok[1] == "(") {
    MappedFile *mf;

    if (tok.size() >= 5 && tok[2] == "AS_NEEDED" && tok[3] == "(")
      mf = resolve_path(tok[4], false);
    else
      mf = resolve_path(tok[2], false);

    if (mf)
      return get_machine_type(ctx, rctx, mf);
  }

  return "";
}

static bool read_label(std::span<std::string_view> &tok, std::string label) {
  if (tok.size() >= 1 && tok[0] == label + ":") {
    tok = tok.subspan(1);
    return true;
  }

  if (tok.size() >= 2 && tok[0] == label && tok[1] == ":") {
    tok = tok.subspan(2);
    return true;
  }
  return false;
}

template <typename E>
std::span<std::string_view>
Script<E>::read_version_script_commands(std::span<std::string_view> tok,
                                        std::string_view ver_str, u16 ver_idx,
                                        bool is_global, bool is_cpp) {
  while (!tok.empty() && tok[0] != "}") {
    if (read_label(tok, "global")) {
      is_global = true;
      continue;
    }

    if (read_label(tok, "local")) {
      is_global = false;
      continue;
    }

    if (tok[0] == "extern") {
      tok = tok.subspan(1);

      if (!tok.empty() && tok[0] == "\"C\"") {
        tok = tok.subspan(1);
        tok = skip(tok, "{");
        tok = read_version_script_commands(tok, ver_str, ver_idx, is_global, false);
      } else {
        tok = skip(tok, "\"C++\"");
        tok = skip(tok, "{");
        tok = read_version_script_commands(tok, ver_str, ver_idx, is_global, true);
      }

      tok = skip(tok, "}");
      tok = skip(tok, ";");
      continue;
    }

    if (tok[0] == "*") {
      ctx.default_version = (is_global ? ver_idx : (u32)VER_NDX_LOCAL);
    } else if (is_global) {
      ctx.version_patterns.push_back({unquote(tok[0]), mf->name, ver_str,
                                      ver_idx, is_cpp});
    } else {
      ctx.version_patterns.push_back({unquote(tok[0]), mf->name, ver_str,
                                      VER_NDX_LOCAL, is_cpp});
    }

    tok = tok.subspan(1);

    if (!tok.empty() && tok[0] == "}")
      break;
    tok = skip(tok, ";");
  }
  return tok;
}

template <typename E>
std::span<std::string_view>
Script<E>::read_version_script(std::span<std::string_view> tok) {
  u16 next_ver = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size() + 1;

  while (!tok.empty() && tok[0] != "}") {
    std::string_view ver_str;
    u16 ver_idx;

    if (tok[0] == "{") {
      ver_str = "global";
      ver_idx = VER_NDX_GLOBAL;
    } else {
      ver_str = tok[0];
      ver_idx = next_ver++;
      ctx.arg.version_definitions.emplace_back(tok[0]);
      tok = tok.subspan(1);
    }

    tok = skip(tok, "{");
    tok = read_version_script_commands(tok, ver_str, ver_idx, true, false);
    tok = skip(tok, "}");
    if (!tok.empty() && tok[0] != ";")
      tok = tok.subspan(1);
    tok = skip(tok, ";");
  }
  return tok;
}

template <typename E>
void Script<E>::parse_version_script() {
  std::call_once(once, [&] { tokenize(); });
  std::span<std::string_view> tok = tokens;
  tok = read_version_script(tok);
  if (!tok.empty())
    error(tok[0], "trailing garbage token");
}

template <typename E>
std::span<std::string_view>
Script<E>::read_dynamic_list_commands(std::span<std::string_view> tok,
                                      std::vector<DynamicPattern> &result,
                                      bool is_cpp) {
  while (!tok.empty() && tok[0] != "}") {
    if (tok[0] == "extern") {
      tok = tok.subspan(1);

      if (!tok.empty() && tok[0] == "\"C\"") {
        tok = tok.subspan(1);
        tok = skip(tok, "{");
        tok = read_dynamic_list_commands(tok, result, false);
      } else {
        tok = skip(tok, "\"C++\"");
        tok = skip(tok, "{");
        tok = read_dynamic_list_commands(tok, result, true);
      }

      tok = skip(tok, "}");
      tok = skip(tok, ";");
      continue;
    }

    result.push_back({unquote(tok[0]), "", is_cpp});
    tok = skip(tok.subspan(1), ";");
  }
  return tok;
}

template <typename E>
std::vector<DynamicPattern> Script<E>::parse_dynamic_list() {
  std::call_once(once, [&] { tokenize(); });
  std::span<std::string_view> tok = tokens;
  std::vector<DynamicPattern> result;

  tok = skip(tok, "{");
  tok = read_dynamic_list_commands(tok, result, false);
  tok = skip(tok, "}");
  tok = skip(tok, ";");

  if (!tok.empty())
    error(tok[0], "trailing garbage token");

  for (DynamicPattern &p : result)
    p.source = mf->name;
  return result;
}

template <typename E>
std::vector<DynamicPattern>
parse_dynamic_list(Context<E> &ctx, std::string_view path) {
  ReaderContext rctx;
  MappedFile *mf = must_open_file(ctx, std::string(path));
  return Script(ctx, rctx, mf).parse_dynamic_list();
}

using E = MOLD_TARGET;

template class Script<E>;

template
std::vector<DynamicPattern> parse_dynamic_list(Context<E> &, std::string_view);

} // namespace mold
