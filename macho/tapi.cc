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
#include <regex>
#include <unordered_set>

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

template <typename E>
static std::optional<TextDylib>
to_tbd(Context<E> &ctx, YamlNode &node, std::string_view arch) {
  if (!contains(get_vector(node, "targets"), arch))
    return {};

  TextDylib tbd;

  if (auto val = get_string(node, "install-name"))
    tbd.install_name = *val;

  for (YamlNode &mem : get_vector(node, "reexported-libraries"))
    if (contains(get_vector(mem, "targets"), arch))
      append(tbd.reexported_libs, get_string_vector(mem, "libraries"));

  auto concat = [&](const std::string &x, std::string_view y) {
    return save_string(ctx, x + std::string(y));
  };

  for (std::string_view key : {"exports", "reexports"}) {
    for (YamlNode &mem : get_vector(node, key)) {
      if (contains(get_vector(mem, "targets"), arch)) {
        append(tbd.exports, get_string_vector(mem, "symbols"));
        append(tbd.weak_exports, get_string_vector(mem, "weak-symbols"));

        for (std::string_view s : get_string_vector(mem, "objc-classes")) {
          tbd.exports.push_back(concat("_OBJC_CLASS_$_", s));
          tbd.exports.push_back(concat("_OBJC_METACLASS_$_", s));
        }

        for (std::string_view s : get_string_vector(mem, "objc-eh-types"))
          tbd.exports.push_back(concat("_OBJC_EHTYPE_$_", s));

        for (std::string_view s : get_string_vector(mem, "objc-ivars"))
          tbd.exports.push_back(concat("_OBJC_IVAR_$_", s));
      }
    }
  }

  return tbd;
}

static i64 parse_version(const std::string &arg) {
  auto flags = std::regex_constants::ECMAScript | std::regex_constants::optimize;
  static std::regex re(R"((\d+)(?:\.(\d+))?(?:\.(\d+))?)", flags);

  std::smatch m;
  bool ok = std::regex_match(arg, m, re);
  assert(ok);

  i64 major = (m[1].length() == 0) ? 0 : stoi(m[1]);
  i64 minor = (m[2].length() == 0) ? 0 : stoi(m[2]);
  i64 patch = (m[3].length() == 0) ? 0 : stoi(m[3]);
  return (major << 16) | (minor << 8) | patch;
}

// Dylib can contain special symbols whose name starts with "$ld$".
// Such symbols aren't actually symbols but linker directives.
// We interpret such symbols in this function.
template <typename E>
static void interpret_ld_symbols(Context<E> &ctx, TextDylib &tbd) {
  std::vector<std::string_view> syms;
  syms.reserve(tbd.exports.size());

  std::unordered_set<std::string_view> hidden_syms;

  auto string_view = [](const std::csub_match &sub) {
    return std::string_view{sub.first, (size_t)sub.length()};
  };

  for (std::string_view s : tbd.exports) {
    if (!s.starts_with("$ld$"))
      continue;

    auto flags = std::regex_constants::ECMAScript | std::regex_constants::optimize;
    std::cmatch m;

    // $ld$previous$ symbol replaces the default install name with a
    // specified one if the platform OS version is in a specified range.
    static std::regex previous_re(
      R"(\$ld\$previous\$([^$]+)\$([\d.]*)\$(\d+)\$([\d.]+)\$([\d.]+)\$(.*)\$)",
      flags);

    if (std::regex_match(s.begin(), s.end(), m, previous_re)) {
      std::string_view install_name = string_view(m[1]);
      i64 platform = std::stoi(m[3].str());
      i64 min_version = parse_version(m[4]);
      i64 max_version = parse_version(m[5]);
      std::string_view symbol_name = string_view(m[6]);

      if (!symbol_name.empty()) {
        // ld64 source seems to have implemented a feature to give an
        // alternative install name for a matching symbol, but it didn't
        // work in practice (or I may be using the feature in a wrong way.)
        // Ignore such symbol for now.
        continue;
      }

      if (platform == ctx.arg.platform &&
          min_version <= ctx.arg.platform_min_version &&
          ctx.arg.platform_min_version < max_version) {
        tbd.install_name = install_name;
      }
      continue;
    }

    // $ld$add$os_version$symbol adds a symbol if the given OS version
    // matches.
    static std::regex add_re(R"(\$ld\$add\$os([\d.]+)\$(.+))", flags);

    if (std::regex_match(s.begin(), s.end(), m, add_re)) {
      if (ctx.arg.platform_min_version == parse_version(m[1]))
        syms.push_back(string_view(m[2]));
      continue;
    }

    // $ld$hide$os_version$symbol hides a symbol if the given OS version
    // matches.
    static std::regex hidden_re(R"(\$ld\$hide\$os([\d.]+)\$(.+))", flags);

    if (std::regex_match(s.begin(), s.end(), m, hidden_re)) {
      if (ctx.arg.platform_min_version == parse_version(m[1]))
        hidden_syms.insert(string_view(m[2]));
      continue;
    }

    // $ld$install_name$os_version$name changes the install name to a
    // given name.
    static std::regex
      install_name_re(R"(\$ld\$install_name\$os([\d.]+)\$(.+))", flags);

    if (std::regex_match(s.begin(), s.end(), m, install_name_re)) {
      if (ctx.arg.platform_min_version == parse_version(m[1]))
        tbd.install_name = string_view(m[2]);
      continue;
    }
  }

  for (std::string_view s : tbd.exports)
    if (!s.starts_with("$ld$") && !hidden_syms.contains(std::string(s)))
      syms.push_back(s);

  std::erase_if(syms, [](std::string_view s) { return s.starts_with("$ld$"); });
  tbd.exports = syms;
}

template <typename E>
static TextDylib parse(Context<E> &ctx, MappedFile<Context<E>> *mf,
                       std::string_view arch);

template <typename E>
static MappedFile<Context<E>> *
find_external_lib(Context<E> &ctx, std::string_view parent, std::string path) {
  if (!path.starts_with('/'))
    Fatal(ctx) << parent << ": contains an invalid reexported path: " << path;

  for (const std::string &root : ctx.arg.syslibroot) {
    if (path.ends_with(".tbd")) {
      if (auto *file = MappedFile<Context<E>>::open(ctx, root + path))
        return file;
      continue;
    }

    if (path.ends_with(".dylib")) {
      std::string stem(path.substr(0, path.size() - 6));
      if (auto *file = MappedFile<Context<E>>::open(ctx, root + stem + ".tbd"))
        return file;
      if (auto *file = MappedFile<Context<E>>::open(ctx, root + path))
        return file;
    }

    for (std::string extn : {".tbd", ".dylib"})
      if (auto *file = MappedFile<Context<E>>::open(ctx, root + path + extn))
        return file;
  }

  Fatal(ctx) << parent << ": cannot open reexported library " << path;
}

// A single YAML file may contain multiple text dylibs. The first text
// dylib is the main file followed by optional other text dylibs for
// re-exported libraries.
//
// This fucntion squashes multiple text dylibs into a single text dylib
// by copying symbols of re-exported text dylibs to the main text dylib.
template <typename E>
static TextDylib
squash(Context<E> &ctx, std::span<TextDylib> tbds, std::string_view arch) {
  std::unordered_map<std::string_view, TextDylib> map;

  TextDylib main = std::move(tbds[0]);
  for (TextDylib &tbd : tbds.subspan(1))
    map[tbd.install_name] = std::move(tbd);

  std::function<void(TextDylib &)> visit = [&](TextDylib &tbd) {
    for (std::string_view lib : tbd.reexported_libs) {
      auto it = map.find(lib);

      if (it != map.end()) {
        // The referenced reexported library is in the same .tbd file.
        TextDylib &child = it->second;
        append(main.exports, child.exports);
        append(main.weak_exports, child.weak_exports);
        visit(child);
      } else {
        // The referenced reexported library is a separate file.
        MappedFile<Context<E>> *mf =
          find_external_lib(ctx, tbd.install_name, std::string(lib));
        TextDylib child = parse(ctx, mf, arch);
        append(main.exports, child.exports);
        append(main.weak_exports, child.weak_exports);
      }
    }
  };

  visit(main);

  sort(main.exports);
  sort(main.weak_exports);
  remove_duplicates(main.exports);
  remove_duplicates(main.weak_exports);
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
    if (std::optional<TextDylib> dylib = to_tbd(ctx, node, arch))
      vec.push_back(*dylib);

  for (TextDylib &tbd : vec)
    interpret_ld_symbols(ctx, tbd);

  return squash(ctx, vec, arch);
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
