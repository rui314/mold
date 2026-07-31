// This file implements the glob matcher used for symbol name patterns.
// Exact, prefix and suffix patterns are matched directly, and simple
// substring patterns are combined into an Aho-Corasick matcher. The
// remaining patterns are matched with the non-recursive algorithm described
// at https://research.swtch.com/glob. If there are many such patterns, a
// bit-parallel NFA matches them together.

#include "lib.h"

namespace mold {

std::optional<Glob::Pattern> Glob::Pattern::compile(std::string_view pat,
                                                    i64 value) {
  std::vector<Token> tokens;

  while (!pat.empty()) {
    u8 c = pat[0];
    pat.remove_prefix(1);

    switch (c) {
    case '[': {
      // Here are a few bracket pattern examples:
      //
      // [abc]: a, b or c
      // [$\]!]: $, ] or !
      // [a-czg-i]: a, b, c, z, g, h, or i
      // [!a-z]: Any character except lowercase letters
      //
      // Both `!` and `^` are accepted as negation markers. `!` is the
      // POSIX/shell convention used by other linkers. `^` was mold's
      // original syntax and is kept for backward compatibility.
      tokens.emplace_back(BRACKET);
      std::bitset<256> &chars = tokens.back().chars;
      bool negate = false;
      bool closed = false;

      if (!pat.empty() && (pat[0] == '!' || pat[0] == '^')) {
        negate = true;
        pat.remove_prefix(1);
      }

      while (!pat.empty()) {
        if (pat[0] == ']') {
          pat.remove_prefix(1);
          closed = true;
          break;
        }

        if (pat[0] == '\\') {
          pat.remove_prefix(1);
          if (pat.empty())
            return {};
        }

        if (pat.size() >= 3 && pat[1] == '-') {
          u8 start = pat[0];
          u8 end = pat[2];
          pat.remove_prefix(3);

          if (end == '\\') {
            if (pat.empty())
              return {};
            end = pat[0];
            pat.remove_prefix(1);
          }

          if (end < start)
            return {};

          for (i64 i = start; i <= end; i++)
            chars[i] = true;
        } else {
          chars[(u8)pat[0]] = true;
          pat.remove_prefix(1);
        }
      }

      if (!closed)
        return {};

      if (negate)
        chars.flip();
      break;
    }
    case '?':
      tokens.emplace_back(QUESTION);
      break;
    case '*':
      if (tokens.empty() || tokens.back().kind != STAR)
        tokens.emplace_back(STAR);
      break;
    case '\\':
      if (pat.empty())
        return {};
      if (tokens.empty() || tokens.back().kind != STRING)
        tokens.emplace_back(STRING);
      tokens.back().str += pat[0];
      pat.remove_prefix(1);
      break;
    default:
      if (tokens.empty() || tokens.back().kind != STRING)
        tokens.emplace_back(STRING);
      tokens.back().str += c;
      break;
    }
  }
  return Pattern(std::move(tokens), value);
}

bool Glob::Pattern::match(std::string_view str) const {
  if (!tokens.empty() && tokens.back().kind == STRING &&
      !str.ends_with(tokens.back().str))
    return false;

  i64 x = 0;
  i64 y = 0;
  i64 next_x = -1;
  i64 next_y = -1;

  while (x < str.size() || y < tokens.size()) {
    if (y < tokens.size()) {
      const Token &tok = tokens[y];

      switch (tok.kind) {
      case STRING:
        if (str.substr(x).starts_with(tok.str)) {
          x += tok.str.size();
          y++;
          continue;
        }
        break;
      case STAR:
        next_y = y++;
        next_x = x + 1;

        if (y < tokens.size() && tokens[y].kind == STRING) {
          const std::string &s = tokens[y].str;
          i64 pos = str.find(s, x);
          if (pos == str.npos)
            return false;
          next_x = pos + 1;
          x = pos + s.size();
          y++;
        }
        continue;
      case QUESTION:
        if (x < str.size()) {
          x++;
          y++;
          continue;
        }
        break;
      case BRACKET:
        if (x < str.size() && tok.chars[(u8)str[x]]) {
          x++;
          y++;
          continue;
        }
        break;
      }
    }

    // Retry the last star after assigning one more input byte to it.
    if (next_x != -1 && next_x <= str.size()) {
      x = next_x;
      y = next_y;
      continue;
    }
    return false;
  }
  return true;
}

void Glob::Nfa::compile(std::span<const Pattern> patterns) {
  i64 num_states = 0;

  for (const Pattern &pattern : patterns) {
    num_states++;

    for (const Pattern::Token &tok : pattern.tokens) {
      if (tok.kind == Pattern::STRING)
        num_states += tok.str.size();
      else if (tok.kind != Pattern::STAR)
        num_states++;
    }
  }

  i64 num_words = (num_states + 63) / 64;
  initial_states.resize(num_words);
  star_states.resize(num_words);
  accept_states.resize(num_words);
  char_masks.resize(256 * num_words);
  values.resize(num_states, -1);

  auto set_bit = [](std::vector<u64> &vec, i64 pos) {
    vec[pos / 64] |= 1ULL << (pos % 64);
  };

  i64 state = 0;

  for (const Pattern &pattern : patterns) {
    set_bit(initial_states, state);

    for (const Pattern::Token &tok : pattern.tokens) {
      switch (tok.kind) {
      case Pattern::STRING:
        for (u8 c : tok.str) {
          state++;
          char_masks[c * num_words + state / 64] |= 1ULL << (state % 64);
        }
        break;
      case Pattern::STAR:
        set_bit(star_states, state);
        break;
      case Pattern::QUESTION:
        state++;
        for (i64 c = 0; c < 256; c++)
          char_masks[c * num_words + state / 64] |= 1ULL << (state % 64);
        break;
      case Pattern::BRACKET:
        state++;
        for (i64 c = 0; c < 256; c++)
          if (tok.chars[c])
            char_masks[c * num_words + state / 64] |= 1ULL << (state % 64);
        break;
      }
    }

    set_bit(accept_states, state);
    values[state] = pattern.value;
    state++;
  }
}

i64 Glob::Nfa::match(std::string_view str) const {
  static thread_local std::vector<u64> states;
  states.assign(initial_states.begin(), initial_states.end());

  i64 num_words = states.size();

  for (u8 c : str) {
    const u64 *mask = char_masks.data() + c * num_words;
    u64 carry = 0;

    for (i64 i = 0; i < num_words; i++) {
      u64 old = states[i];
      u64 next = (old << 1) | carry;
      states[i] = (old & star_states[i]) | (next & mask[i]);
      carry = old >> 63;
    }
  }

  i64 value = -1;

  for (i64 i = 0; i < num_words; i++) {
    u64 word = states[i] & accept_states[i];

    while (word) {
      i64 bit = std::countr_zero(word);
      value = std::max(value, values[i * 64 + bit]);
      word &= word - 1;
    }
  }
  return value;
}

static bool is_literal(std::string_view pat) {
  return pat.find_first_of("*?[\\") == pat.npos;
}

bool Glob::add(std::string_view pat, i64 val) {
  assert(val >= 0);
  assert(!is_compiled);
  is_empty = false;

  // Match-all, exact, prefix and suffix patterns are handled with
  // plain string comparisons instead of the matchers below, which
  // have to scan the entire input string on every query.
  if (pat == "*") {
    match_all = std::max(match_all, val);
    return true;
  }

  if (is_literal(pat)) {
    exacts.push_back({std::string(pat), val});
    return true;
  }

  if (pat.ends_with('*') && is_literal(pat.substr(0, pat.size() - 1))) {
    prefixes.push_back({std::string(pat.substr(0, pat.size() - 1)), val});
    return true;
  }

  if (pat.starts_with('*') && is_literal(pat.substr(1))) {
    suffixes.push_back({std::string(pat.substr(1)), val});
    return true;
  }

  // If the pattern requires only a single substring search, the
  // Aho-Corasick algorithm is even faster than our glob matcher.
  if (aho_corasick.can_handle(pat))
    return aho_corasick.add(pat, val);

  if (std::optional<Pattern> pattern = Pattern::compile(pat, val)) {
    patterns.push_back(std::move(*pattern));
    return true;
  }
  return false;
}

i64 Glob::find(std::string_view str) {
  std::call_once(once, [&] {
    // If the same name was added more than once, keep only the entry
    // with the largest value, as find() returns the largest match.
    // Sorting by (name, negated value) places that entry first in
    // each run of duplicates, which is the one unique() keeps.
    ranges::sort(exacts, {}, [](const LiteralPattern &p) {
      return std::pair<std::string_view, i64>(p.pat, -p.value);
    });

    auto dup = ranges::unique(exacts, {}, &LiteralPattern::pat);
    exacts.erase(dup.begin(), dup.end());

    if (patterns.size() >= 64) {
      nfa.compile(patterns);
      patterns.clear();
    }

    aho_corasick.compile();
    is_compiled = true;
  });

  i64 val = match_all;

  auto it = ranges::lower_bound(exacts, str, {}, &LiteralPattern::pat);
  if (it != exacts.end() && it->pat == str)
    val = std::max(val, it->value);

  for (const LiteralPattern &p : prefixes)
    if (val < p.value && str.starts_with(p.pat))
      val = p.value;

  for (const LiteralPattern &p : suffixes)
    if (val < p.value && str.ends_with(p.pat))
      val = p.value;

  if (!nfa.empty())
    val = std::max(val, nfa.match(str));

  for (const Pattern &p : patterns)
    if (val < p.value && p.match(str))
      val = p.value;

  return std::max(val, aho_corasick.find(str));
}

} // namespace mold
