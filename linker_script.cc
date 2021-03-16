// On Linux, /usr/lib/x86_64-linux-gnu/libc.so is not actually
// a shared object file but an ASCII text file containing a linker
// script to include a "real" libc.so file. Therefore, we need to
// support a (very limited) subset of the linker script language.

#include "mold.h"

static thread_local std::string current_file;

static std::vector<std::string_view> tokenize(std::string_view input) {
  std::vector<std::string_view> vec;
  while (!input.empty()) {
    if (input[0] == ' ' || input[0] == '\t' || input[0] == '\n') {
      input = input.substr(1);
      continue;
    }

    if (input.starts_with("/*")) {
      i64 pos = input.find("*/", 2);
      if (pos == std::string_view::npos)
        Fatal() << current_file << ": unclosed comment";
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
        Fatal() << current_file << ": unclosed string literal";
      vec.push_back(input.substr(0, pos + 1));
      input = input.substr(pos + 1);
      continue;
    }

    i64 pos = input.find_first_not_of(
      "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
      "0123456789_.$/\\~=+[]*?-!^:");

    if (pos == 0)
      pos = 1;
    vec.push_back(input.substr(0, pos));
    input = input.substr(pos);
  }
  return vec;
}

static std::span<std::string_view>
skip(std::span<std::string_view> tok, std::string_view str) {
  if (tok.empty() || tok[0] != str)
    Fatal() << current_file << ": expected '" << str << "'";
  return tok.subspan(1);
}

static std::string_view unquote(std::string_view s) {
  if (s.size() > 0 && s[0] == '"') {
    assert(s[s.size() - 1] == '"');
    return s.substr(1, s.size() - 2);
  }
  return s;
}

static std::span<std::string_view> read_output_format(std::span<std::string_view> tok) {
  tok = skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.subspan(1);
  if (tok.empty())
    Fatal() << current_file << ": expected ')'";
  return tok.subspan(1);
}

static MemoryMappedFile *resolve_path(std::string str, ReadContext &ctx) {
  if (str.starts_with("/"))
    return MemoryMappedFile::must_open(config.sysroot + str);

  if (str.starts_with("-l"))
    return find_library(str.substr(2), config.library_paths, ctx);

  if (std::string path = path_dirname(current_file) + "/";
      MemoryMappedFile *mb = MemoryMappedFile::open(path + str))
    return mb;

  if (MemoryMappedFile *mb = MemoryMappedFile::open(str))
    return mb;

  for (std::string_view dir : config.library_paths) {
    std::string root = dir.starts_with("/") ? config.sysroot : "";
    std::string path = root + std::string(dir) + "/" + str;
    if (MemoryMappedFile *mb = MemoryMappedFile::open(path))
      return mb;
  }

  Fatal() << current_file << ": library not found: " << str;
}

static std::span<std::string_view>
read_group(std::span<std::string_view> tok, ReadContext &ctx) {
  tok = skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      bool orig = ctx.as_needed;
      ctx.as_needed = true;
      tok = read_group(tok.subspan(1), ctx);
      ctx.as_needed = orig;
      continue;
    }

    MemoryMappedFile *mb = resolve_path(std::string(unquote(tok[0])), ctx);
    read_file(mb, ctx);
    tok = tok.subspan(1);
  }

  if (tok.empty())
    Fatal() << current_file << ": expected ')'";
  return tok.subspan(1);
}

void parse_linker_script(MemoryMappedFile *mb, ReadContext &ctx) {
  current_file = mb->name;

  std::vector<std::string_view> vec = tokenize(mb->get_contents());
  std::span<std::string_view> tok = vec;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT")
      tok = read_output_format(tok.subspan(1));
    else if (tok[0] == "INPUT" || tok[0] == "GROUP")
      tok = read_group(tok.subspan(1), ctx);
    else
      Fatal() << current_file << ": unknown token: " << tok[0];
  }
}

static bool read_label(std::span<std::string_view> &tok,
                       std::string label) {
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

void parse_version_script(std::string path) {
  current_file = path;

  MemoryMappedFile *mb = MemoryMappedFile::must_open(path);
  std::vector<std::string_view> vec = tokenize(mb->get_contents());
  std::span<std::string_view> tok = vec;
  i16 next_ver = VER_NDX_LAST_RESERVED + 1;

  while (!tok.empty()) {
    i16 ver = VER_NDX_GLOBAL;
    if (tok[0] != "{") {
      ver = next_ver++;
      config.version_definitions.push_back(tok[0]);
      tok = tok.subspan(1);
    }

    tok = skip(tok, "{");
    bool is_global = true;

    while (!tok.empty() && tok[0] != "}") {
      if (read_label(tok, "global")) {
        is_global = true;
        continue;
      }

      if (read_label(tok, "local")) {
        is_global = false;
        continue;
      }

      if (tok[0] == "*")
        config.default_version = (is_global ? ver : VER_NDX_LOCAL);
      else
        config.version_patterns.push_back({tok[0], ver});
      tok = skip(tok.subspan(1), ";");
    }

    tok = skip(tok, "}");
    tok = skip(tok, ";");
  }

  if (!tok.empty())
    Fatal() << current_file << ": trailing garbage token: " << tok[0];
}

void parse_dynamic_list(std::string path) {
  current_file = path;

  MemoryMappedFile *mb = MemoryMappedFile::must_open(path);
  std::vector<std::string_view> vec = tokenize(mb->get_contents());
  std::span<std::string_view> tok = vec;

  tok = skip(tok, "{");
  i64 ver = VER_NDX_GLOBAL;

  while (!tok.empty() && tok[0] != "}") {
    if (read_label(tok, "global")) {
      ver = VER_NDX_GLOBAL;
      continue;
    }

    if (read_label(tok, "local")) {
      ver = VER_NDX_LOCAL;
      continue;
    }

    if (tok[0] == "*")
      config.default_version = ver;
    else
      config.version_patterns.push_back({tok[0], ver});
    tok = skip(tok.subspan(1), ";");
  }

  tok = skip(tok, "}");
  tok = skip(tok, ";");

  if (!tok.empty())
    Fatal() << current_file << ": trailing garbage token: " << tok[0];
}
