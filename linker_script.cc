#include "mold.h"

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

static ArrayRef<StringRef> skip(ArrayRef<StringRef> tok, StringRef str) {
  if (tok.empty() || tok[0] != str)
    error("expected '" + str + "'");
  return tok.slice(1);
}

static ArrayRef<StringRef> read_output_format(ArrayRef<StringRef> tok) {
  tok = skip(tok, "(");
  while (!tok.empty() && tok[0] != ")")
    tok = tok.slice(1);
  if (tok.empty())
    error("expected ')'");
  return tok.slice(1);
}

static ArrayRef<StringRef> read_group(ArrayRef<StringRef> tok) {
  tok = skip(tok, "(");

  while (!tok.empty() && tok[0] != ")") {
    if (tok[0] == "AS_NEEDED") {
      tok = read_group(tok.slice(1));
      continue;
    }

    read_file(tok[0]);
    tok = tok.slice(1);
  }

  if (tok.empty())
    error("expected ')'");
  return tok.slice(1);
}

void parse_linker_script(StringRef input) {
  std::vector<StringRef> vec = tokenize(input);
  ArrayRef<StringRef> tok = vec;

  while (!tok.empty()) {
    if (tok[0] == "OUTPUT_FORMAT")
      tok = read_output_format(tok.slice(1));
    else if (tok[0] == "GROUP")
      tok = read_group(tok.slice(1));
    else
      error("unknown token: " + tok[0]);
  }
}
