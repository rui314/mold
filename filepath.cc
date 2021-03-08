#include "mold.h"

std::string path_dirname(std::string_view path) {
  i64 pos = path.find_last_of('/');
  if (pos == path.npos)
    return ".";
  return std::string(path.substr(0, pos));
}

std::string path_basename(std::string_view path) {
  if (path.empty())
    return ".";

  while (path.ends_with('/'))
    path = path.substr(0, path.size() - 2);

  if (path.empty())
    return "/";

  i64 pos = path.find_last_of('/');
  if (pos == path.npos)
    return std::string(path);
  return std::string(path.substr(pos + 1));
}
