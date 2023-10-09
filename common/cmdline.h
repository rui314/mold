#pragma once

#include "common.h"

namespace mold {

template <typename Context>
std::vector<std::string_view>
read_response_file(Context &ctx, std::string_view path, i64 depth) {
  if (depth > 10)
    Fatal(ctx) << path << ": response file nesting too deep";

  std::vector<std::string_view> vec;
  MappedFile<Context> *mf = MappedFile<Context>::must_open(ctx, std::string(path));
  u8 *data = mf->data;

  auto read_quoted = [&](i64 i, char quote) {
    std::string buf;
    while (i < mf->size && data[i] != quote) {
      if (data[i] == '\\') {
        buf.append(1, data[i + 1]);
        i += 2;
      } else {
        buf.append(1, data[i++]);
      }
    }
    if (i >= mf->size)
      Fatal(ctx) << path << ": premature end of input";
    vec.push_back(save_string(ctx, buf));
    return i + 1;
  };

  auto read_unquoted = [&](i64 i) {
    std::string buf;

    while (i < mf->size) {
      if (data[i] == '\\' && i + 1 < mf->size) {
        buf.append(1, data[i + 1]);
        i += 2;
        continue;
      }

      if (!isspace(data[i])) {
        buf.append(1, data[i++]);
        continue;
      }

      break;
    }

    vec.push_back(save_string(ctx, buf));
    return i;
  };

  for (i64 i = 0; i < mf->size;) {
    if (isspace(data[i])) {
      i++;
      continue;
    }

    if (data[i] == '\'')
      i = read_quoted(i + 1, '\'');
    else if (data[i] == '\"')
      i = read_quoted(i + 1, '\"');
    else
      i = read_unquoted(i);

    if (vec.back().starts_with('@')) {
      std::string_view path = vec.back().substr(1);
      vec.pop_back();
      append(vec, read_response_file(ctx, path, depth + 1));
    }
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
