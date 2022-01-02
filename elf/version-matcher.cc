// This file implements the Aho-Corasick algorithm to match version
// script patterns to symbol strings as quickly as possible.
//
// Here are some examples of version script patterns:
//
//    qt_private_api_tag*
//    *16QAccessibleCache*
//    *32QAbstractFileIconProviderPrivate*
//    *17QPixmapIconEngine*
//
// The pattern is a glob pattern, so `*` is a wildcard that matches
// any substring. We sometimes have hundreds of version script
// patterns and have to match them against millions of symbol strings.
//
// Aho-Corasick cannot handle complex patterns such as `*foo*bar*`.
// We convert such patterns into regexes. Regexes are slow, but
// complex patterns are rare in practice, so it should be OK.

#include "mold.h"

#include <queue>

namespace mold::elf {

std::optional<u16> VersionMatcher::find(std::string_view str) {
  u32 idx = UINT32_MAX;

  if (root) {
    std::call_once(once_flag, [&]() { compile(); });

    // Match against simple glob patterns
    TrieNode *node = root.get();

    auto walk = [&](u8 c) {
      for (;;) {
        if (node->children[c]) {
          node = node->children[c].get();
          idx = std::min(idx, node->value);
          return;
        }

        if (!node->suffix_link)
          return;
        node = node->suffix_link;
        idx = std::min(idx, node->value);
      }
    };

    walk('\0');
    for (u8 c : str)
      walk(c);
    walk('\0');
  }

  // Match against complex glob patterns
  for (std::pair<std::regex, u32> &re : regexes)
    if (std::regex_match(str.begin(), str.end(), re.first))
      idx = std::min(idx, re.second);

  if (idx == UINT32_MAX)
    return {};
  return versions[idx];
}

static bool is_simple_pattern(std::string_view pat) {
  static std::regex re(R"(\*?[^*[?]+\*?)", std::regex_constants::optimize);
  return std::regex_match(pat.begin(), pat.end(), re);
}

static std::string handle_stars(std::string_view pat) {
  std::string str(pat);

  if (str.starts_with('*') && str.ends_with('*'))
    return str.substr(1, str.size() - 2);
  if (str.starts_with('*'))
    return str.substr(1) + "\0";
  if (str.ends_with('*'))
    return "\0" + str.substr(0, str.size() - 1);
  return "\0" + str + "\0";
}

bool VersionMatcher::add(std::string_view pat, u16 ver) {
  assert(!compiled);
  assert(!pat.empty());

  u32 idx = strings.size();
  strings.push_back(std::string(pat));
  versions.push_back(ver);

  // Complex glob pattern
  if (!is_simple_pattern(pat)) {
    if (std::optional<std::regex> re = glob_to_regex(pat)) {
      regexes.push_back({*re, idx});
      return true;
    }
    return false;
  }

  // Simple glob pattern
  if (!root)
    root.reset(new TrieNode);
  TrieNode *node = root.get();

  for (u8 c : handle_stars(pat)) {
    if (!node->children[c])
      node->children[c].reset(new TrieNode);
    node = node->children[c].get();
  }

  node->value = std::min(node->value, idx);
  return true;
}

void VersionMatcher::compile() {
  fix_suffix_links(*root);
  fix_value();
  compiled = true;
}

void VersionMatcher::fix_suffix_links(TrieNode &node) {
  for (i64 i = 0; i < 256; i++) {
    if (!node.children[i])
      continue;

    TrieNode &child = *node.children[i];

    TrieNode *cur = node.suffix_link;
    for (;;) {
      if (!cur) {
        child.suffix_link = root.get();
        break;
      }

      if (cur->children[i]) {
        child.suffix_link = cur->children[i].get();
        break;
      }

      cur = cur->suffix_link;
    }

    fix_suffix_links(child);
  }
}

void VersionMatcher::fix_value() {
  std::queue<TrieNode *> queue;
  queue.push(root.get());

  do {
    TrieNode *node = queue.front();
    queue.pop();

    for (std::unique_ptr<TrieNode> &child : node->children) {
      if (!child)
        continue;
      child->value = std::min(child->value, child->suffix_link->value);
      queue.push(child.get());
    }
  } while (!queue.empty());
}

} // namespace mold::elf
