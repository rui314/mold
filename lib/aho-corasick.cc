// This file implements the Aho-Corasick algorithm to search multiple
// strings within an input string simultaneously. It is essentially a
// trie with additional links. For details, see
// https://en.wikipedia.org/wiki/Aho-Corasick_algorithm.
//
// We use it for simple glob patterns in version scripts or dynamic
// list files. Here are some examples of glob patterns:
//
//    qt_private_api_tag*
//    *16QAccessibleCache*
//    *32QAbstractFileIconProviderPrivate*
//    *17QPixmapIconEngine*
//
// Aho-Corasick can do only substring search, so it cannot handle
// complex glob patterns such as `*foo*bar*`. We handle such patterns
// with the Glob class.

#include "lib.h"

#include <queue>

namespace mold {

bool AhoCorasick::can_handle(std::string_view str) {
  if (str.starts_with('*'))
    str.remove_prefix(1);
  if (str.ends_with('*'))
    str.remove_suffix(1);
  return str.find_first_of("*?[") == str.npos;
}

i64 AhoCorasick::find(std::string_view str) {
  if (nodes.empty())
    return -1;

  i64 idx = 0;
  i64 val = -1;

  auto walk = [&](u8 c) {
    for (i64 j = idx; j != -1; j = nodes[j].suffix_link) {
      i64 child = nodes[j].children[c];
      if (child != -1) {
        idx = child;
        val = std::max(val, nodes[child].value);
        return;
      }
    }
    idx = 0;
  };

  walk('\0');
  for (u8 c : str)
    walk(c);
  walk('\0');
  return val;
}

bool AhoCorasick::add(std::string_view pat, i64 val) {
  assert(can_handle(pat));

  strings.emplace_back(pat);
  if (nodes.empty())
    nodes.resize(1);
  i64 idx = 0;

  auto walk = [&](u8 c) {
    if (nodes[idx].children[c] == -1) {
      nodes[idx].children[c] = nodes.size();
      nodes.resize(nodes.size() + 1);
    }
    idx = nodes[idx].children[c];
  };

  // We handle "foo" as if "\0foo\0", "*foo" as if "foo\0", "foo*" as
  // if "\0foo", and "*foo*" as if "foo". Aho-Corasick can do only
  // substring matching, so we use \0 as a beginning/end-of-string
  // markers.
  if (!pat.starts_with('*'))
    walk('\0');
  for (u8 c : pat)
    if (c != '*')
      walk(c);
  if (!pat.ends_with('*'))
    walk('\0');

  nodes[idx].value = std::max(nodes[idx].value, val);
  return true;
}

void AhoCorasick::compile() {
  if (nodes.empty())
    return;
  fix_suffix_links(0);
  fix_values();
}

void AhoCorasick::fix_suffix_links(i64 idx) {
  for (i64 i = 0; i < 256; i++) {
    i64 child = nodes[idx].children[i];
    if (child == -1)
      continue;

    i64 j = nodes[idx].suffix_link;
    for (; j != -1; j = nodes[j].suffix_link) {
      if (nodes[j].children[i] != -1) {
        nodes[child].suffix_link = j;
        break;
      }
    }
    if (j == -1)
      nodes[child].suffix_link = 0;
    fix_suffix_links(child);
  }
}

void AhoCorasick::fix_values() {
  std::queue<i64> queue;
  queue.push(0);

  do {
    i64 idx = queue.front();
    queue.pop();

    for (i64 child : nodes[idx].children) {
      if (child != -1) {
        i64 suffix = nodes[child].suffix_link;
        nodes[child].value = std::max(nodes[child].value, nodes[suffix].value);
        queue.push(child);
      }
    }
  } while (!queue.empty());
}

} // namespace mold
