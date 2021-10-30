#include "mold.h"

#include <optional>

namespace mold::macho {

typedef std::vector<YamlNode> Vector;

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

template <typename T>
static const T *lookup(const YamlNode &node, std::string_view key) {
  if (auto *map = std::get_if<std::map<std::string_view, YamlNode>>(&node.data))
    if (auto it = map->find(key); it != map->end())
      return std::get_if<T>(&it->second.data);
  return nullptr;
}

static bool contains(const Vector &vec, std::string_view key) {
  for (const YamlNode &mem : vec)
    if (const std::string_view *s = std::get_if<std::string_view>(&mem.data))
      if (*s == key)
        return true;
  return false;
}

static std::optional<TextDylib> to_tbd(const YamlNode &node) {
  const Vector *targets = lookup<Vector>(node, "targets");
  if (!targets || !contains(*targets, "x86_64-macos"))
    return {};

  TextDylib tbd;

  if (auto *vec = lookup<Vector>(node, "uuids"))
    for (const YamlNode &mem : *vec)
      if (auto *target = lookup<std::string_view>(mem, "target"))
        if (*target == "x86_64-macos")
          if (auto *value = lookup<std::string_view>(mem, "value"))
            tbd.uuid = *value;

  if (auto *val = lookup<std::string_view>(node, "install-name"))
    tbd.install_name = *val;

  if (auto *val = lookup<std::string_view>(node, "current-version"))
    tbd.current_version = *val;

  if (auto *vec = lookup<Vector>(node, "parent-umbrella"))
    for (const YamlNode &mem : *vec)
      if (auto *targets = lookup<Vector>(mem, "targets"))
        if (contains(*targets, "x86_64-macos"))
          if (auto *val = lookup<std::string_view>(mem, "umbrella"))
            tbd.parent_umbrella = *val;

  if (auto *vec = lookup<Vector>(node, "reexported-libraries"))
    for (const YamlNode &mem : *vec)
      if (auto *targets = lookup<Vector>(mem, "targets"))
        if (contains(*targets, "x86_64-macos"))
          if (auto *libs = lookup<Vector>(mem, "libraries"))
            for (const YamlNode &mem : *libs)
              if (auto *lib = std::get_if<std::string_view>(&mem.data))
                tbd.reexported_libs.push_back(*lib);

  for (std::string_view key : {"exports", "reexports"})
    if (auto *vec = lookup<Vector>(node, key))
      for (const YamlNode &mem : *vec)
        if (auto *targets = lookup<Vector>(mem, "targets"))
          if (contains(*targets, "x86_64-macos"))
            if (auto *syms = lookup<Vector>(mem, "symbols"))
              for (const YamlNode &mem : *syms)
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
  std::variant<Vector, YamlError> res = parse_yaml(contents);

  if (YamlError *err = std::get_if<YamlError>(&res)) {
    std::string_view line = get_line(contents, err->pos);
    i64 lineno = std::count(contents.begin(), contents.begin() + err->pos, '\n');
    Fatal(ctx) << mf->name << ":" << (lineno + 1)
               << ": YAML parse error: " << err->msg;
  }

  Vector &nodes = std::get<Vector>(res);
  if (nodes.empty())
    Fatal(ctx) << mf->name << ": malformed TBD file";

  std::vector<TextDylib> vec;
  for (YamlNode &node : nodes)
    if (std::optional<TextDylib> dylib = to_tbd(node))
      vec.push_back(*dylib);
  return squash(ctx, vec);
}

} // namespace mold::macho
