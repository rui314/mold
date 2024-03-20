#pragma once

#include "common.h"

namespace mold {

template <typename Context>
std::vector<std::string_view>
read_response_file(Context &ctx, std::string_view path, i64 depth) {
  if (depth > 10)
    Fatal(ctx) << path << ": response file nesting too deep";

  std::vector<std::string_view> vec;
  MappedFile *mf = must_open_file(ctx, std::string(path));
  std::string_view data((char *)mf->data, mf->size);

  while (!data.empty()) {
    if (isspace(data[0])) {
      data = data.substr(1);
      continue;
    }

    auto read_quoted = [&]() {
      char quote = data[0];
      data = data.substr(1);

      std::string buf;
      while (!data.empty() && data[0] != quote) {
        if (data[0] == '\\' && data.size() >= 1) {
          buf.append(1, data[1]);
          data = data.substr(2);
        } else {
          buf.append(1, data[0]);
          data = data.substr(1);
        }
      }
      if (data.empty())
        Fatal(ctx) << path << ": premature end of input";
      data = data.substr(1);
      return save_string(ctx, buf);
    };

    auto read_unquoted = [&] {
      std::string buf;
      while (!data.empty()) {
        if (data[0] == '\\' && data.size() >= 1) {
          buf.append(1, data[1]);
          data = data.substr(2);
          continue;
        }

        if (!isspace(data[0])) {
          buf.append(1, data[0]);
          data = data.substr(1);
          continue;
        }
        break;
      }
      return save_string(ctx, buf);
    };

    std::string_view tok;
    if (data[0] == '\'' || data[0] == '\"')
      tok = read_quoted();
    else
      tok = read_unquoted();

    if (tok.starts_with('@'))
      append(vec, read_response_file(ctx, tok.substr(1), depth + 1));
    else
      vec.push_back(tok);
  }
  return vec;
}

// Replace "@path/to/some/text/file" with its file contents.
template <typename Context>
std::vector<std::string_view> expand_response_files(Context &ctx, char **argv) {
  std::vector<std::string_view> vec;
  for (i64 i = 0; argv[i]; i++) {
    if (argv[i][0] == '@')
      append(vec, read_response_file(ctx, argv[i] + 1, 1));
    else
      vec.push_back(argv[i]);
  }
  return vec;
}

static inline std::string_view string_trim(std::string_view str) {
  size_t pos = str.find_first_not_of(" \t");
  if (pos == str.npos)
    return "";
  str = str.substr(pos);

  pos = str.find_last_not_of(" \t");
  if (pos == str.npos)
    return str;
  return str.substr(0, pos + 1);
}

} // namespace mold
