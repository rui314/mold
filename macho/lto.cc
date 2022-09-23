#ifdef _WIN32
# include "lto-win32.cc"
#else
# include "lto-unix.cc"
#endif
