#include "mold.h"

namespace mold::macho {

struct Token {
  enum { STRING = 1, INDENT, DEDENT, END };

  u8 kind = 0;
  std::string_view str;
};

class YamlParser {
public:
  YamlParser(std::string_view input) : input(input) {}

  std::vector<YamlNode> parse(Context &ctx);
  void dump(Context &ctx);

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

  YamlNode parse_element(Context &ctx, std::span<Token> &tok);
  YamlNode parse_list(Context &ctx, std::span<Token> &tok);
  YamlNode parse_map(Context &ctx, std::span<Token> &tok);
  YamlNode parse_flow_element(Context &ctx, std::span<Token> &tok);
  YamlNode parse_flow_list(Context &ctx, std::span<Token> &tok);

  std::string_view input;
};

std::vector<Token> YamlParser::tokenize(Context &ctx) {
  std::vector<Token> tokens;
  std::vector<i64> indents = {0};

  auto indent = [&](i64 depth) {
    tokens.push_back({Token::INDENT, ""});
    indents.push_back(depth);
  };

  auto dedent = [&]() {
    assert(indents.size() > 1);
    tokens.push_back({Token::DEDENT, ""});
    indents.pop_back();
  };

  auto skip_line = [](std::string_view str) {
    size_t pos = str.find('\n');
    if (pos == str.npos)
      return str.substr(str.size());
    return str.substr(pos + 1);
  };

  auto tokenize_line = [&](std::string_view str) {
    const char *start = str.data();

    if (str.starts_with("---")) {
      while (indents.size() > 1)
        dedent();
      tokens.push_back({Token::END, str.substr(0, 3)});
      return skip_line(str);
    }

    size_t pos = str.find_first_not_of(" \t");
    if (pos == str.npos || str[pos] == '#' || str[pos] == '\n')
      return skip_line(str);

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

    str = str.substr(pos);

    while (!str.empty()) {
      if (str[0] == '\n')
        return str.substr(1);

      if (str.starts_with("- ")) {
        tokens.push_back({'-', str.substr(0, 1)});
        size_t pos = str.find_first_not_of(" \t", 1);
        if (pos == str.npos || str[pos] == '\n')
          return skip_line(str);
        str = str.substr(pos);
        indent(str.data() - start);
        continue;
      }

      if (str.starts_with('['))
        return tokenize_list(ctx, tokens, str);

      if (str.starts_with('\'')) {
        str = tokenize_string(ctx, tokens, str, '\'');
        continue;
      }

      if (str.starts_with('"')) {
        str = tokenize_string(ctx, tokens, str, '"');
        continue;
      }

      if (str.starts_with('#'))
        return skip_line(str);

      if (str.starts_with(':')) {
        tokens.push_back({':', str.substr(0, 1)});
        size_t pos = str.find_first_not_of(" \t", 1);
        if (pos == str.npos || str[pos] == '\n')
          return skip_line(str);
        str = str.substr(pos);
        continue;
      }

      str = tokenize_bare_string(ctx, tokens, str);
    }
    return str;
  };

  std::string_view str = input;
  while (!str.empty())
    str = tokenize_line(str);

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
    if (size_t pos = str.find_first_not_of(" \t\n"); pos) {
      str = str.substr(pos);
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
    Fatal(ctx) << "unclosed list";

  tokens.push_back({']', str.substr(0, 1)});
  str = str.substr(1);

  while (!str.empty() && (str[0] == ' ' || str[0] == '\t'))
    str = str.substr(1);
  if (str.empty() || str[0] != '\n')
    Fatal(ctx) << "no newline after '['";
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

void YamlParser::dump(Context &ctx) {
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
    case Token::END:
      SyncOut(ctx) << "END";
      break;
    default:
      SyncOut(ctx) << "'" << (char)tok.kind << "'";
      break;
    }
  }
}

std::vector<YamlNode> YamlParser::parse(Context &ctx) {
  std::vector<Token> tokens = tokenize(ctx);
  std::span<Token> tok(tokens);

  std::vector<YamlNode> vec;

  while (!tok.empty()) {
    if (tok[0].kind == Token::END) {
      tok = tok.subspan(1);
      continue;
    }

    vec.push_back(parse_element(ctx, tok));
    if (tok[0].kind != Token::END)
      Fatal(ctx) << "stray token";
  }
  return vec;
}

YamlNode YamlParser::parse_element(Context &ctx, std::span<Token> &tok) {
  if (tok[0].kind == Token::INDENT) {
    tok = tok.subspan(1);
    YamlNode node = parse_element(ctx, tok);
    assert(tok[0].kind == Token::DEDENT);
    tok = tok.subspan(1);
    return node;
  }

  if (tok[0].kind == '-')
    return parse_list(ctx, tok);

  if (tok.size() > 2 && tok[0].kind == Token::STRING && tok[1].kind == ':')
    return parse_map(ctx, tok);

  return parse_flow_element(ctx, tok);
}

YamlNode YamlParser::parse_list(Context &ctx, std::span<Token> &tok) {
  std::vector<YamlNode> vec;

  while (tok[0].kind != Token::END && tok[0].kind != Token::DEDENT) {
    if (tok[0].kind != '-')
      Fatal(ctx) << "list element expected";
    tok = tok.subspan(1);
    vec.push_back(parse_element(ctx, tok));
  }
  return {vec};
}

YamlNode YamlParser::parse_map(Context &ctx, std::span<Token> &tok) {
  std::unordered_map<std::string_view, YamlNode> map;

  while (tok[0].kind != Token::END && tok[0].kind != Token::DEDENT) {
    if (tok.size() < 2 || tok[0].kind != Token::STRING || tok[1].kind != ':')
      Fatal(ctx) << "map key expected";

    std::string_view key = tok[0].str;
    tok = tok.subspan(2);
    map[key] = parse_element(ctx, tok);
  }
  return {map};
}

YamlNode YamlParser::parse_flow_element(Context &ctx, std::span<Token> &tok) {
  if (tok[0].kind == '[') {
    tok = tok.subspan(1);
    return parse_flow_list(ctx, tok);
  }

  if (tok[0].kind != Token::STRING)
    Fatal(ctx) << "scalar expected";

  std::string_view val = tok[0].str;
  tok = tok.subspan(1);
  return {val};
}

YamlNode YamlParser::parse_flow_list(Context &ctx, std::span<Token> &tok) {
  std::vector<YamlNode> vec;
  while (tok[0].kind != ']' && tok[0].kind != Token::END) {
    vec.push_back(parse_flow_element(ctx, tok));
    if (tok[0].kind == ']')
      break;
    if (tok[0].kind != ',')
      Fatal(ctx) << "comma expected";
    tok = tok.subspan(1);
  }

  if (tok[0].kind == Token::END)
    Fatal(ctx) << "unterminated flow list";
  tok = tok.subspan(1);
  return {vec};
}

std::vector<YamlNode> parse_yaml(Context &ctx, std::string_view str) {
  return YamlParser(str).parse(ctx);
}

void dump_yaml(Context &ctx, YamlNode &node, i64 depth) {
  if (auto *elem = std::get_if<std::string_view>(&node.data)) {
    SyncOut(ctx) << std::string(depth * 2, ' ') << '"' << *elem << '"';
    return;
  }

  if (auto *elem = std::get_if<std::vector<YamlNode>>(&node.data)) {
    SyncOut(ctx) << std::string(depth * 2, ' ') << "vector:";
    for (YamlNode &child : *elem)
      dump_yaml(ctx, child, depth + 1);
    return;
  }

  auto *elem =
    std::get_if<std::unordered_map<std::string_view, YamlNode>>(&node.data);
  assert(elem);

  SyncOut(ctx) << std::string(depth * 2, ' ') << "map:";
  for (std::pair<const std::string_view, YamlNode> &kv : *elem) {
    SyncOut(ctx) << std::string(depth * 2 + 2, ' ') << "key: " << kv.first;
    dump_yaml(ctx, kv.second, depth + 1);
  }
}

} // namespace mold::macho
