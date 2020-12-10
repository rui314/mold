#include "mold.h"

#include "llvm/Support/FileSystem.h"

using namespace llvm;
using namespace llvm::ELF;
using namespace llvm::sys;

static thread_local StringRef script_path;
static thread_local StringRef script_dir;

static std::vector<StringRef> tokenize(StringRef input) {
  std::vector<StringRef> vec;
  while (!input.empty()) {
    if (input[0] == ' ' || input[0] == '\t' || input[0] == '\n') {
      input = input.substr(1);
      continue;
    }

    if (input.startswith("/*")) {
      int pos = input.find("*/", 2);
      if (pos == StringRef::npos)
        error("unclosed comment");
      input = input.substr(pos + 2);
      continue;
    }

    if (input[0] == '#') {
      int pos = input.find("\n", 1);
      if (pos == StringRef::npos)
        break;
      input = input.substr(pos + 1);
      continue;
    }

    if (input[0] == '"') {
      int pos = input.find('"', 1);
      if (pos == StringRef::npos)
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

static std::span<StringRef> skip(std::span<StringRef> tok, StringRef str) {
  if (tok.empty() || tok[0] != str)
    error("expected '" + str + "'");
  return tok.subspan(1);
}

static std::span<StringRef> read_output_format(std::span<StringRef> tok) {
  tok = skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.subspan(1);
  if (tok.empty())
    error("expected ')'");
  return tok.subspan(1);
}

static MemoryBufferRef resolve_path(StringRef str) {
  if (str.startswith("/"))
    return must_open_input_file(config.sysroot + str);
  if (str.startswith("-l"))
    return find_library(str.substr(2));
  if (std::string path = (script_dir + "/" + str).str(); fs::exists(path))
    return must_open_input_file(path);
  if (MemoryBufferRef *mb = open_input_file(str))
    return *mb;
  for (StringRef dir : config.library_paths) {
    std::string root = dir.startswith("/") ? config.sysroot : "";
    if (MemoryBufferRef *mb = open_input_file(root + dir + "/" + str))
      return *mb;
  }
  error("library not found: " + str);
}

static std::span<StringRef> read_group(std::span<StringRef> tok) {
  tok = skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      bool orig = config.as_needed;
      tok = read_group(tok.subspan(1));
      config.as_needed = orig;
      continue;
    }

    read_file(resolve_path(tok[0]));
    tok = tok.subspan(1);
  }

  if (tok.empty())
    error("expected ')'");
  return tok.subspan(1);
}

void parse_linker_script(StringRef path, StringRef input) {
  script_path = path;
  script_dir = path.substr(0, path.find_last_of('/'));

  std::vector<StringRef> vec = tokenize(input);
  std::span<StringRef> tok = vec;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT")
      tok = read_output_format(tok.subspan(1));
    else if (tok[0] == "INPUT" || tok[0] == "GROUP")
      tok = read_group(tok.subspan(1));
    else
      error(path + ": unknown token: " + tok[0]);
  }
}

void parse_version_script(StringRef path) {
  script_path = path;
  script_dir = path.substr(0, path.find_last_of('/'));

  MemoryBufferRef mb = must_open_input_file(path);
  std::vector<StringRef> vec = tokenize(mb.getBuffer());
  std::span<StringRef> tok = vec;
  tok = skip(tok, "{");

  std::vector<StringRef> locals;
  std::vector<StringRef> globals;
  std::vector<StringRef> *cur = &locals;

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

    cur->push_back(tok[0]);
    tok = skip(tok.subspan(1), ";");
  }

  tok = skip(tok, "}");
  tok = skip(tok, ";");

  if (!tok.empty())
    error(path + ": trailing garbage token: " + tok[0]);

  if (locals.size() != 1 || locals[0] != "*")
    error(path + ": unsupported version script");
  config.export_dynamic = false;
  config.globals = globals;
}
