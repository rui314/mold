// On macOS, you can pass a test file describing a dylib instead of an
// actual dylib file to link against a dynamic library. Such text file
// should be in the YAML format and contains dylib's exported symbols
// as well as the file's various attributes. The extension of the text
// file is `.tbd`.
//
// .tbd files allows users to link against a library without
// distributing the binary of the library file itself.
//
// This file contains functions to parse the .tbd file.

#include "mold.h"

#include <optional>

namespace mold::macho {

static std::vector<YamlNode>
get_vector(YamlNode &node, std::string_view key) {
  if (auto *map = std::get_if<std::map<std::string_view, YamlNode>>(&node.data))
    if (auto it = map->find(key); it != map->end())
      if (auto *vec = std::get_if<std::vector<YamlNode>>(&it->second.data))
        return *vec;
  return {};
}

static std::optional<std::string_view>
get_string(YamlNode &node, std::string_view key) {
  if (auto *map = std::get_if<std::map<std::string_view, YamlNode>>(&node.data))
    if (auto it = map->find(key); it != map->end())
      if (auto *str = std::get_if<std::string_view>(&it->second.data))
        return *str;
  return {};
}

static bool contains(const std::vector<YamlNode> &vec, std::string_view key) {
  for (const YamlNode &mem : vec)
    if (const std::string_view *s = std::get_if<std::string_view>(&mem.data))
      if (*s == key)
        return true;
  return false;
}

static std::optional<TextDylib> to_tbd(YamlNode &node, std::string_view arch) {
  if (!contains(get_vector(node, "targets"), arch))
    return {};

  TextDylib tbd;

  for (YamlNode &mem : get_vector(node, "uuids"))
    if (auto target = get_string(mem, "target"))
      if (*target == arch)
        if (auto value = get_string(mem, "value"))
          tbd.uuid = *value;

  if (auto val = get_string(node, "install-name"))
    tbd.install_name = *val;

  if (auto val = get_string(node, "current-version"))
    tbd.current_version = *val;

  for (YamlNode &mem : get_vector(node, "parent-umbrella"))
    if (contains(get_vector(mem, "targets"), arch))
      if (auto val = get_string(mem, "umbrella"))
        tbd.parent_umbrella = *val;

  for (YamlNode &mem : get_vector(node, "reexported-libraries"))
    if (contains(get_vector(mem, "targets"), arch))
      for (YamlNode &mem : get_vector(mem, "libraries"))
        if (auto *lib = std::get_if<std::string_view>(&mem.data))
          tbd.reexported_libs.push_back(*lib);

  for (std::string_view key : {"exports", "reexports"})
    for (YamlNode &mem : get_vector(node, key))
      if (contains(get_vector(mem, "targets"), arch)) {
        for (YamlNode &mem : get_vector(mem, "symbols"))
          if (auto *sym = std::get_if<std::string_view>(&mem.data))
            tbd.exports.push_back(*sym);
        for (YamlNode &mem : get_vector(mem, "weak-symbols"))
          if (auto *sym = std::get_if<std::string_view>(&mem.data))
            tbd.weak_exports.push_back(*sym);
        for (YamlNode &mem : get_vector(mem, "objc-classes"))
          if (auto *clazz = std::get_if<std::string_view>(&mem.data))
            tbd.objc_classes.push_back(*clazz);
        for (YamlNode &mem : get_vector(mem, "objc-eh-types"))
          if (auto *eh_type = std::get_if<std::string_view>(&mem.data))
            tbd.objc_eh_types.push_back(*eh_type);
        for (YamlNode &mem : get_vector(mem, "objc-ivars"))
          if (auto *ivar = std::get_if<std::string_view>(&mem.data))
            tbd.objc_ivars.push_back(*ivar);
      }

  return tbd;
}

template <typename E>
static TextDylib squash(Context<E> &ctx, std::span<TextDylib> tbds) {
  std::unordered_map<std::string_view, TextDylib> map;

  TextDylib main = std::move(tbds[0]);
  for (TextDylib &tbd : tbds.subspan(1))
    map[tbd.install_name] = std::move(tbd);

  std::vector<std::string_view> libs;

  for (std::string_view lib : main.reexported_libs) {
    auto it = map.find(lib);
    if (it != map.end())
      append(main.exports, it->second.exports);
    else
      libs.push_back(lib);
  }

  main.reexported_libs = std::move(libs);
  return main;
}

template <typename E>
static TextDylib parse(Context<E> &ctx, MappedFile<Context<E>> *mf,
                       std::string_view arch) {
  std::string_view contents = mf->get_contents();
  std::variant<std::vector<YamlNode>, YamlError> res = parse_yaml(contents);

  if (YamlError *err = std::get_if<YamlError>(&res)) {
    i64 lineno = std::count(contents.begin(), contents.begin() + err->pos, '\n');
    Fatal(ctx) << mf->name << ":" << (lineno + 1)
               << ": YAML parse error: " << err->msg;
  }

  std::vector<YamlNode> &nodes = std::get<std::vector<YamlNode>>(res);
  if (nodes.empty())
    Fatal(ctx) << mf->name << ": malformed TBD file";

  std::vector<TextDylib> vec;
  for (YamlNode &node : nodes)
    if (std::optional<TextDylib> dylib = to_tbd(node, arch))
      vec.push_back(*dylib);
  return squash(ctx, vec);
}

template <>
TextDylib parse_tbd(Context<ARM64> &ctx, MappedFile<Context<ARM64>> *mf) {
  return parse(ctx, mf, "arm64-macos");
}

template <>
TextDylib parse_tbd(Context<X86_64> &ctx, MappedFile<Context<X86_64>> *mf) {
  return parse(ctx, mf, "x86_64-macos");
}

} // namespace mold::macho
