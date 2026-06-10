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

// Extracts a token from the beginning of `input` and returns it.
// Returns an empty string if there's no more token. The lexer never
// produces an empty token, so an empty return value unambiguously
// means "end of input".
template <typename E>
std::string_view Script<E>::lex_one() {
  // Skip whitespace and comments
  for (;;) {
    if (input.empty())
      return "";

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
      if (pos == std::string_view::npos) {
        input = "";
        return "";
      }
      input = input.substr(pos + 1);
      continue;
    }
    break;
  }

  if (input[0] == '"') {
    i64 pos = input.find('"', 1);
    if (pos == std::string_view::npos)
      error(input, "unclosed string literal");
    std::string_view tok = input.substr(0, pos + 1);
    input = input.substr(pos + 1);
    return tok;
  }

  i64 pos = input.find_first_not_of(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
    "0123456789_.$/\\~=+[]*?-!^:");

  if (pos == 0)
    pos = 1;
  else if (pos == input.npos)
    pos = input.size();

  std::string_view tok = input.substr(0, pos);
  input = input.substr(pos);
  return tok;
}

// The following functions form a cursor into the token stream.
// peek() returns the token `n` tokens ahead without consuming it,
// or an empty string if there's no such token.
template <typename E>
std::string_view Script<E>::peek(i64 n) {
  while (tokens.size() <= pos + n) {
    std::string_view tok = lex_one();
    if (tok.empty())
      return "";
    tokens.push_back(tok);
  }
  return tokens[pos + n];
}

template <typename E>
std::string_view Script<E>::next() {
  if (peek().empty())
    Fatal(ctx) << mf->name << ": unexpected EOF";
  return tokens[pos++];
}

template <typename E>
bool Script<E>::at_eof() {
  return peek().empty();
}

template <typename E>
bool Script<E>::consume(std::string_view str) {
  if (peek() == str) {
    pos++;
    return true;
  }
  return false;
}

template <typename E>
void Script<E>::skip(std::string_view str) {
  if (at_eof())
    Fatal(ctx) << mf->name << ": expected '" << str << "', but got EOF";
  if (!consume(str))
    error(peek(), "expected '" + std::string(str) + "'");
}

// Reads a token like "global:", which may have been tokenized as a
// single token or as two tokens depending on whether the label was
// followed by a space.
template <typename E>
bool Script<E>::consume_label(std::string label) {
  if (consume(label + ":"))
    return true;

  if (peek() == label && peek(1) == ":") {
    pos += 2;
    return true;
  }
  return false;
}

static std::string_view unquote(std::string_view s) {
  if (s.size() > 0 && s[0] == '"') {
    assert(s[s.size() - 1] == '"');
    return s.substr(1, s.size() - 2);
  }
  return s;
}

template <typename E>
void Script<E>::read_output_format() {
  skip("(");
  while (!consume(")"))
    next();
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
void Script<E>::read_group() {
  skip("(");

  while (!consume(")")) {
    if (consume("AS_NEEDED")) {
      bool orig = rctx.as_needed;
      rctx.as_needed = true;
      read_group();
      rctx.as_needed = orig;
    } else {
      read_file(ctx, rctx, resolve_path(next(), true));
    }
  }
}

template <typename E>
void Script<E>::parse_linker_script() {
  while (!at_eof()) {
    if (consume("OUTPUT_FORMAT")) {
      read_output_format();
    } else if (consume("INPUT") || consume("GROUP")) {
      read_group();
    } else if (consume("VERSION")) {
      skip("{");
      read_version_script();
      skip("}");
    } else if (peek(1) == "=" && peek(3) == ";") {
      ctx.arg.defsyms.emplace_back(get_symbol(ctx, unquote(peek())),
                                   get_symbol(ctx, unquote(peek(2))));
      pos += 4;
    } else if (consume(";")) {
    } else {
      error(peek(), "unknown linker script token");
    }
  }
}

template <typename E>
std::string_view Script<E>::get_script_output_type() {
  if (peek() == "OUTPUT_FORMAT" && peek(1) == "(") {
    if (peek(2) == "elf64-x86-64")
      return X86_64::name;
    if (peek(2) == "elf32-i386")
      return I386::name;
  }

  if ((peek() == "INPUT" || peek() == "GROUP") && peek(1) == "(") {
    i64 i = (peek(2) == "AS_NEEDED" && peek(3) == "(") ? 4 : 2;
    if (!peek(i).empty())
      if (MappedFile *mf2 = resolve_path(peek(i), false))
        return get_machine_type(ctx, rctx, mf2);
  }
  return "";
}

template <typename E>
void Script<E>::read_version_script_commands(std::string_view ver_str,
                                             u16 ver_idx, bool is_global,
                                             bool is_cpp) {
  while (!at_eof() && peek() != "}") {
    if (consume_label("global")) {
      is_global = true;
      continue;
    }

    if (consume_label("local")) {
      is_global = false;
      continue;
    }

    if (consume("extern")) {
      bool is_cpp2;
      if (consume("\"C\"")) {
        is_cpp2 = false;
      } else {
        skip("\"C++\"");
        is_cpp2 = true;
      }

      skip("{");
      read_version_script_commands(ver_str, ver_idx, is_global, is_cpp2);
      skip("}");
      skip(";");
      continue;
    }

    if (peek() == "*") {
      ctx.default_version = (is_global ? ver_idx : (u32)VER_NDX_LOCAL);
    } else if (is_global) {
      ctx.version_patterns.push_back({unquote(peek()), mf->name, ver_str,
                                      ver_idx, is_cpp});
    } else {
      ctx.version_patterns.push_back({unquote(peek()), mf->name, ver_str,
                                      VER_NDX_LOCAL, is_cpp});
    }

    next();
    if (peek() == "}")
      break;
    skip(";");
  }
}

template <typename E>
void Script<E>::read_version_script() {
  u16 next_ver = VER_NDX_LAST_RESERVED + ctx.arg.version_definitions.size() + 1;

  while (!at_eof() && peek() != "}") {
    std::string_view ver_str;
    u16 ver_idx;

    if (peek() == "{") {
      ver_str = "global";
      ver_idx = VER_NDX_GLOBAL;
    } else {
      ver_str = next();
      ver_idx = next_ver++;
      ctx.arg.version_definitions.emplace_back(ver_str);
    }

    skip("{");
    read_version_script_commands(ver_str, ver_idx, true, false);
    skip("}");

    // A version definition may be followed by a predecessor version
    // name (e.g. `VER_1.1 { ... } VER_1.0;`), which we just ignore.
    if (!at_eof() && peek() != ";")
      next();
    skip(";");
  }
}

template <typename E>
void Script<E>::parse_version_script() {
  read_version_script();
  if (!at_eof())
    error(peek(), "trailing garbage token");
}

template <typename E>
void Script<E>::read_dynamic_list_commands(std::vector<DynamicPattern> &result,
                                           bool is_cpp) {
  while (!at_eof() && peek() != "}") {
    if (consume("extern")) {
      bool is_cpp2;
      if (consume("\"C\"")) {
        is_cpp2 = false;
      } else {
        skip("\"C++\"");
        is_cpp2 = true;
      }

      skip("{");
      read_dynamic_list_commands(result, is_cpp2);
      skip("}");
      skip(";");
      continue;
    }

    result.push_back({unquote(next()), "", is_cpp});
    skip(";");
  }
}

template <typename E>
std::vector<DynamicPattern> Script<E>::parse_dynamic_list() {
  std::vector<DynamicPattern> result;

  skip("{");
  read_dynamic_list_commands(result, false);
  skip("}");
  skip(";");

  if (!at_eof())
    error(peek(), "trailing garbage token");

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
