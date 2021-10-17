#include "mold.h"

namespace mold::macho {

struct Token {
  enum { STRING = 1, INDENT, DEDENT, RESET, END };

  u8 kind = 0;
  std::string_view str;
};

class YamlParser {
public:
  YamlParser(std::string_view input) : input(input) {}

  YamlNode parse(Context &ctx);

private:
  std::vector<Token> tokenize(Context &ctx);
  void tokenize_line(Context &ctx, std::vector<Token> &tokens,
                     std::string_view line);

  std::string_view
  tokenize_bare_string(Context &ctx, std::vector<Token> &tokens,
                       std::string_view str);

  std::string_view
  tokenize_list(Context &ctx, std::vector<Token> &tokens, std::string_view str);

  std::string_view
  tokenize_string(Context &ctx, std::vector<Token> &tokens,
                  std::string_view str, char end);

  std::string_view input;
};

std::vector<Token> YamlParser::tokenize(Context &ctx) {
  std::vector<Token> tokens;
  std::vector<i64> indents = {0};
  std::string_view str = input;

  auto indent = [&](i64 depth) {
    tokens.push_back({Token::INDENT, str});
    indents.push_back(depth);
  };

  auto dedent = [&]() {
    assert(indents.size() > 1);
    tokens.push_back({Token::DEDENT, str});
    indents.pop_back();
  };

  auto tokenize_line = [&](std::string_view line) {
    if (line.empty())
      return;

    const char *start = line.data();

    if (line.starts_with("---")) {
      while (indents.size() > 1)
        dedent();
      tokens.push_back({Token::RESET, line.substr(0, 3)});
      return;
    }

    size_t pos = line.find_first_not_of(" \t");
    if (pos == line.npos || line[pos] == '#')
      return;

    if (indents.back() != pos) {
      if (indents.back() < pos) {
        indent(pos);
      } else {
        while (indents.back() != pos) {
          if (pos < indents.back())
            dedent();
          else
            Fatal(ctx) << "bad indentation";
        }
      }
    }

    line = line.substr(pos);

    while (!line.empty()) {
      if (line.starts_with("- ")) {
        tokens.push_back({'-', line.substr(0, 1)});
        size_t pos = line.find_first_not_of(' ', 1);
        if (pos == line.npos)
          return;
        line = line.substr(pos);
        indent(line.data() - start);
        continue;
      }

      if (line.starts_with('[')) {
        line = tokenize_list(ctx, tokens, line);
        continue;
      }

      if (line.starts_with('\'')) {
        line = tokenize_string(ctx, tokens, line, '\'');
        continue;
      }

      if (line.starts_with('"')) {
        line = tokenize_string(ctx, tokens, line, '"');
        continue;
      }

      if (line.starts_with(',')) {
        tokens.push_back({(u8)line[0], line.substr(0, 1)});
        line = line.substr(1);
        continue;
      }

      if (line.starts_with('#'))
        return;

      if (line.starts_with(':')) {
        tokens.push_back({':', line.substr(0, 1)});
        size_t pos = line.find_first_not_of(' ', 1);
        if (pos == line.npos)
          return;
        line = line.substr(pos);
        continue;
      }

      line = tokenize_bare_string(ctx, tokens, line);
    }
  };

  while (!str.empty()) {
    size_t pos = str.find('\n');
    if (pos == str.npos)
      pos = str.size();
    tokenize_line(str.substr(0, pos));
    str = str.substr(pos + 1);
  }

  while (indents.size() > 1)
    dedent();
  tokens.push_back({Token::END, str});
  return tokens;
}

std::string_view
YamlParser::tokenize_list(Context &ctx, std::vector<Token> &tokens,
                          std::string_view str) {
  tokens.push_back({'[', str.substr(0, 1)});
  str = str.substr(1);

  while (!str.empty() && str[0] != ']') {
    if (str[0] == ' ' || str[0] == '\n') {
      str = str.substr( str.find_first_not_of(" \n"));
      continue;
    }

    if (str.starts_with('\'')) {
      str = tokenize_string(ctx, tokens, str, '\'');
      continue;
    }

    if (str.starts_with('"')) {
      str = tokenize_string(ctx, tokens, str, '"');
      continue;
    }

    if (str.starts_with(',')) {
      tokens.push_back({',', str.substr(0, 1)});
      str = str.substr(1);
      continue;
    }

    str = tokenize_bare_string(ctx, tokens, str);
  }

  if (str.empty())
    Error(ctx) << "unclosed list";
  tokens.push_back({']', str.substr(0, 1)});
  return str.substr(1);
}

std::string_view
YamlParser::tokenize_string(Context &ctx, std::vector<Token> &tokens,
                            std::string_view str, char end) {
  str = str.substr(1);
  size_t pos = str.find(end);
  if (pos == str.npos)
    Fatal(ctx) << "unterminated string literal";
  tokens.push_back({Token::STRING, str.substr(1, pos - 1)});
  return str.substr(pos + 1);
}

std::string_view
YamlParser::tokenize_bare_string(Context &ctx, std::vector<Token> &tokens,
                                 std::string_view str) {
  size_t pos = str.find_first_not_of(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-/.");
  if (pos == str.npos)
    pos = str.size();
  tokens.push_back({Token::STRING, str.substr(0, pos)});
  return str.substr(pos);
}

YamlNode YamlParser::parse(Context &ctx) {
  std::vector<Token> tokens = tokenize(ctx);

  for (Token &tok : tokens) {
    switch (tok.kind) {
    case Token::STRING:
      SyncOut(ctx) << "\"" << tok.str << "\"";
      break;
    case Token::INDENT:
      SyncOut(ctx) << "INDENT";
      break;
    case Token::DEDENT:
      SyncOut(ctx) << "DEDENT";
      break;
    case Token::RESET:
      SyncOut(ctx) << "RESET";
      break;
    case Token::END:
      SyncOut(ctx) << "END";
      break;
    case '\n':
      SyncOut(ctx) << "'\\n'";
      break;
    default:
      SyncOut(ctx) << "'" << (char)tok.kind << "'";
      break;
    }
  }

  return {"foo"};
}

YamlNode parse_yaml(Context &ctx, std::string_view str) {
  return YamlParser(str).parse(ctx);
}

} // namespace mold::macho
