#include "mold.h"

GlobPattern::GlobPattern(std::string_view pat) {
  if (pat.find('*') == pat.npos) {
    kind = EXACT;
    this->pat = pat;
  } else if (pat.ends_with('*') &&
             pat.substr(0, pat.size() - 1).find('*') == pat.npos) {
    kind = PREFIX;
    this->pat = pat.substr(0, pat.size() - 1);
  } else if (pat.starts_with('*') && pat.substr(1).find('*') == pat.npos) {
    kind = SUFFIX;
    this->pat = pat.substr(1);
  } else {
    kind = GENERIC;
    this->pat = pat;
  }
}

static bool generic_match(std::string_view pat, std::string_view str) {
  for (;;) {
    if (pat.empty())
      return str.empty();

    if (pat[0] == '*') {
      for (i64 i = 0; i < str.size(); i++)
        if (generic_match(pat.substr(1), str.substr(i)))
          return true;
      return false;
    }

    if (str.empty() || pat[0] != str[0])
      return false;

    pat = pat.substr(1);
    str = str.substr(1);
  }
}

bool GlobPattern::match(std::string_view str) const {
  switch (kind) {
  case EXACT:
    return str == pat;
  case PREFIX:
    return str.starts_with(pat);
  case SUFFIX:
    return str.ends_with(pat);
  case GENERIC:
    return generic_match(pat, str);
  }
  unreachable();
}
