#include "mold.h"

#include <optional>

namespace mold::macho {

static std::string_view get_line(std::string_view str, i64 pos) {
  i64 begin = str.substr(0, pos).rfind('\n');
  if (begin == str.npos)
    begin = 0;
  else
    begin++;

  i64 end = str.substr(pos).find('\n');
  if (end == str.npos)
    end = str.size();

  return str.substr(begin, end - begin);
}

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

static std::optional<TextDylib> to_tbd(YamlNode &node) {
  if (!contains(get_vector(node, "targets"), "x86_64-macos"))
    return {};

  TextDylib tbd;

  for (YamlNode &mem : get_vector(node, "uuids"))
    if (auto target = get_string(mem, "target"))
      if (*target == "x86_64-macos")
        if (auto value = get_string(mem, "value"))
          tbd.uuid = *value;

  if (auto val = get_string(node, "install-name"))
    tbd.install_name = *val;

  if (auto val = get_string(node, "current-version"))
    tbd.current_version = *val;

  for (YamlNode &mem : get_vector(node, "parent-umbrella"))
    if (contains(get_vector(mem, "targets"), "x86_64-macos"))
      if (auto val = get_string(mem, "umbrella"))
        tbd.parent_umbrella = *val;

  for (YamlNode &mem : get_vector(node, "reexported-libraries"))
    if (contains(get_vector(mem, "targets"), "x86_64-macos"))
      for (YamlNode &mem : get_vector(mem, "libraries"))
        if (auto *lib = std::get_if<std::string_view>(&mem.data))
          tbd.reexported_libs.push_back(*lib);

  for (std::string_view key : {"exports", "reexports"})
    for (YamlNode &mem : get_vector(node, key))
      if (contains(get_vector(mem, "targets"), "x86_64-macos"))
        for (YamlNode &mem : get_vector(mem, "symbols"))
          if (auto *sym = std::get_if<std::string_view>(&mem.data))
            tbd.exports.push_back(*sym);

  return tbd;
}

static TextDylib squash(Context &ctx, std::span<TextDylib> tbds) {
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

TextDylib parse_tbd(Context &ctx, MappedFile<Context> *mf) {
  std::string_view contents = mf->get_contents();
  std::variant<std::vector<YamlNode>, YamlError> res = parse_yaml(contents);

  if (YamlError *err = std::get_if<YamlError>(&res)) {
    std::string_view line = get_line(contents, err->pos);
    i64 lineno = std::count(contents.begin(), contents.begin() + err->pos, '\n');
    Fatal(ctx) << mf->name << ":" << (lineno + 1)
               << ": YAML parse error: " << err->msg;
  }

  std::vector<YamlNode> &nodes = std::get<std::vector<YamlNode>>(res);
  if (nodes.empty())
    Fatal(ctx) << mf->name << ": malformed TBD file";

  std::vector<TextDylib> vec;
  for (YamlNode &node : nodes)
    if (std::optional<TextDylib> dylib = to_tbd(node))
      vec.push_back(*dylib);
  return squash(ctx, vec);
}

} // namespace mold::macho
