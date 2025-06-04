#include "mold.h"

namespace mold {

static std::string_view fatal_mono = "mold: fatal: ";
static std::string_view fatal_color = "mold: \033[0;1;31mfatal:\033[0m ";
static std::string_view error_mono = "mold: error: ";
static std::string_view error_color = "mold: \033[0;1;31merror:\033[0m ";
static std::string_view warning_mono = "mold: warning: ";
static std::string_view warning_color = "mold: \033[0;1;35mwarning:\033[0m ";

template <typename E>
Fatal<E>::Fatal(Context<E> &ctx) {
  out << (ctx.arg.color_diagnostics ? fatal_color : fatal_mono);
}

template <typename E>
[[noreturn]] Fatal<E>::~Fatal() {
  out.emit();
  cleanup();
  _exit(1);
}

template <typename E>
Error<E>::Error(Context<E> &ctx) {
  if (ctx.arg.noinhibit_exec) {
    out << (ctx.arg.color_diagnostics ? warning_color : warning_mono);
  } else {
    out << (ctx.arg.color_diagnostics ? error_color : error_mono);
    ctx.has_error = true;
  }
}

template <typename E>
Warn<E>::Warn(Context<E> &ctx) {
  if (ctx.arg.suppress_warnings)
    return;

  out.emplace(std::cerr);

  if (ctx.arg.fatal_warnings) {
    *out << (ctx.arg.color_diagnostics ? error_color : error_mono);
    ctx.has_error = true;
  } else {
    *out << (ctx.arg.color_diagnostics ? warning_color : warning_mono);
  }
}

using E = MOLD_TARGET;

template class Fatal<E>;
template class Error<E>;
template class Warn<E>;

} // namespace mold
