#include "mold.h"

#include <cstring>

namespace mold::elf {

std::optional<GlobPattern> GlobPattern::compile(std::string_view pat) {
  std::vector<Element> vec;

  for (i64 i = 0; i < pat.size(); i++) {
    switch (pat[i]) {
    case '[': {
      vec.push_back({BRACKET});
      std::vector<bool> &bitmap = vec.back().bitmap;
      bitmap.resize(256);

      for (i++; i < pat.size(); i++) {
        if (pat[i] == ']')
          break;
        if (pat[i] == '\\') {
          i++;
          if (i == pat.size())
            return {};
        }
        bitmap[(u8)pat[i]] = true;
      }
      if (i == pat.size())
        return {};
      break;
    }
    case '?':
      vec.push_back({QUESTION});
      break;
    case '*':
      vec.push_back({STAR});
      break;
    default:
      if (vec.empty() || vec.back().kind != STRING)
        vec.push_back({STRING});
      vec.back().str += pat[i];
      break;
    }
  }

  return {GlobPattern{std::move(vec)}};
}

bool GlobPattern::match(std::string_view str) {
  return do_match(str, elements);
}

bool GlobPattern::do_match(std::string_view str, std::span<Element> elements) {
  while (!elements.empty()) {
    Element &e = elements[0];
    elements = elements.subspan(1);

    switch (e.kind) {
    case STRING:
      if (str.empty() || !str.starts_with(e.str))
        return false;
      str = str.substr(e.str.size());
      break;
    case STAR:
      if (elements.empty())
        return true;

      // Patterns like "*foo*bar*" should be much more common than more
      // complex ones like "*foo*[abc]*" or "*foo**?bar*", so we optimize
      // the former case here.
      if (elements[0].kind == STRING) {
        for (;;) {
          size_t pos = str.find(elements[0].str);
          if (pos == str.npos)
            break;
          if (do_match(str.substr(pos + elements[0].str.size()),
                       elements.subspan(1)))
            return true;
          str = str.substr(pos + 1);
        }
        return false;
      }

      // Other cases are handled here.
      for (i64 j = 0; j < str.size(); j++)
        if (do_match(str.substr(j), elements))
          return true;
      return false;
    case QUESTION:
      if (str.empty())
        return false;
      str = str.substr(1);
      break;
    case BRACKET:
      if (str.empty() || !e.bitmap[str[0]])
        return false;
      str = str.substr(1);
      break;
    }
  }

  return str.empty();
}

} // namespace mold::elf
