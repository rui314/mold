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

static std::vector<std::string_view>
get_string_vector(YamlNode &node, std::string_view key) {
  std::vector<std::string_view> vec;
  for (YamlNode &mem : get_vector(node, key))
    if (auto val = std::get_if<std::string_view>(&mem.data))
      vec.push_back(*val);
  return vec;
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
    if (auto val = std::get_if<std::string_view>(&mem.data))
      if (*val == key)
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
        if (auto val = get_string(mem, "value"))
          tbd.uuid = *val;

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
      append(tbd.reexported_libs, get_string_vector(mem, "libraries"));

  for (std::string_view key : {"exports", "reexports"}) {
    for (YamlNode &mem : get_vector(node, key)) {
      if (contains(get_vector(mem, "targets"), arch)) {
        append(tbd.exports, get_string_vector(mem, "symbols"));
        append(tbd.weak_exports, get_string_vector(mem, "weak-symbols"));
        append(tbd.objc_classes, get_string_vector(mem, "objc-classes"));
        append(tbd.objc_eh_types, get_string_vector(mem, "objc-eh-types"));
        append(tbd.objc_ivars, get_string_vector(mem, "objc-ivars"));
      }
    }
  }

  return tbd;
}

template <typename E>
static TextDylib squash(Context<E> &ctx, std::span<TextDylib> tbds) {
  std::unordered_map<std::string_view, TextDylib> map;

  TextDylib main = std::move(tbds[0]);
  for (TextDylib &tbd : tbds.subspan(1))
    map[tbd.install_name] = std::move(tbd);

  std::vector<std::string_view> external_libs;

  std::function<void(std::span<std::string_view>)> handle_reexported_libs =
    [&](std::span<std::string_view> libs) {
    for (std::string_view lib : libs) {
      auto it = map.find(lib);
      if (it != map.end()) {
        TextDylib &child = it->second;
        append(main.exports, child.exports);
        append(main.weak_exports, child.weak_exports);
        append(main.objc_classes, child.objc_classes);
        append(main.objc_eh_types, child.objc_eh_types);
        append(main.objc_ivars, child.objc_ivars);

        handle_reexported_libs(child.reexported_libs);
      } else {
        external_libs.push_back(lib);
      }
    }
  };

  handle_reexported_libs(main.reexported_libs);
  main.reexported_libs = std::move(external_libs);
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
