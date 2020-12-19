#include "mold.h"

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

static void skip(std::span<std::string_view> &tok, std::string_view str) {
  if (tok.empty() || tok[0] != str)
    error("expected '" + std::string(str) + "'");
  tok = tok.subspan(1);
}

static void read_output_format(std::span<std::string_view> &tok) {
  skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.subspan(1);
  if (tok.empty())
    error("expected ')'");
  tok = tok.subspan(1);
}

static MemoryMappedFile resolve_path(std::string str) {
  if (str.starts_with("/"))
    return must_open_input_file(config.sysroot + str);
  if (str.starts_with("-l"))
    return find_library(str.substr(2));
  if (MemoryMappedFile *mb = open_input_file(script_dir + "/" + str))
    return *mb;
  if (MemoryMappedFile *mb = open_input_file(str))
    return *mb;
  for (std::string &dir : config.library_paths) {
    std::string root = dir.starts_with("/") ? config.sysroot : "";
    if (MemoryMappedFile *mb = open_input_file(root + dir + "/" + str))
      return *mb;
  }
  error("library not found: " + str);
}

static std::vector<InputFile *>
read_group(std::span<std::string_view> &tok, bool as_needed) {
  std::vector<InputFile *> files;
  skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      bool orig = config.as_needed;
      tok = tok.subspan(1);
      for (InputFile *file : read_group(tok, true))
        files.push_back(file);
      continue;
    }

    for (InputFile *file : read_file(resolve_path(std::string(tok[0])), as_needed))
      files.push_back(file);
    tok = tok.subspan(1);
  }

  if (tok.empty())
    error("expected ')'");
  tok = tok.subspan(1);
  return files;
}

std::vector<InputFile *> parse_linker_script(MemoryMappedFile mb, bool as_needed) {
  script_dir = mb.name.substr(0, mb.name.find_last_of('/'));

  std::vector<std::string_view> tokens = tokenize({(char *)mb.data, mb.size});
  std::span<std::string_view> tok = tokens;
  std::vector<InputFile *> files;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT") {
      tok = tok.subspan(1);
      read_output_format(tok);
    } else if (tok[0] == "INPUT" || tok[0] == "GROUP") {
      tok = tok.subspan(1);
      for (InputFile *file : read_group(tok, as_needed))
        files.push_back(file);
    } else {
      error(mb.name + ": unknown token: " + std::string(tok[0]));
    }
  }
  return files;
}

void parse_version_script(std::string path) {
  script_dir = path.substr(0, path.find_last_of('/'));

  MemoryMappedFile mb = must_open_input_file(path);
  std::vector<std::string_view> vec = tokenize({(char *)mb.data, mb.size});
  std::span<std::string_view> tok = vec;
  skip(tok, "{");

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
    tok = tok.subspan(1);
    skip(tok, ";");
  }

  skip(tok, "}");
  skip(tok, ";");

  if (!tok.empty())
    error(path + ": trailing garbage token: " + std::string(tok[0]));

  if (locals.size() != 1 || locals[0] != "*")
    error(path + ": unsupported version script");
  config.export_dynamic = false;
  config.globals = globals;
}
