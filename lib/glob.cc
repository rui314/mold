// This file implements a glob matcher that can run multiple glob patterns
// against an input string. mold uses the glob matcher for symbol name
// patterns in a version script or a dynamic list file. Since we may need to
// match hundreds of glob patterns against millions of symbol names, the
// speed of the matcher is very important.
//
// The pattern match implemented in this file is NFA-based, although the
// cost of the function is O(n*m), where n is the number of NFA states and m
// is the length of the input string. We do not use recursion or
// backtracking, unlike a generic NFA-based regular expression matcher. This
// is doable because glob patterns are very limited subsets of regexes.
//
// Here is the explanation of the algorithm. Observe that the only "tricky"
// meta-character in a glob pattern is "*", which matches zero or more
// characters. Other characters and meta-characters always match a single
// input character. So the key of the algorithm is to handle "*" efficiently.
//
// We can represent a glob pattern "a*b*" with three NFA states: q_start, q1
// and q_accept, with the following transition functions:
//
//   δ(q_start, "a") = q1
//   δ(q1, σ) = q1
//   δ(q1, "b") = q_accept
//   δ(q_accept, σ) = q_accept
//
// where σ is any single input character.
//
// We can construct such an NFA in a straightforward manner. We maintain NFA
// states as a list, with the initial contents being the start state. Each
// character except for "*" creates a new NFA state, adds a transition from
// the last state in the list to the new one, and appends the new state at
// the end of the list. "*" sets the "is_star" flag on the last NFA state.
// The flag indicates that the state machine can remain in the state for any
// input character.
//
// An NFA constructed this way doesn't have any complicated loops,
// ε-transitions, or anything like that. Each state has only one incoming
// edge. The only loops in the state transition are the self-loops on states
// followed by a "*". Aside from that, the state machine progresses linearly
// from the start state to the accept state.
//
// Each state of an NFA can be represented by a single bit. If a bit is 1,
// the non-deterministic state machine is in that state. Otherwise, it's
// not. Observe that a state with the "is_star" flag will continued to be 1
// once it becomes 1, since the state machine can loop over the state on any
// input character.
//
// With that observation, we can represent an NFA with a bit vector of N
// bits, where N is the number of NFA states. For each input character, bit
// M becomes 1 if
//
//   - bit M-1 is 1 and there's a transition from state_{M-1} to state_M
//     with the given character, or
//   - bit M is 1 and state_M's "is_star" flag is 1.
//
// Initially, the 0th bit is 1 for the start state. At each step, the bits
// propagate from least significant to most significant positions, at most
// one bit at a time. If the most significant bit is 1 after the entire
// input has been processed, the string matches.
//
// This propagation can be implemented with bitwise OR, bitwise AND, and a
// one-bit bit shift on the bit vector. All these operations are very cheap.
//
// We can combine multiple glob matchers into a single matcher by simply
// concatenating the bit vectors of their state machines.

#include "lib.h"

#include <cstring>

namespace mold {

static std::vector<MultiGlob::State> parse_glob(std::string_view pat) {
  std::vector<MultiGlob::State> vec(1);

  while (!pat.empty()) {
    u8 c = pat[0];
    pat = pat.substr(1);
    std::bitset<256> chars;

    switch (c) {
    case '*':
      vec.back().is_star = true;
      continue;
    case '?':
      chars.set();
      break;
    case '\\':
      if (pat.empty())
        return {};
      chars[pat[0]] = true;
      pat = pat.substr(1);
      break;
    case '[': {
      // Here are a few bracket pattern examples:
      //
      // [abc]: a, b or c
      // [$\]!]: $, ] or !
      // [a-czg-i]: a, b, c, z, g, h, or i
      // [^a-z]: Any character except lowercase letters
      bool negate = false;
      bool closed = false;

      if (!pat.empty() && pat[0] == '^') {
        negate = true;
        pat = pat.substr(1);
      }

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
            chars[i] = true;
        } else {
          chars[pat[0]] = true;
          pat = pat.substr(1);
        }
      }

      if (!closed)
        return {};

      if (negate)
        chars.flip();
      break;
    }
    default:
      chars[c] = true;
      break;
    }

    vec.push_back({chars, false});
  }
  return vec;
}

bool MultiGlob::add(std::string_view pat, i64 val) {
  std::vector<State> vec = parse_glob(pat);
  if (vec.empty())
    return false;

  start_pos.push_back(states.size());
  append(states, vec);
  accept_pos.push_back(states.size() - 1);
  values.push_back(val);
  return true;
}

void MultiGlob::compile() {
  if (states.empty())
    return;
  i64 sz = states.size();

  star_mask.resize(sz);
  for (i64 i = 0; i < sz; i++)
    if (states[i].is_star)
      star_mask[i] = true;

  for (i64 i = 0; i < 256; i++) {
    char_mask[i].resize(sz);
    for (i64 j = 1; j < sz; j++)
      if (states[j].incoming_edge[i])
        char_mask[i][j] = true;
  }
}

i64 MultiGlob::find(std::string_view str) {
  if (states.empty())
    return -1;

  Bitvector bits(states.size());
  Bitvector tmp(states.size());

  for (i64 pos : start_pos)
    bits[pos] = true;

  for (u8 c : str) {
    // This is equivalent to
    //
    //   bits = (bits & star_mask) | ((bits << 1) & char_mask[c])
    //
    // but we update the existing objects in place to avoid allocating
    // temporary objects.
    tmp = bits;
    tmp &= star_mask;
    bits <<= 1;
    bits &= char_mask[c];
    bits |= tmp;
  }

  for (i64 i = 0; i < accept_pos.size(); i++)
    if (bits[accept_pos[i]])
      return values[i];
  return -1;
}

bool Glob::add(std::string_view pat, i64 val) {
  assert(val >= 0);
  assert(!is_compiled);

  // If the pattern requires only a single substring search, the
  // Aho-Corasick algorithm is even faster than our glob matcher.
  if (aho_corasick.can_handle(pat))
    return aho_corasick.add(pat, val);
  return multi_glob.add(pat, val);
}

i64 Glob::find(std::string_view str) {
  std::call_once(once, [&] {
    multi_glob.compile();
    aho_corasick.compile();
    is_compiled = true;
  });

  return std::max(multi_glob.find(str), aho_corasick.find(str));
}

} // namespace mold
