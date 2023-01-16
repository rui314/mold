#include "common.h"

#include <filesystem>
#include <sys/stat.h>

namespace mold {

std::string get_realpath(std::string_view path) {
  std::error_code ec;
  std::string ret = std::filesystem::canonical(path, ec).string();
  return ec ? std::string(path) : ret;
}

// Removes redundant '/..' or '/.' from a given path.
// The transformation is done purely by lexical processing.
// This function does not access file system.
std::string path_clean(std::string_view path) {
  return filepath(path).lexically_normal().string();
}

std::filesystem::path to_abs_path(std::filesystem::path path) {
  if (path.is_absolute())
    return path.lexically_normal();
  return (std::filesystem::current_path() / path).lexically_normal();
}

} // namespace mold
