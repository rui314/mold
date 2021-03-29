#include "mold.h"

GlobPattern::GlobPattern(std::string_view pattern) : pat(pattern) {
  if (pat.find('*') == pat.npos) {
    kind = EXACT;
  } else if (pat.ends_with('*') &&
             pat.substr(0, pat.size() - 1).find('*') == pat.npos) {
    kind = PREFIX;
    pat.remove_suffix(1);
  } else if (pat.starts_with('*') && pat.substr(1).find('*') == pat.npos) {
    kind = SUFFIX;
    pat.remove_prefix(1);
  } else {
    kind = GENERIC;
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
}
