#include "mold.h"

namespace mold::macho {

void dump_yaml(Context &ctx, YamlNode &node, i64 depth = 0) {
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
    std::get_if<std::vector<std::pair<std::string_view, YamlNode>>>(&node.data);
  assert(elem);

  SyncOut(ctx) << std::string(depth * 2, ' ') << "map:";
  for (std::pair<std::string_view, YamlNode> &kv : *elem) {
    SyncOut(ctx) << std::string(depth * 2 + 2, ' ') << "key: " << kv.first;
    dump_yaml(ctx, kv.second, depth + 1);
  }
}

std::string_view get_line(std::string_view str, i64 pos) {
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

std::vector<TextBasedDylib> parse_tbd(Context &ctx, MappedFile<Context> *mf) {
  std::string_view contents = mf->get_contents();
  std::variant<std::vector<YamlNode>, YamlError> res = parse_yaml(contents);

  if (YamlError *err = std::get_if<YamlError>(&res)) {
    std::string_view line = get_line(contents, err->pos);
    i64 lineno = std::count(contents.begin(), contents.begin() + err->pos, '\n');
    Fatal(ctx) << mf->name << ":" << (lineno + 1)
               << ": YAML parse error: " << err->msg;
  }

  for (YamlNode &node : std::get<std::vector<YamlNode>>(res)) {
    SyncOut(ctx) << "---";
    dump_yaml(ctx, node);
  }

  return {};
}

} // namespace mold::macho
