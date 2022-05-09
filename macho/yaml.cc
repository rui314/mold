// On macOS, a .tbd file can be used instead of a .dylib to link
// against a dynamic library. A .tbd file is a text file in the YAML
// format, so the linker has to be able to parse a YAML file. This
// file implements a YAML parser.
//
// We implemented a YAML parser ourselves instead of using an external
// library. We didn't want to add an dependency to an external library
// just to read .tdb files.
//
// YAML as a format is very complicated. For example, the official
// YAML specification allows embedding a JSON string into YAML; so it
// is strictly larger than JSON. It has surprisingly many features
// that most users are not aware of. Fortunately, we have to support
// only a small portion of the spec to read a .tbd file.

#include "mold.h"

#include <optional>

namespace mold::macho {

struct Token {
  enum { STRING = 1, INDENT, DEDENT, END };

  u8 kind = 0;
  std::string_view str;
};

class YamlParser {
public:
  YamlParser(std::string_view input) : input(input) {}

  std::variant<std::vector<YamlNode>, YamlError> parse();

private:
  std::optional<YamlError> tokenize();

  void tokenize_bare_string(std::string_view &str);

  std::optional<YamlError> tokenize_list(std::string_view &str);
  std::optional<YamlError> tokenize_string(std::string_view &str, char end);

  std::variant<YamlNode, YamlError> parse_element(std::span<Token> &tok);
  std::variant<YamlNode, YamlError> parse_list(std::span<Token> &tok);
  std::variant<YamlNode, YamlError> parse_map(std::span<Token> &tok);
  std::variant<YamlNode, YamlError> parse_flow_element(std::span<Token> &tok);
  std::variant<YamlNode, YamlError> parse_flow_list(std::span<Token> &tok);

  std::string_view input;
  std::vector<Token> tokens;
};

std::optional<YamlError> YamlParser::tokenize() {
  std::vector<i64> indents = {0};

  auto indent = [&](std::string_view str, i64 depth) {
    tokens.push_back({Token::INDENT, str});
    indents.push_back(depth);
  };

  auto dedent = [&](std::string_view str) {
    assert(indents.size() > 1);
    tokens.push_back({Token::DEDENT, str});
    indents.pop_back();
  };

  auto skip_line = [](std::string_view &str) {
    size_t pos = str.find('\n');
    if (pos == str.npos)
      str = str.substr(str.size());
    else
      str = str.substr(pos + 1);
  };

  auto tokenize_line = [&](std::string_view &str) -> std::optional<YamlError> {
    const char *start = str.data();

    if (str.starts_with("---")) {
      while (indents.size() > 1)
        dedent(str);
      tokens.push_back({Token::END, str.substr(0, 3)});
      skip_line(str);
      return {};
    }

    if (str.starts_with("...")) {
      while (indents.size() > 1)
        dedent(str);
      tokens.push_back({Token::END, str.substr(0, 3)});
      str = str.substr(str.size());
      return {};
    }

    size_t pos = str.find_first_not_of(" \t");
    if (pos == str.npos || str[pos] == '#' || str[pos] == '\n') {
      skip_line(str);
      return {};
    }

    if (indents.back() != pos) {
      if (indents.back() < pos) {
        indent(str, pos);
      } else {
        while (indents.back() != pos) {
          if (pos < indents.back())
            dedent(str);
          else
            return YamlError{"bad indentation", start - input.data()};
        }
      }
    }

    str = str.substr(pos);

    while (!str.empty()) {
      if (str[0] == '\n') {
        str = str.substr(1);
        return {};
      }

      if (str.starts_with("- ")) {
        tokens.push_back({'-', str.substr(0, 1)});

        size_t pos = str.find_first_not_of(" \t", 1);
        if (pos == str.npos || str[pos] == '\n') {
          skip_line(str);
          return {};
        }

        str = str.substr(pos);
        indent(str, str.data() - start);
        continue;
      }

      if (str.starts_with('['))
        return tokenize_list(str);

      if (str.starts_with('\'')) {
        if (std::optional<YamlError> err = tokenize_string(str, '\''))
          return err;
        continue;
      }

      if (str.starts_with('"')) {
        if (std::optional<YamlError> err = tokenize_string(str, '"'))
          return err;
        continue;
      }

      if (str.starts_with('#')) {
        skip_line(str);
        return {};
      }

      if (str.starts_with(':')) {
        tokens.push_back({':', str.substr(0, 1)});

        size_t pos = str.find_first_not_of(" \t", 1);
        if (pos == str.npos || str[pos] == '\n') {
          skip_line(str);
          return {};
        }

        str = str.substr(pos);
        continue;
      }

      tokenize_bare_string(str);
    }
    return {};
  };

  std::string_view str = input;
  while (!str.empty())
    if (std::optional<YamlError> err = tokenize_line(str))
      return err;
  return {};
}

std::optional<YamlError> YamlParser::tokenize_list(std::string_view &str) {
  const char *start = str.data();

  tokens.push_back({'[', str.substr(0, 1)});
  str = str.substr(1);

  while (!str.empty() && str[0] != ']') {
    if (size_t pos = str.find_first_not_of(" \t\n"); pos) {
      str = str.substr(pos);
      continue;
    }

    if (str.starts_with('\'')) {
      if (std::optional<YamlError> err = tokenize_string(str, '\''))
        return err;
      continue;
    }

    if (str.starts_with('"')) {
      if (std::optional<YamlError> err = tokenize_string(str, '"'))
        return err;
      continue;
    }

    if (str.starts_with(',')) {
      tokens.push_back({',', str.substr(0, 1)});
      str = str.substr(1);
      continue;
    }

    tokenize_bare_string(str);
  }

  if (str.empty())
    return YamlError{"unclosed list", start - input.data()};

  const char *bracket = str.data();
  tokens.push_back({']', str.substr(0, 1)});
  str = str.substr(1);

  while (!str.empty() && (str[0] == ' ' || str[0] == '\t'))
    str = str.substr(1);
  if (str.empty() || str[0] != '\n')
    return YamlError{"no newline after ']'", bracket - input.data()};
  str = str.substr(1);
  return {};
}

std::optional<YamlError>
YamlParser::tokenize_string(std::string_view &str, char end) {
  const char *start = str.data();
  size_t pos = str.find(end, 1);
  if (pos == str.npos)
    return YamlError{"unterminated string literal", start - input.data()};

  tokens.push_back({Token::STRING, str.substr(1, pos - 1)});
  str = str.substr(pos + 1);
  return {};
}

void
YamlParser::tokenize_bare_string(std::string_view &str) {
  size_t pos = str.find_first_not_of(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-/.");
  if (pos == str.npos)
    pos = str.size();
  tokens.push_back({Token::STRING, str.substr(0, pos)});
  str = str.substr(pos);
}

std::variant<std::vector<YamlNode>, YamlError> YamlParser::parse() {
  if (std::optional<YamlError> err = tokenize())
    return *err;

  std::span<Token> tok(tokens);
  std::vector<YamlNode> vec;

  while (!tok.empty()) {
    if (tok[0].kind == Token::END) {
      tok = tok.subspan(1);
      continue;
    }

    std::variant<YamlNode, YamlError> elem = parse_element(tok);
    if (YamlError *err = std::get_if<YamlError>(&elem))
      return *err;
    vec.push_back(std::get<YamlNode>(elem));

    if (tok[0].kind != Token::END)
      return YamlError{"stray token", tok[0].str.data() - input.data()};
  }
  return vec;
}

std::variant<YamlNode, YamlError>
YamlParser::parse_element(std::span<Token> &tok) {
  if (tok[0].kind == Token::INDENT) {
    tok = tok.subspan(1);

    std::variant<YamlNode, YamlError> elem = parse_element(tok);
    assert(tok[0].kind == Token::DEDENT);
    tok = tok.subspan(1);
    return elem;
  }

  if (tok[0].kind == '-')
    return parse_list(tok);

  if (tok.size() > 2 && tok[0].kind == Token::STRING && tok[1].kind == ':')
    return parse_map(tok);

  return parse_flow_element(tok);
}

std::variant<YamlNode, YamlError>
YamlParser::parse_list(std::span<Token> &tok) {
  std::vector<YamlNode> vec;

  while (tok[0].kind != Token::END && tok[0].kind != Token::DEDENT) {
    if (tok[0].kind != '-')
      return YamlError{"list element expected", tok[0].str.data() - input.data()};
    tok = tok.subspan(1);

    std::variant<YamlNode, YamlError> elem = parse_element(tok);
    if (YamlError *err = std::get_if<YamlError>(&elem))
      return *err;
    vec.push_back(std::get<YamlNode>(elem));
  }
  return YamlNode{vec};
}

std::variant<YamlNode, YamlError>
YamlParser::parse_map(std::span<Token> &tok) {
  std::map<std::string_view, YamlNode> map;

  while (tok[0].kind != Token::END && tok[0].kind != Token::DEDENT) {
    if (tok.size() < 2 || tok[0].kind != Token::STRING || tok[1].kind != ':')
      return YamlError{"map key expected", tok[0].str.data() - input.data()};

    std::string_view key = tok[0].str;
    tok = tok.subspan(2);

    std::variant<YamlNode, YamlError> elem = parse_element(tok);
    if (YamlError *err = std::get_if<YamlError>(&elem))
      return *err;
    map[key] = std::get<YamlNode>(elem);
  }
  return YamlNode{map};
}

std::variant<YamlNode, YamlError>
YamlParser::parse_flow_element(std::span<Token> &tok) {
  if (tok[0].kind == '[') {
    tok = tok.subspan(1);
    return parse_flow_list(tok);
  }

  if (tok[0].kind != Token::STRING)
    return YamlError{"scalar expected", tok[0].str.data() - input.data()};

  std::string_view val = tok[0].str;
  tok = tok.subspan(1);
  return YamlNode{val};
}

std::variant<YamlNode, YamlError>
YamlParser::parse_flow_list(std::span<Token> &tok) {
  std::vector<YamlNode> vec;
  const char *start = tok[0].str.data();

  while (tok[0].kind != ']' && tok[0].kind != Token::END) {
    std::variant<YamlNode, YamlError> elem = parse_flow_element(tok);
    if (YamlError *err = std::get_if<YamlError>(&elem))
      return *err;
    vec.push_back(std::get<YamlNode>(elem));

    if (tok[0].kind == ']')
      break;
    if (tok[0].kind != ',')
      return YamlError{"comma expected", tok[0].str.data() - input.data()};
    tok = tok.subspan(1);
  }

  if (tok[0].kind == Token::END)
    return YamlError{"unterminated flow list", start - input.data()};

  tok = tok.subspan(1);
  return YamlNode{vec};
}

std::variant<std::vector<YamlNode>, YamlError>
parse_yaml(std::string_view str) {
  return YamlParser(str).parse();
}

} // namespace mold::macho
