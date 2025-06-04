#include "lib.h"

// Including mimalloc-new-delete.h overrides new/delete operators.
// We need it only when we are using mimalloc as a dynamic library.
#if MOLD_USE_SYSTEM_MIMALLOC
# include <mimalloc-new-delete.h>
#endif

// Silence mimalloc warning messages that users can just ignore.
#if MOLD_USE_MIMALLOC
# include <mimalloc.h>

namespace mold {
void set_mimalloc_options() {
  mi_option_disable(mi_option_verbose);
  mi_option_disable(mi_option_show_errors);
}
}

#else
namespace mold {
void set_mimalloc_options() {}
}
#endif
