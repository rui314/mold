#include "lib.h"

#include <cstring>

namespace mold {

std::optional<Glob> Glob::compile(std::string_view pat) {
  std::vector<Element> vec;

  while (!pat.empty()) {
    u8 c = pat[0];
    pat = pat.substr(1);

    switch (c) {
    case '[': {
      // Here are a few bracket pattern examples:
      //
      // [abc]: a, b or c
      // [$\]!]: $, ] or !
      // [a-czg-i]: a, b, c, z, g, h, or i
      // [^a-z]: Any character except lowercase letters
      vec.emplace_back(BRACKET);
      std::bitset<256> &bitset = vec.back().bitset;

      bool negate = false;
      if (!pat.empty() && pat[0] == '^') {
        negate = true;
        pat = pat.substr(1);
      }

      bool closed = false;

      while (!pat.empty()) {
        if (pat[0] == ']') {
          pat = pat.substr(1);
          closed = true;
          break;
        }

        if (pat[0] == '\\') {
          pat = pat.substr(1);
          if (pat.empty())
            return {};
        }

        if (pat.size() >= 3 && pat[1] == '-') {
          u8 start = pat[0];
          u8 end = pat[2];
          pat = pat.substr(3);

          if (end == '\\') {
            if (pat.empty())
              return {};
            end = pat[0];
            pat = pat.substr(1);
          }

          if (end < start)
            return {};

          for (i64 i = start; i <= end; i++)
            bitset[i] = true;
        } else {
          bitset[(u8)pat[0]] = true;
          pat = pat.substr(1);
        }
      }

      if (!closed)
        return {};

      if (negate)
        bitset.flip();
      break;
    }
    case '?':
      vec.emplace_back(QUESTION);
      break;
    case '*':
      vec.emplace_back(STAR);
      break;
    case '\\':
      if (pat.empty())
        return {};
      if (vec.empty() || vec.back().kind != STRING)
        vec.emplace_back(STRING);
      vec.back().str += pat[0];
      pat = pat.substr(1);
      break;
    default:
      if (vec.empty() || vec.back().kind != STRING)
        vec.emplace_back(STRING);
      vec.back().str += c;
      break;
    }
  }

  return {Glob{std::move(vec)}};
}

bool Glob::match(std::string_view str) {
  return do_match(str, elements);
}

// This glob match runs in nearly linear time. See
// https://research.swtch.com/glob if you are interested.
bool Glob::do_match(std::string_view str, std::span<Element> elem) {
  i64 x = 0;
  i64 y = 0;
  i64 nx = 0;
  i64 ny = 0;

  while (x < str.size() || y < elem.size()) {
    if (y < elem.size()) {
      Element &e = elem[y];

      switch (e.kind) {
      case STRING:
        if (x < str.size() && str.substr(x).starts_with(e.str)) {
          x += e.str.size();
          y++;
          continue;
        }
        break;
      case STAR:
        // Try to match at x. If that doesn't work out, restart at x+1 next.
        nx = x + 1;
        ny = y;
        y++;
        continue;
      case QUESTION:
        if (x < str.size()) {
          x++;
          y++;
          continue;
        }
        break;
      case BRACKET:
        if (x < str.size() && e.bitset[str[x]]) {
          x++;
          y++;
          continue;
        }
        break;
      }
    }

    // Mismatch. Maybe restart.
    if (0 < nx && nx <= str.size()) {
      x = nx;
      y = ny;
      continue;
    }
    return false;
  }
  return true;
}

} // namespace mold
