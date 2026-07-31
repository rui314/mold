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

bool AhoCorasick::can_handle(std::string_view pat) {
  if (pat.starts_with('*'))
    pat.remove_prefix(1);
  if (pat.ends_with('*'))
    pat.remove_suffix(1);
  return !pat.empty() && pat.find_first_of("*?[\\") == pat.npos;
}

i32 AhoCorasick::find_child(i32 node, u8 ch) const {
  if (node == 0)
    return root_children[ch];

  for (i32 i = nodes[node].first_child; i != -1; i = nodes[i].next_sibling)
    if (nodes[i].ch == ch)
      return i;
  return -1;
}

i32 AhoCorasick::add_child(i32 node, u8 ch) {
  if (i32 child = find_child(node, ch); child != -1)
    return child;

  i32 child = nodes.size();
  i32 sibling = nodes[node].first_child;
  nodes.emplace_back();
  nodes[child].next_sibling = sibling;
  nodes[child].ch = ch;
  nodes[node].first_child = child;

  if (node == 0)
    root_children[ch] = child;
  return child;
}

i64 AhoCorasick::find(std::string_view str) {
  if (nodes.empty())
    return -1;

  i32 idx = 0;
  i64 val = -1;

  auto walk = [&](u8 c) {
    for (i32 j = idx; j != -1; j = nodes[j].suffix_link) {
      if (i32 child = find_child(j, c); child != -1) {
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
  if (nodes.empty()) {
    root_children.fill(-1);
    nodes.emplace_back();
  }
  i32 idx = 0;

  auto walk = [&](u8 c) { idx = add_child(idx, c); };

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

  // A failure link may refer to any node at the previous depth, so failure
  // links must be constructed breadth-first.
  std::queue<i32> queue;
  for (i32 child = nodes[0].first_child; child != -1;
       child = nodes[child].next_sibling) {
    nodes[child].suffix_link = 0;
    queue.push(child);
  }

  while (!queue.empty()) {
    i32 idx = queue.front();
    queue.pop();

    for (i32 child = nodes[idx].first_child; child != -1;
         child = nodes[child].next_sibling) {
      i32 suffix = nodes[idx].suffix_link;
      while (suffix != 0 && find_child(suffix, nodes[child].ch) == -1)
        suffix = nodes[suffix].suffix_link;

      if (i32 next = find_child(suffix, nodes[child].ch); next != -1)
        suffix = next;

      nodes[child].suffix_link = suffix;
      nodes[child].value = std::max(nodes[child].value, nodes[suffix].value);
      queue.push(child);
    }
  }
}

} // namespace mold
