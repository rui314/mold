#include "mold.h"

#include "llvm/Support/FileSystem.h"

using namespace llvm;
using namespace llvm::sys;

static thread_local std::string script_path;
static thread_local std::string script_dir;

static std::vector<std::string_view> tokenize(std::string_view input) {
  std::vector<std::string_view> vec;
  while (!input.empty()) {
    if (input[0] == ' ' || input[0] == '\t' || input[0] == '\n') {
      input = input.substr(1);
      continue;
    }

    if (input.starts_with("/*")) {
      int pos = input.find("*/", 2);
      if (pos == std::string_view::npos)
        error("unclosed comment");
      input = input.substr(pos + 2);
      continue;
    }

    if (input[0] == '#') {
      int pos = input.find("\n", 1);
      if (pos == std::string_view::npos)
        break;
      input = input.substr(pos + 1);
      continue;
    }

    if (input[0] == '"') {
      int pos = input.find('"', 1);
      if (pos == std::string_view::npos)
        error("unclosed string literal");
      vec.push_back(input.substr(0, pos));
      input = input.substr(pos);
      continue;
    }

    int pos = input.find_first_not_of(
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
    error("expected '" + std::string(str) + "'");
  return tok.subspan(1);
}

static std::span<std::string_view> read_output_format(std::span<std::string_view> tok) {
  tok = skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.subspan(1);
  if (tok.empty())
    error("expected ')'");
  return tok.subspan(1);
}

static MemoryMappedFile resolve_path(std::string str) {
  if (str.starts_with("/"))
    return must_open_input_file(config.sysroot + str);
  if (str.starts_with("-l"))
    return find_library(str.substr(2));
  if (std::string path = script_dir + "/" + str; fs::exists(path))
    return must_open_input_file(path);
  if (MemoryMappedFile *mb = open_input_file(str))
    return *mb;
  for (std::string &dir : config.library_paths) {
    std::string root = dir.starts_with("/") ? config.sysroot : "";
    if (MemoryMappedFile *mb = open_input_file(root + dir + "/" + str))
      return *mb;
  }
  error("library not found: " + str);
}

static std::span<std::string_view> read_group(std::span<std::string_view> tok) {
  tok = skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      bool orig = config.as_needed;
      tok = read_group(tok.subspan(1));
      config.as_needed = orig;
      continue;
    }

    read_file(resolve_path(std::string(tok[0])));
    tok = tok.subspan(1);
  }

  if (tok.empty())
    error("expected ')'");
  return tok.subspan(1);
}

void parse_linker_script(MemoryMappedFile mb) {
  script_path = mb.name;
  script_dir = mb.name.substr(0, mb.name.find_last_of('/'));

  std::vector<std::string_view> vec = tokenize({(char *)mb.data, mb.size});
  std::span<std::string_view> tok = vec;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT")
      tok = read_output_format(tok.subspan(1));
    else if (tok[0] == "INPUT" || tok[0] == "GROUP")
      tok = read_group(tok.subspan(1));
    else
      error(mb.name + ": unknown token: " + std::string(tok[0]));
  }
}

void parse_version_script(std::string path) {
  script_path = path;
  script_dir = path.substr(0, path.find_last_of('/'));

  MemoryMappedFile mb = must_open_input_file(path);
  std::vector<std::string_view> vec = tokenize({(char *)mb.data, mb.size});
  std::span<std::string_view> tok = vec;
  tok = skip(tok, "{");

  std::vector<std::string> locals;
  std::vector<std::string> globals;
  std::vector<std::string> *cur = &locals;

  while (!tok.empty() && tok[0] != "}") {
    if (tok[0] == "local:") {
      cur = &locals;
      tok = tok.subspan(1);
      continue;
    }

    if (tok.size() >= 2 && tok[0] == "local" && tok[1] == ":") {
      cur = &locals;
      tok = tok.subspan(2);
      continue;
    }

    if (tok[0] == "global:") {
      cur = &globals;
      tok = tok.subspan(1);
      continue;
    }

    if (tok.size() >= 2 && tok[0] == "global" && tok[1] == ":") {
      cur = &globals;
      tok = tok.subspan(2);
      continue;
    }

    cur->push_back(std::string(tok[0]));
    tok = skip(tok.subspan(1), ";");
  }

  tok = skip(tok, "}");
  tok = skip(tok, ";");

  if (!tok.empty())
    error(path + ": trailing garbage token: " + std::string(tok[0]));

  if (locals.size() != 1 || locals[0] != "*")
    error(path + ": unsupported version script");
  config.export_dynamic = false;
  config.globals = globals;
}
