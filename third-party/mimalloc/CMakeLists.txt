cmake_minimum_required(VERSION 3.18)
project(libmimalloc C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)

set(MI_SECURE "OFF" CACHE STRING "Use security mitigations (like meta data guard pages, allocation randomization, double-free mitigation, and free-list corruption detection): OFF, ON, FULL. The FULL setting adds guard pages at the end of each mimalloc page (which may be expensive)")
set_property(CACHE MI_SECURE PROPERTY STRINGS "OFF" "ON" "FULL")

option(MI_OVERRIDE          "Override the standard malloc interface (i.e. define entry points for 'malloc', 'free', etc)" ON)
option(MI_XMALLOC           "Enable abort() call on memory allocation failure by default" OFF)
option(MI_SHOW_ERRORS       "Show error and warning messages by default (only enabled by default in DEBUG mode)" OFF)
option(MI_GUARDED           "Build with guard pages behind certain object allocations (enabled by default in a debug build)" OFF)
option(MI_USE_CXX           "Use the C++ compiler to compile the library (instead of the C compiler)" OFF)
option(MI_OPT_ARCH          "Only for optimized builds: turn on architecture specific optimizations (for x64: '-march=haswell;-mavx2' (2013), for arm64: '-march=armv8.3-a' (2016), for riscV: '-march=rv64gcb_zacas' (2023))" OFF)
option(MI_OPT_SIMD          "Use SIMD instructions (requires MI_OPT_ARCH to be enabled)" OFF)

set(MI_FREE_IS_CHECKED "DEFAULT" CACHE STRING "Check for invalid pointers in 'mi_free': OFF, SHARED, ON, DEFAULT. 'SHARED' enables it only for the dynamic/shared library when MI_OVERRIDE is ON")
set_property(CACHE MI_FREE_IS_CHECKED PROPERTY STRINGS "OFF" "SHARED" "ON" "DEFAULT")

set(MI_DEBUG "DEFAULT" CACHE STRING "Enable assertion checks: OFF, ON, INTERNAL, FULL, DEFAULT. INTERNAL adds extra internal invariant checks, while FULL adds expensive heap invariant checks")
set_property(CACHE MI_DEBUG PROPERTY STRINGS "OFF" "ON" "INTERNAL" "FULL" "DEFAULT")

set(MI_TRACK "OFF" CACHE STRING "Enable tracking: OFF, ASAN (address sanitizer), VALGRIND, ETW (Windows event tracing). Adds a small overhead.")
set_property(CACHE MI_TRACK PROPERTY STRINGS "OFF" "ASAN" "VALGRIND" "ETW")

option(MI_DEBUG_TSAN        "Build with thread sanitizer (needs clang)" OFF)
option(MI_DEBUG_UBSAN       "Build with undefined-behavior sanitizer (needs clang++)" OFF)

option(MI_BUILD_SHARED      "Build shared library" ON)
option(MI_BUILD_STATIC      "Build static library" ON)
option(MI_BUILD_OBJECT      "Build object library" ON)
option(MI_BUILD_TESTS       "Build test executables" ON)

# OS specific
option(MI_OSX_INTERPOSE     "Use interpose to override standard malloc on macOS" ON)
option(MI_OSX_ZONE          "Use malloc zone to override standard malloc on macOS" ON)
option(MI_WIN_REDIRECT      "Use redirection module ('mimalloc-redirect') on Windows if compiling mimalloc as a DLL" ON)
option(MI_LIBC_MUSL         "Enable this when linking with musl libc" OFF)

# advanced
option(MI_INSTALL_TOPLEVEL  "Install directly into $CMAKE_INSTALL_PREFIX instead of PREFIX/lib/mimalloc-version" OFF)
option(MI_NO_THP            "Disable transparent huge pages support on Linux/Android for the mimalloc process only" OFF)
option(MI_FREE_USE_PAGEMAP  "Always use the page map when free-ing a pointer -- enable only for systems that don't support large OS aligned allocations well." OFF)
option(MI_PADDING           "Enable padding to detect heap block overflow (always on in DEBUG or SECURE mode, or with Valgrind/ASAN)" OFF)
option(MI_NO_PADDING        "Force no use of padding even in DEBUG mode etc." OFF)
option(MI_SKIP_COLLECT_ON_EXIT "Skip collecting memory on program exit" OFF)
option(MI_EXTRA_CPPDEFS     "Extra pre-processor definitions (use as `-DMI_EXTRA_CPPDEFS=\"opt1=val1;opt2=val2\"`)" "")
option(MI_SEE_ASM           "Generate assembly files" OFF)

# negated options for vcpkg features
option(MI_NO_USE_CXX        "Use plain C compilation (has priority over MI_USE_CXX)" OFF)
option(MI_NO_OPT_ARCH       "Do not use architecture specific optimizations (like '-march=armv8.3-a' for example) (has priority over MI_OPT_ARCH)" OFF)
option(MI_NO_DEBUG          "Disable all assertion checks even in debug builds" OFF)

# experimental
option(MI_TLS_RECURSE_GUARD "Force a recursion check on thread local access (for MI_TLS_MODEL_LOCAL only)" OFF)

set(MI_WIN_INIT "DEFAULT" CACHE STRING "Force Windows intialization strategy: CRT_TLS (default), RAW_DLLMAIN (use raw DLL main entry point), TLS_DLLMAIN (TLS entries with DllMain), FLS (legacy fiber API), DEFAULT")
set_property(CACHE MI_WIN_INIT PROPERTY STRINGS "CRT_TLS" "RAW_DLLMAIN" "TLS_DLLMAIN" "FLS" "DEFAULT")

set(MI_TLS_MODEL "DEFAULT" CACHE STRING "Force TLS model: LOCAL (thread local variables), PTHREADS (pthread locals), LOCAL_DYNAMIC (dlopen-compatible thread local storage mechanism (Unix)), WIN32 (direct Windows TLS slots), FIXED (fixed thread local slots), DEFAULT.")
set_property(CACHE MI_TLS_MODEL PROPERTY STRINGS "LOCAL" "PTHREADS" "LOCAL_DYNAMIC" "WIN32" "FIXED" "DEFAULT")

# deprecated options
option(MI_USE_LIBATOMIC       "Deprecated: Explicitly link with -latomic (on older systems) (deprecated and detected automatically)" OFF)
option(MI_OPT_FREE_SMALL      "Deprecated: Use aligned `mi_free_small` without pagemap lookup (can only be used if SECURE or GUARDED mode are off)" OFF)
option(MI_DEBUG_INTERNAL      "Deprecated: Enable assertion and internal invariant checks (use MI_DEBUG=INTERNAL instead)" OFF)
option(MI_DEBUG_FULL          "Deprecated: Enable assertion checks and expensive internal heap invariant checking (use MI_DEBUG=FULL instead)" OFF)
option(MI_CHECK_FULL          "Deprecated: Use full internal invariant checking in DEBUG mode (use MI_DEBUG=FULL instead)" OFF)
option(MI_SECURE_FULL         "Deprecated: Use full security mitigations including guard pages at the end of each mimalloc page (may be expensive) (use MI_SECURE=FULL instead)" OFF)
option(MI_TLS_MODEL_LOCAL     "Deprecated: Force TLS model based on thread local variables (use MI_TLS_MODEL=LOCAL instead)" OFF)
option(MI_TLS_MODEL_PTHREADS  "Deprecated: Force TLS model based on pthreads (use MI_TLS_MODEL=PTHREADS instead)" OFF)
option(MI_TLS_MODEL_FIXED     "Deprecated: Force TLS model based on fixed thread local slots (use MI_TLS_MODEL=FIXED instead)" OFF)
option(MI_LOCAL_DYNAMIC_TLS   "Deprecated: Use local-dynamic-tls, a slightly slower but dlopen-compatible thread local storage mechanism (Unix) (use MI_TLS_MODEL=LOCAL_DYNAMIC instead)" OFF)
option(MI_WIN_USE_FLS         "Deprecated: Use Fiber local storage on Windows to detect thread termination (use MI_WIN_INIT=FLS instead)" OFF)
option(MI_WIN_INIT_USE_RAW_DLLMAIN "Deprecated: Use the raw DLL main entry point for mimalloc initialization; can be more robust but can also lead to link errors with other libraries (use MI_WIN_INIT=RAW_DLLMAIN instead)" OFF)
option(MI_WIN_INIT_USE_TLS_DLLMAIN "Deprecated: Use legacy TLS entries with DllMain for mimalloc initialization (use MI_WIN_INIT=TLS_DLLMAIN instead)" OFF)
option(MI_WIN_DIRECT_TLS      "Deprecated: Use only direct TLS slots on Windows to avoid extra tests in the malloc fast path (only works if the program uses less than 64 TlsAlloc'd slots in total)" OFF)
option(MI_TRACK_VALGRIND      "Deprecated: Compile with Valgrind support (adds a small overhead) (use MI_TRACK=VALGRIND instead)" OFF)
option(MI_TRACK_ASAN          "Deprecated: Compile with address sanitizer support (adds a small overhead) (use MI_TRACK=ASAN instead)" OFF)
option(MI_TRACK_ETW           "Deprecated: Compile with Windows event tracing (ETW) support (adds a small overhead) (use MI_TRACK=ETW instead)" OFF)


include(CheckLinkerFlag)    # requires cmake 3.18
include(CheckIncludeFiles)
include(GNUInstallDirs)
include("cmake/mimalloc-config-version.cmake")

set(mi_sources
    src/alloc.c
    src/alloc-aligned.c
    src/alloc-posix.c
    src/arena.c
    src/bitmap.c
    src/heap.c
    src/init.c
    src/libc.c
    src/options.c
    src/os.c
    src/page.c
    src/page-map.c
    src/random.c
    src/stats.c
    src/subproc.c
    src/theap.c
    src/threadlocal.c
    src/prim/prim.c
    src/prim/prim-tls.c)

set(mi_cflags "")
set(mi_cflags_static "")            # extra flags for a static library build
set(mi_cflags_dynamic "")           # extra flags for a shared-object library build
set(mi_libraries "")

if(MI_EXTRA_CPPDEFS)
 set(mi_defines ${MI_EXTRA_CPPDEFS})
else()
 set(mi_defines "")
endif()

# pass git revision as a define
if(EXISTS "${CMAKE_SOURCE_DIR}/.git/index")
  find_package(Git)
  if(GIT_FOUND)
    execute_process(COMMAND ${GIT_EXECUTABLE} "describe" OUTPUT_VARIABLE mi_git_describe RESULT_VARIABLE mi_git_res ERROR_QUIET OUTPUT_STRIP_TRAILING_WHITESPACE)
    if(mi_git_res EQUAL "0")
      list(APPEND mi_defines "MI_GIT_DESCRIBE=${mi_git_describe}")
      # add to dependencies so we rebuild if the git head commit changes
      set_property(GLOBAL APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_SOURCE_DIR}/.git/index")
    endif()
  endif()
endif()

# -----------------------------------------------------------------------------
# Convenience: set default build type and compiler depending on the build directory
# -----------------------------------------------------------------------------

message(STATUS "")
if(NOT CMAKE_BUILD_TYPE)
  if ("${CMAKE_BINARY_DIR}" MATCHES ".*((D|d)ebug|asan|tsan|ubsan|valgrind)$")
    message(STATUS "No build type selected, default to 'Debug'")
    set(CMAKE_BUILD_TYPE "Debug")
  else()
    message(STATUS "No build type selected, default to 'Release'")
    set(CMAKE_BUILD_TYPE "Release")
  endif()
endif()

if(CMAKE_GENERATOR MATCHES "^Visual Studio.*$")
  message(STATUS "Note: when building with Visual Studio the build type is specified when building.")
  message(STATUS "For example: 'cmake --build . --config=Release")
endif()

if(NOT MI_SECURE AND NOT MI_SECURE_FULL AND "${CMAKE_BINARY_DIR}" MATCHES ".*(S|s)ecure$")
  message(STATUS "Default to secure build")
  set(MI_SECURE "ON")
endif()


# Determine architecture
set(MI_OPT_ARCH_FLAGS "")
set(MI_ARCH "unknown")
if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(x86|i[3456]86)$" OR CMAKE_GENERATOR_PLATFORM MATCHES "^(x86|Win32)$")
  set(MI_ARCH "x86")
elseif((CMAKE_SYSTEM_PROCESSOR MATCHES "^(x86_64|x64|amd64|AMD64)$" OR CMAKE_GENERATOR_PLATFORM STREQUAL "x64" OR "x86_64" IN_LIST CMAKE_OSX_ARCHITECTURES) AND NOT CMAKE_GENERATOR_PLATFORM STREQUAL "ARM64") # must be before arm64
  set(MI_ARCH "x64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(aarch64|arm64|armv[89].?|ARM64)$" OR CMAKE_GENERATOR_PLATFORM STREQUAL "ARM64" OR "arm64" IN_LIST CMAKE_OSX_ARCHITECTURES)
  set(MI_ARCH "arm64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(arm|armv[34567].?|ARM)$")
  set(MI_ARCH "arm32")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "^(riscv|riscv32|riscv64)$")
  if(CMAKE_SIZEOF_VOID_P==4)
    set(MI_ARCH "riscv32")
  else()
    set(MI_ARCH "riscv64")
  endif()
else()
  set(MI_ARCH ${CMAKE_SYSTEM_PROCESSOR})
endif()
message(STATUS "Architecture: ${MI_ARCH}") # (${CMAKE_SYSTEM_PROCESSOR}, ${CMAKE_GENERATOR_PLATFORM}, ${CMAKE_GENERATOR})")

# negative overrides (mainly to support vcpkg features)
if(MI_NO_USE_CXX)
  set(MI_USE_CXX "OFF")
endif()

if(MI_NO_OPT_ARCH)
  set(MI_OPT_ARCH "OFF")
elseif(MI_ARCH STREQUAL "arm64")
  set(MI_OPT_ARCH "ON")  # enable armv8.3-a by default (has LDAPR) on arm64 unless MI_NO_OPT_ARCH is set
endif()

if(DEFINED ENV{MI_TEST_LIGHT})
  list(APPEND mi_defines MI_TEST_LIGHT=1)  # to lighten test load in test integration
endif()

# -----------------------------------------------------------------------------
# Enable the C++ compiler early on if needed
# -----------------------------------------------------------------------------

# clang-cl detection on windows
if(CMAKE_C_COMPILER_ID STREQUAL "Clang" AND CMAKE_C_COMPILER_FRONTEND_VARIANT STREQUAL "MSVC")
  set(MI_CLANG_CL "ON")
endif()

# force C++ compilation with msvc or clang-cl to use modern C++ atomics
if(CMAKE_C_COMPILER_ID MATCHES "MSVC|Intel" OR MI_CLANG_CL)
  set(MI_USE_CXX "ON")
elseif(MI_DEBUG_UBSAN AND CMAKE_BUILD_TYPE MATCHES "Debug")  # ubsan needs C++
  set(MI_USE_CXX "ON")
endif()

# enable C++ ?
if(MI_USE_CXX)
  enable_language(CXX)
  message(STATUS "Use the C++ compiler to compile (MI_USE_CXX=ON)")
  if(CMAKE_CXX_COMPILER_ID MATCHES "Intel" AND NOT CMAKE_CXX_COMPILER_ID MATCHES "IntelLLVM")
    list(APPEND mi_cflags -Kc++)
  endif()
endif()


# -----------------------------------------------------------------------------
# Process options
# -----------------------------------------------------------------------------

# put -Wall early so other warnings can be disabled selectively
if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang")
  if (MI_CLANG_CL)
    list(APPEND mi_cflags -W)
  else()
    list(APPEND mi_cflags -Wall -Wextra -Wpedantic)
  endif()
endif()
if(CMAKE_C_COMPILER_ID MATCHES "GNU")
    list(APPEND mi_cflags -Wall -Wextra)
endif()
if(CMAKE_C_COMPILER_ID MATCHES "Intel")
    list(APPEND mi_cflags -Wall)
endif()

if(CMAKE_CXX_COMPILER_ID MATCHES "AppleClang|Clang")
  list(APPEND mi_cflags -Wno-deprecated)
endif()

if(MI_OVERRIDE)
  message(STATUS "Override standard malloc (MI_OVERRIDE=ON)")
  if(APPLE)
    if(MI_OSX_ZONE)
      # use zone's on macOS
      message(STATUS "  Use malloc zone to override malloc (MI_OSX_ZONE=ON)")
      list(APPEND mi_sources src/prim/osx/alloc-override-zone.c)
      list(APPEND mi_defines MI_OSX_ZONE=1)
      if (NOT MI_OSX_INTERPOSE)
        message(STATUS "  WARNING: zone overriding usually also needs interpose (use -DMI_OSX_INTERPOSE=ON)")
      endif()
    endif()
    if(MI_OSX_INTERPOSE)
      # use interpose on macOS
      message(STATUS "  Use interpose to override malloc (MI_OSX_INTERPOSE=ON)")
      list(APPEND mi_defines MI_OSX_INTERPOSE=1)
      if (NOT MI_OSX_ZONE)
        message(STATUS "  WARNING: interpose usually also needs zone overriding (use -DMI_OSX_ZONE=ON)")
      endif()
    endif()
    if(MI_USE_CXX AND MI_OSX_INTERPOSE)
      message(STATUS "  WARNING: if dynamically overriding malloc/free, it is more reliable to build mimalloc as C code (use -DMI_USE_CXX=OFF)")
    endif()
  endif()
endif()

if(WIN32)
  if (NOT MI_WIN_REDIRECT)
    # use a negative define for backward compatibility
    list(APPEND mi_defines MI_WIN_NOREDIRECT=1)
  endif()
endif()

if(MI_SECURE_FULL)
  message(STATUS "MI_SECURE_FULL is deprecated, use MI_SECURE=FULL instead")
  set(MI_SECURE "FULL")
endif()
if(MI_SECURE STREQUAL "FULL")  
  message(STATUS "Use full secure build (MI_SECURE=FULL, may be expensive)")
  list(APPEND mi_defines MI_SECURE=5)
elseif(MI_SECURE)
  message(STATUS "Use secure build (MI_SECURE=${MI_SECURE})")
  list(APPEND mi_defines MI_SECURE=4)
endif()

if(MI_TRACK_VALGRIND)
  message(STATUS "Use of MI_TRACK_VALGRIND is deprecated, use MI_TRACK=VALGRIND instead")
  set(MI_TRACK "VALGRIND")
elseif(MI_TRACK_ASAN)
  message(STATUS "Use of MI_TRACK_ASAN is deprecated, use MI_TRACK=ASAN instead")
  set(MI_TRACK "ASAN")
elseif(MI_TRACK_ETW)
  message(STATUS "Use of MI_TRACK_ETW is deprecated, use MI_TRACK=ETW instead")
  set(MI_TRACK "ETW")
endif()

if(MI_TRACK STREQUAL "VALGRIND")
  CHECK_INCLUDE_FILES("valgrind/valgrind.h;valgrind/memcheck.h" MI_HAS_VALGRINDH)
  if (NOT MI_HAS_VALGRINDH)
    set(MI_TRACK OFF)
    message(WARNING "Cannot find the 'valgrind/valgrind.h' and 'valgrind/memcheck.h' -- install valgrind first?")
    message(STATUS  "Disabling Valgrind support (MI_TRACK=OFF)")
  else()
    message(STATUS "Compile with Valgrind support (MI_TRACK=VALGRIND)")
    list(APPEND mi_defines MI_TRACK_VALGRIND=1)
  endif()
elseif(MI_TRACK STREQUAL "ASAN")
  if (APPLE AND MI_OVERRIDE)
    set(MI_TRACK OFF)
    message(WARNING "Cannot enable address sanitizer support on macOS if MI_OVERRIDE is ON (MI_TRACK=OFF)")
  else()
    CHECK_INCLUDE_FILES("sanitizer/asan_interface.h" MI_HAS_ASANH)
    if (NOT MI_HAS_ASANH)
      set(MI_TRACK OFF)
      message(WARNING "Cannot find the 'sanitizer/asan_interface.h' -- install address sanitizer support first")
      message(STATUS  "Compile **without** address sanitizer support (MI_TRACK=OFF)")
    else()
      message(STATUS "Compile with address sanitizer support (MI_TRACK=ASAN)")
      list(APPEND mi_defines MI_TRACK_ASAN=1)
      list(APPEND mi_cflags -fsanitize=address)
      list(APPEND mi_libraries -fsanitize=address)
    endif()
  endif()
elseif(MI_TRACK STREQUAL "ETW")
  if(NOT WIN32)
    set(MI_TRACK OFF)
    message(WARNING "Can only enable ETW support on Windows (MI_TRACK=OFF)")
  else()
    message(STATUS "Compile with Windows event tracing support (MI_TRACK=ETW)")
    list(APPEND mi_defines MI_TRACK_ETW=1)
  endif()
elseif(MI_TRACK)
  message(STATUS "Invalid option value: MI_TRACK=${MI_TRACK} (ignored)")
endif()

if(MI_SEE_ASM)
  message(STATUS "Generate assembly listings (MI_SEE_ASM=ON)")
  if(CMAKE_C_COMPILER_ID MATCHES "MSVC" OR MI_CLANG_CL)
    list(APPEND mi_cflags -FA)
  else()
    list(APPEND mi_cflags -save-temps)
    if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang" AND CMAKE_C_COMPILER_VERSION VERSION_GREATER 14)
      message(STATUS "No GNU Line marker")
      list(APPEND mi_cflags -Wno-gnu-line-marker)
    endif()
  endif()
endif()


if (MI_SKIP_COLLECT_ON_EXIT)
  message(STATUS "Skip collecting memory on program exit (MI_SKIP_COLLECT_ON_EXIT=ON)")
  list(APPEND mi_defines MI_SKIP_COLLECT_ON_EXIT=1)
endif()

if(MI_CHECK_FULL OR MI_DEBUG_FULL)
  message(STATUS "Use of MI_CHECK_FULL/MI_DEBUG_FULL is deprecated, use MI_DEBUG=FULL instead")
  if(MI_DEBUG STREQUAL "DEFAULT")
    set(MI_DEBUG "FULL")
  endif()
elseif(MI_DEBUG_INTERNAL)
  message(STATUS "Use of MI_DEBUG_INTERNAL is deprecated, use MI_DEBUG=INTERNAL instead")
  if(MI_DEBUG STREQUAL "DEFAULT")
    set(MI_DEBUG "INTERNAL")
  endif()
endif()

if(MI_DEBUG STREQUAL "DEFAULT")
  if(CMAKE_BUILD_TYPE MATCHES "Debug")
    set(MI_DEBUG "INTERNAL")
  else()
    set(MI_DEBUG "OFF")
  endif()
endif()

if(MI_NO_DEBUG)
  message(STATUS "Disable assertion checks (MI_NO_DEBUG=ON)")
  list(APPEND mi_defines NDEBUG=1)
  set(MI_DEBUG OFF)
elseif(MI_DEBUG STREQUAL "FULL")
  message(STATUS "Set debug level to full assertion and internal invariant checking (MI_DEBUG=FULL, expensive)")
  list(APPEND mi_defines MI_DEBUG=3)   # full invariant checking (mi_assert, mi_assert_internal, and mi_assert_expensive)
elseif(MI_DEBUG STREQUAL "INTERNAL")
  message(STATUS "Set debug level to internal assertion and invariant checking (MI_DEBUG=INTERNAL)")
  list(APPEND mi_defines MI_DEBUG=2)   # invariant checking (mi_assert and mi_assert_internal)
elseif(MI_DEBUG)
  message(STATUS "Set debug level to assertion checking (MI_DEBUG=${MI_DEBUG})")
  list(APPEND mi_defines MI_DEBUG=1)   # assertion checking (mi_assert)
endif()

if(MI_DEBUG AND !MI_GUARDED)
  message(STATUS "Enable MI_GUARDED (since MI_DEBUG is enabled)")
  set(MI_GUARDED=ON)
endif()
if(MI_GUARDED)
  message(STATUS "Compile guard pages behind certain object allocations (MI_GUARDED=ON)")
  list(APPEND mi_defines MI_GUARDED=1)
endif()

if(MI_NO_PADDING)
  message(STATUS "Suppress any padding of heap blocks (MI_NO_PADDING=ON)")
  list(APPEND mi_defines MI_PADDING=0)
else()
  if(MI_PADDING)
    message(STATUS "Enable explicit padding of heap blocks (MI_PADDING=ON)")
    list(APPEND mi_defines MI_PADDING=1)
  endif()
endif()

if(MI_FREE_IS_CHECKED STREQUAL "DEFAULT")
  if (MI_SECURE)
    set(MI_FREE_IS_CHECKED ON)
  else()
    set(MI_FREE_IS_CHECKED OFF)
  endif()
endif()

if(MI_FREE_IS_CHECKED STREQUAL "ON")
  message(STATUS "Check validity eof pointers passed to 'mi_free' (MI_FREE_IS_CHECKED=ON)")
  list(APPEND mi_defines MI_FREE_IS_CHECKED=1)
elseif(MI_SECURE)
  message(STATUS "Check validity of pointers passed to 'mi_free' (MI_SECURE=ON, MI_FREE_IS_CHECKED=ON)")    
elseif(MI_FREE_IS_CHECKED STREQUAL "SHARED")
  message(STATUS "Check validity of pointers passed to 'mi_free' only in the dynamic/shared link library (MI_FREE_IS_CHECKED=SHARED)")
  list(APPEND mi_cflags_dynamic -DMI_FREE_IS_CHECKED=1)
elseif(MI_FREE_IS_CHECKED STREQUAL "OFF")
  # message(STATUS "Do not check validity of pointers passed to 'mi_free' (MI_FREE_IS_CHECKED=OFF)")
else()
  message(STATUS "Invalid value for MI_FREE_IS_CHECKED: ${MI_FREE_IS_CHECKED} (expecting: DEFAULT, OFF, SHARED, or ON)")
endif()

if(CMAKE_SYSTEM_NAME STREQUAL "WASI")
  message(STATUS "Enabling MI_FREE_USE_PAGEMAP since the target (WASI) does not support large OS aligned allocations well")
  set(MI_FREE_USE_PAGEMAP ON)
endif()
if(MI_FREE_USE_PAGEMAP)
  message(STATUS "Always use the pagemap in mi_free (MI_FREE_USE_PAGEMAP=ON)")
  list(APPEND mi_defines MI_FREE_USE_PAGEMAP=1)
endif()  

# deprecated
if(MI_OPT_FREE_SMALL)
  if(MI_GUARDED OR MI_SECURE)
    message(STATUS "Cannot enable MI_OPT_FREE_SMALL in secure or guarded mode (MI_OPT_FREE_SMALL=OFF)")
  elseif(MI_OPT_FREE)
    message(STATUS "Cannot enable MI_OPT_FREE_SMALL with MI_OPT_FREE (MI_OPT_FREE_SMALL=OFF)")
  else()
    message(STATUS "Use aligned `mi_free_small` (MI_OPT_FREE_SMALL=ON)")
    list(APPEND mi_defines MI_OPT_FREE_SMALL=1)
  endif()
endif()

if(MI_XMALLOC)
  message(STATUS "Enable abort() calls on memory allocation failure (MI_XMALLOC=ON)")
  list(APPEND mi_defines MI_XMALLOC=1)
endif()

if(MI_SHOW_ERRORS)
  message(STATUS "Enable printing of error and warning messages by default (MI_SHOW_ERRORS=ON)")
  list(APPEND mi_defines MI_SHOW_ERRORS=1)
endif()

if(MI_DEBUG_TSAN)
  if(CMAKE_C_COMPILER_ID MATCHES "Clang")
    message(STATUS "Build with thread sanitizer (MI_DEBUG_TSAN=ON)")
    list(APPEND mi_defines MI_TSAN=1)
    list(APPEND mi_cflags -fsanitize=thread -g -O1)
    list(APPEND mi_libraries -fsanitize=thread)
  else()
    message(WARNING "Can only use thread sanitizer with clang (MI_DEBUG_TSAN=ON but ignored)")
  endif()
endif()

if(MI_DEBUG_UBSAN)
  if(CMAKE_BUILD_TYPE MATCHES "Debug")
    if(MI_USE_CXX)
      if(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
        message(STATUS "Build with undefined-behavior sanitizer (MI_DEBUG_UBSAN=ON)")
        list(APPEND mi_defines MI_UBSAN=1)
        list(APPEND mi_cflags -fsanitize=undefined -g -fno-sanitize-recover=undefined)
        list(APPEND mi_libraries -fsanitize=undefined)
      else()
        message(WARNING "Can only use undefined-behavior sanitizer with clang++ (MI_DEBUG_UBSAN=ON but ignored)")
      endif()
    else()
      message(WARNING "Can only use undefined-behavior sanitizer with a C++ build (MI_USE_CXX=ON)")
    endif()
  else()
    message(WARNING "Can only use undefined-behavior sanitizer with a debug build (CMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE})")
  endif()
endif()

if(CMAKE_SYSTEM_NAME MATCHES "Linux|Android")
  if(MI_NO_THP)
    message(STATUS "Disable transparent huge pages support (MI_NO_THP=ON)")
    list(APPEND mi_defines MI_DEFAULT_ALLOW_THP=0)
  endif()
endif()

if(MI_LIBC_MUSL)
  message(STATUS "Assume using musl libc (MI_LIBC_MUSL=ON)")
  list(APPEND mi_defines MI_LIBC_MUSL=1)
endif()

if(MI_TLS_MODEL STREQUAL "DEFAULT")
  if(MI_TLS_MODEL_PTHREADS)
    message(STATUS "Use of MI_TLS_MODEL_PTHREADS is deprecated, use MI_TLS_MODEL=PTHREADS instead")
    set(MI_TLS_MODEL "PTHREADS")
  elseif(MI_TLS_MODEL_FIXED)
    message(STATUS "Use of MI_TLS_MODEL_FIXED is deprecated, use MI_TLS_MODEL=FIXED instead")
    set(MI_TLS_MODEL "FIXED")
  elseif(MI_TLS_MODEL_LOCAL)
    message(STATUS "Use of MI_TLS_MODEL_LOCAL is deprecated, use MI_TLS_MODEL=LOCAL instead")
    set(MI_TLS_MODEL "LOCAL")
  elseif(MI_LOCAL_DYNAMIC_TLS)
    message(STATUS "Use of MI_LOCAL_DYNAMIC_TLS is deprecated, use MI_TLS_MODEL=LOCAL_DYNAMIC instead")
    set(MI_TLS_MODEL "LOCAL_DYNAMIC")
  endif()
endif()
if(MI_TLS_MODEL STREQUAL "PTHREADS")
  message(STATUS "Use TLS model based on pthreads (MI_TLS_MODEL=PTHREADS)")
  list(APPEND mi_defines MI_TLS_MODEL_PTHREADS=1)
elseif(MI_TLS_MODEL STREQUAL "FIXED")
  message(STATUS "Use TLS model based on fixed thread local slots (MI_TLS_MODEL=FIXED)")
  message(STATUS "Note: fixed TLS slots do not allow multiple mimalloc instances in a single process.")
  list(APPEND mi_defines MI_TLS_MODEL=FIXED=1)
elseif(MI_TLS_MODEL STREQUAL "LOCAL")
  message(STATUS "Use TLS model based on thread local (MI_TLS_MODEL=LOCAL)")
  if(APPLE)
  message(WARNING "On macOS the MI_TLS_MODEL=LOCAL does not work reliably when dynamically overriding mimalloc. MI_TLS_MODEL=PTHREADS is recommended instead.")
  endif()
  list(APPEND mi_defines MI_TLS_MODEL_LOCAL=1)
elseif(MI_TLS_MODEL STREQUAL "LOCAL_DYNAMIC")
  message(STATUS "Use TLS model based on dlopen compatible dynamic thread locals (MI_TLS_MODEL=LOCAL_DYNAMIC)")
  list(APPEND mi_defines MI_TLS_MODEL_LOCAL=1)
elseif(MI_TLS_MODEL STREQUAL "WIN32")
  message(STATUS "Use TLS model based on direct Win32 TlsAlloc'd slots (MI_TLS_MODEL=WIN32)")
  list(APPEND mi_defines MI_TLS_MODEL_WIN32=1)
elseif(NOT (MI_TLS_MODEL STREQUAL "DEFAULT"))
  message(STATUS "Invalid option value: MI_TLS_MODEL=${MI_TLS_MODEL} (ignored)")
endif()

if(MI_TLS_RECURSE_GUARD)
  message(STATUS "Use TLS recursion guard (for MI_TLS_MODEL_LOCAL only)")
  list(APPEND mi_defines MI_TLS_RECURSE_GUARD=1)
endif()

if("$ENV{MSYSTEM}" STREQUAL "UCRT64")
  message(STATUS "Detected MSYS2 UCRT64 environment")
  set(MI_MINGW_UCRT64 "ON")
  list(APPEND mi_defines MI_MINGW_UCRT64=1)
elseif("$ENV{MSYSTEM}" STREQUAL "MSYS")
  message(STATUS "Detected MSYS2 MSYS/Cygwin environment")
  set(MI_CYGWIN "ON")
endif()

if(MI_WIN_DIRECT_TLS)
  message(STATUS "Use only direct TLS slots on Windows to avoid extra tests in the malloc fast path -- this only works if the program uses less than 64 TlsAlloc'd slots in total! (MI_WIN_USE_ONLY_DIRECT_TLS=ON)")
  list(APPEND mi_defines MI_WIN_DIRECT_TLS=1)
endif()

if(MI_WIN_INIT STREQUAL "DEFAULT")
  if(MI_WIN_INIT_USE_RAW_DLLMAIN)
    message(STATUS "Use of MI_WIN_INIT_USE_RAW_DLLMAIN is deprecated, use MI_WIN_INIT=RAW_DLLMAIN instead")
    set(MI_WIN_INIT "RAW_DLLMAIN")
  elseif(MI_WIN_INIT_USE_TLS_DLLMAIN)
    message(STATUS "Use of MI_WIN_INIT_USE_TLS_DLLMAIN is deprecated, use MI_WIN_INIT=TLS_DLLMAIN instead")
    set(MI_WIN_INIT "TLS_DLLMAIN")
  elseif(MI_WIN_USE_FLS)
    message(STATUS "Use of MI_WIN_USE_FLS is deprecated, use MI_WIN_INIT=FLS instead")
    set(MI_WIN_INIT "FLS")
  endif()
endif()
  
if(MI_WIN_INIT STREQUAL "RAW_DLLMAIN")
  message(STATUS "Use raw Dll main entry on Windows to initialize and finalize mimalloc (MI_WIN_INIT=RAW_DLLMAIN)")
  message(STATUS "Note: this can lead to link errors if other libraries also try to define a raw Dll main entry point")
  list(APPEND mi_defines MI_WIN_INIT_USE_RAW_DLLMAIN=1)
elseif(MI_WIN_INIT STREQUAL "TLS_DLLMAIN")
  message(STATUS "Use Dll main entry with TLS entries on Windows to initialize and finalize mimalloc (MI_WIN_INIT=TLS_DLLMAIN)")
  list(APPEND mi_defines MI_WIN_INIT_USE_TLS_DLLMAIN=1)
elseif(MI_WIN_INIT STREQUAL "FLS")
  message(STATUS "Use the Fiber API to detect thread termination (deprecated) (MI_WIN_INIT=FLS)")
  list(APPEND mi_defines MI_WIN_INIT_USE_FLS=1)
elseif(NOT (MI_WIN_INIT STREQUAL "DEFAULT"))
  message(STATUS "Invalid option value: MI_WIN_INIT=${MI_WIN_INIT} (ignored)")
endif()

if(MI_ARCH MATCHES "riscv")
  CHECK_INCLUDE_FILES("asm/hwprobe.h" MI_HAS_ASM_HWPROBEH)
  if (MI_HAS_ASM_HWPROBEH)
    list(APPEND mi_defines MI_HAS_ASM_HWPROBEH=1)
  endif()
  CHECK_INCLUDE_FILES("sys/hwprobe.h" MI_HAS_SYS_HWPROBEH)
  if (MI_HAS_SYS_HWPROBEH)
    list(APPEND mi_defines MI_HAS_SYS_HWPROBEH=1)
  endif()
endif()

# On Haiku use `-DCMAKE_INSTALL_PREFIX` instead, issue #788
# if(CMAKE_SYSTEM_NAME MATCHES "Haiku")
#   SET(CMAKE_INSTALL_LIBDIR ~/config/non-packaged/lib)
#   SET(CMAKE_INSTALL_INCLUDEDIR ~/config/non-packaged/headers)
# endif()

# Compiler flags
if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang|GNU" AND NOT MI_CLANG_CL)
  list(APPEND mi_cflags -Wno-unknown-pragmas -fvisibility=hidden)
  if(NOT MI_USE_CXX)
    list(APPEND mi_cflags -Wstrict-prototypes)
  endif()
  if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang")
    list(APPEND mi_cflags -Wno-static-in-inline -mno-outline)
  endif()
endif()

if(CMAKE_C_COMPILER_ID MATCHES "Intel")
  list(APPEND mi_cflags -fvisibility=hidden)
endif()

if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang|GNU|Intel" AND NOT CMAKE_SYSTEM_NAME MATCHES "Haiku" AND NOT MI_CLANG_CL)
  if(MI_TLS_MODEL STREQUAL "LOCAL_DYNAMIC")
    list(APPEND mi_cflags -ftls-model=local-dynamic)
  else()
    if(MI_LIBC_MUSL)
      # with musl we use local-dynamic for the static build, see issue #644
      list(APPEND mi_cflags_static  -ftls-model=local-dynamic)
      list(APPEND mi_cflags_dynamic -ftls-model=initial-exec)
      message(STATUS "Use local dynamic TLS for the static build (since MI_LIBC_MUSL=ON)")
    else()
      list(APPEND mi_cflags -ftls-model=initial-exec)
    endif()
  endif()
endif()

if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang|GNU|Intel")
  if(MI_OVERRIDE)
    list(APPEND mi_cflags -fno-builtin-malloc)
  endif()
endif()

# Compiler and architecture specific flags
if(CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang|GNU|Intel" AND NOT CMAKE_SYSTEM_NAME MATCHES "Haiku")
  if(MI_OPT_ARCH)
    if(APPLE AND CMAKE_C_COMPILER_ID MATCHES "AppleClang|Clang" AND CMAKE_OSX_ARCHITECTURES)   # to support multi-arch binaries (#999)
      if("arm64" IN_LIST CMAKE_OSX_ARCHITECTURES)
        list(APPEND MI_OPT_ARCH_FLAGS "-Xarch_arm64;-march=armv8.3-a")
      endif()
      if("x86_64" IN_LIST CMAKE_OSX_ARCHITECTURES)
        list(APPEND MI_OPT_ARCH_FLAGS "-Xarch_x86_64;-march=haswell;-Xarch_x86_64;-mavx2")
      endif()
    elseif(MI_ARCH STREQUAL "x64")
      set(MI_OPT_ARCH_FLAGS "-march=haswell;-mavx2")    # fast bit scan (since 2013)
    elseif(MI_ARCH STREQUAL "arm64")
      set(MI_OPT_ARCH_FLAGS "-march=armv8.3-a")         # fast atomics (since 2016)
    elseif(MI_ARCH STREQUAL "riscv64")
      set(MI_OPT_ARCH_FLAGS "-march=rv64gcb_zacas")     # fast bit scan and atomics (since 2023) TODO: add zalasr (2025) when possible.
    elseif(MI_ARCH STREQUAL "riscv32")
      set(MI_OPT_ARCH_FLAGS "-march=rv32gcb_zacas")     # fast bit scan and atomics (since 2023)
    endif()
  endif()
endif()

if (MSVC AND MSVC_VERSION GREATER_EQUAL 1914) # vs2017+
  list(APPEND mi_cflags /Zc:__cplusplus)
  if(MI_OPT_ARCH AND NOT MI_CLANG_CL)
    if(MI_ARCH STREQUAL "x64")
      set(MI_OPT_ARCH_FLAGS "/arch:AVX2")
    elseif(MI_ARCH STREQUAL "arm64")
      set(MI_OPT_ARCH_FLAGS "/arch:armv8.3")           # fast atomics (LDAPR)
    endif()
  endif()
endif()

if(MINGW)
  add_definitions(-D_WIN32_WINNT=0x600)                # issue #976
endif()

if(MI_OPT_ARCH_FLAGS)
  list(APPEND mi_cflags ${MI_OPT_ARCH_FLAGS})
  message(STATUS "Architecture specific optimization is enabled (with ${MI_OPT_ARCH_FLAGS}) (MI_OPT_ARCH=ON)")
  if (MI_OPT_SIMD)
    list(APPEND mi_defines "MI_OPT_SIMD=1")
    message(STATUS "SIMD instructions are enabled (MI_OPT_SIMD=ON)")
  endif()
elseif(MI_OPT_SIMD)
  message(STATUS "SIMD instructions are not enabled (either MI_OPT_ARCH=OFF or this architecture has no SIMD support)")
endif()

# extra needed libraries

# we prefer -l<lib> test over `find_library` as sometimes core libraries
# like `libatomic` are not on the system path (see issue #898)
function(find_link_library libname outlibname)
  check_linker_flag(C "-l${libname}" mi_has_lib${libname})
  if (mi_has_lib${libname})
    message(VERBOSE "link library: -l${libname}")
    set(${outlibname} ${libname} PARENT_SCOPE)
  else()
    find_library(MI_LIBPATH_${libname} ${libname})
    if (MI_LIBPATH_${libname})
      message(VERBOSE "link library ${libname} at ${MI_LIBPATH_${libname}}")
      set(${outlibname} ${MI_LIBPATH_${libname}} PARENT_SCOPE)
    else()
      message(VERBOSE "link library not found: ${libname}")
      set(${outlibname} "" PARENT_SCOPE)
    endif()
  endif()
endfunction()

if(WIN32)
  list(APPEND mi_libraries psapi shell32 user32 advapi32 bcrypt)
else()
  find_link_library("pthread" MI_LIB_PTHREAD)
  if(MI_LIB_PTHREAD)
    list(APPEND mi_libraries "${MI_LIB_PTHREAD}")
  endif()
  find_link_library("rt" MI_LIB_RT)
  if(MI_LIB_RT)
    list(APPEND mi_libraries "${MI_LIB_RT}")
  endif()
  find_link_library("atomic" MI_LIB_ATOMIC)
  if(MI_LIB_ATOMIC)
    list(APPEND mi_libraries "${MI_LIB_ATOMIC}")
  endif()
endif()

# set language for source files now
if(MI_USE_CXX)
  set_source_files_properties(${mi_sources} PROPERTIES LANGUAGE CXX )
  set_source_files_properties(src/static.c test/test-api.c test/test-api-fill.c test/test-stress.c test/test-stress-heaps.c  test/test-stress-subprocs.c PROPERTIES LANGUAGE CXX )
endif()


# -----------------------------------------------------------------------------
# Install and output names
# -----------------------------------------------------------------------------

# dynamic/shared library and symlinks always go to /usr/local/lib equivalent
# we use ${CMAKE_INSTALL_BINDIR} and ${CMAKE_INSTALL_LIBDIR}.

# static libraries and object files, includes, and cmake config files
# are either installed at top level, or use versioned directories for side-by-side installation (default)
if (MI_INSTALL_TOPLEVEL)
  set(mi_install_objdir     "${CMAKE_INSTALL_LIBDIR}")
  set(mi_install_incdir     "${CMAKE_INSTALL_INCLUDEDIR}")
  set(mi_install_cmakedir   "${CMAKE_INSTALL_LIBDIR}/cmake/mimalloc")
else()
  set(mi_install_objdir     "${CMAKE_INSTALL_LIBDIR}/mimalloc-${mi_version}")       # for static library and object files
  set(mi_install_incdir     "${CMAKE_INSTALL_INCLUDEDIR}/mimalloc-${mi_version}")   # for includes
  set(mi_install_cmakedir   "${CMAKE_INSTALL_LIBDIR}/cmake/mimalloc-${mi_version}") # for cmake package info
endif()

set(mi_libname "mimalloc")
if(MI_SECURE)
  set(mi_libname "${mi_libname}-secure")
endif()
if(MI_TRACK STREQUAL "VALGRIND")
  set(mi_libname "${mi_libname}-valgrind")
endif()
if(MI_TRACK STREQUAL "ASAN")
  set(mi_libname "${mi_libname}-asan")
endif()
string(TOLOWER "${CMAKE_BUILD_TYPE}" CMAKE_BUILD_TYPE_LC)
list(APPEND mi_defines "MI_CMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE_LC}")  #todo: multi-config project needs $<CONFIG> ?
if(CMAKE_BUILD_TYPE_LC MATCHES "^(release|relwithdebinfo|minsizerel|none)$")
  list(APPEND mi_defines MI_BUILD_RELEASE)
else()
  set(mi_libname "${mi_libname}-${CMAKE_BUILD_TYPE_LC}") #append build type (e.g. -debug) if not a release version
endif()
string(REGEX REPLACE "^mimalloc[-]?" "" mi_libsuffix "${mi_libname}")


if(MI_BUILD_SHARED)
  list(APPEND mi_build_targets "shared")
endif()
if(MI_BUILD_STATIC)
  list(APPEND mi_build_targets "static")
endif()
if(MI_BUILD_OBJECT)
  list(APPEND mi_build_targets "object")
endif()
if(MI_BUILD_TESTS)
  list(APPEND mi_build_targets "tests")
endif()

message(STATUS "")
message(STATUS "Library name     : ${mi_libname}")
message(STATUS "Version          : ${mi_version}.${mi_version_patch}")
message(STATUS "Build type       : ${CMAKE_BUILD_TYPE_LC}")
if(MI_USE_CXX)
  message(STATUS "C++ Compiler     : ${CMAKE_CXX_COMPILER}")
else()
  message(STATUS "C Compiler       : ${CMAKE_C_COMPILER}")
endif()
message(STATUS "Compiler flags   : ${mi_cflags}")
message(STATUS "Compiler defines : ${mi_defines}")
message(STATUS "Link libraries   : ${mi_libraries}")
message(STATUS "Build targets    : ${mi_build_targets}")
message(STATUS "")

# -----------------------------------------------------------------------------
# Main targets
# -----------------------------------------------------------------------------

# shared library
if(MI_BUILD_SHARED)
  add_library(mimalloc SHARED ${mi_sources})
  set_target_properties(mimalloc PROPERTIES VERSION ${mi_version} SOVERSION ${mi_version_major} OUTPUT_NAME ${mi_libname} )
  target_compile_definitions(mimalloc PRIVATE ${mi_defines} MI_SHARED_LIB MI_SHARED_LIB_EXPORT)
  target_compile_options(mimalloc PRIVATE ${mi_cflags} ${mi_cflags_dynamic})
  target_link_libraries(mimalloc PRIVATE ${mi_libraries})
  target_include_directories(mimalloc PUBLIC
      $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
      $<INSTALL_INTERFACE:${mi_install_incdir}>
  )
  install(TARGETS mimalloc EXPORT mimalloc ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR} RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR} LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR})
  install(EXPORT mimalloc DESTINATION ${mi_install_cmakedir})

  if(WIN32 AND NOT MINGW)
    # On windows, the import library name for the dll would clash with the static mimalloc.lib library
    # so we postfix the dll import library with `.dll.lib` (and also the .pdb debug file)
    set_property(TARGET mimalloc PROPERTY ARCHIVE_OUTPUT_NAME "${mi_libname}.dll" )
    install(FILES "$<TARGET_FILE_DIR:mimalloc>/${mi_libname}.dll.lib" DESTINATION ${CMAKE_INSTALL_LIBDIR}/mimalloc-${mi_version})
    set_property(TARGET mimalloc PROPERTY PDB_NAME "${mi_libname}.dll")
    # don't try to install the pdb since it may not be generated depending on the configuration
    # install(FILES "$<TARGET_FILE_DIR:mimalloc>/${mi_libname}.dll.pdb" DESTINATION ${CMAKE_INSTALL_LIBDIR})
  endif()
  if(WIN32 AND MI_WIN_REDIRECT)
    if(MINGW)
      set_property(TARGET mimalloc PROPERTY PREFIX "")
    endif()
    # On windows, link and copy the mimalloc redirection dll too.
    if(CMAKE_GENERATOR_PLATFORM STREQUAL "arm64ec")
      set(MIMALLOC_REDIRECT_SUFFIX "-arm64ec")
      set(MINJECT_SUFFIX "")
    elseif(MI_ARCH STREQUAL "x64")
      set(MIMALLOC_REDIRECT_SUFFIX "")
      set(MINJECT_SUFFIX "")
      if(CMAKE_SYSTEM_PROCESSOR STREQUAL "ARM64")
        message(STATUS "Note: x64 code emulated on Windows for arm64 should use an arm64ec build of 'mimalloc.dll'")
        message(STATUS "      together with 'mimalloc-redirect-arm64ec.dll'. See the 'bin\\readme.md' for more information.")
      endif()
    elseif(MI_ARCH STREQUAL "x86")
      set(MIMALLOC_REDIRECT_SUFFIX "32")
      set(MINJECT_SUFFIX "32")
    else()
      set(MIMALLOC_REDIRECT_SUFFIX "-${MI_ARCH}")  # -arm64 etc.
      set(MINJECT_SUFFIX "-${MI_ARCH}")
    endif()

    target_link_libraries(mimalloc PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/bin/mimalloc-redirect${MIMALLOC_REDIRECT_SUFFIX}.lib)  # the DLL import library
    add_custom_command(TARGET mimalloc POST_BUILD
      COMMAND "${CMAKE_COMMAND}" -E copy "${CMAKE_CURRENT_SOURCE_DIR}/bin/mimalloc-redirect${MIMALLOC_REDIRECT_SUFFIX}.dll" $<TARGET_FILE_DIR:mimalloc>
      COMMENT "Copy mimalloc-redirect${MIMALLOC_REDIRECT_SUFFIX}.dll to output directory")
    install(FILES "$<TARGET_FILE_DIR:mimalloc>/mimalloc-redirect${MIMALLOC_REDIRECT_SUFFIX}.dll" DESTINATION ${CMAKE_INSTALL_BINDIR})
  endif()
endif()


# static library
if (MI_BUILD_STATIC)
  add_library(mimalloc-static STATIC ${mi_sources})
  set_property(TARGET mimalloc-static PROPERTY OUTPUT_NAME ${mi_libname})
  set_property(TARGET mimalloc-static PROPERTY POSITION_INDEPENDENT_CODE ON)
  target_compile_definitions(mimalloc-static PRIVATE ${mi_defines} MI_STATIC_LIB)
  target_compile_options(mimalloc-static PRIVATE ${mi_cflags} ${mi_cflags_static})
  target_link_libraries(mimalloc-static PRIVATE ${mi_libraries})
  target_include_directories(mimalloc-static PUBLIC
      $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
      $<INSTALL_INTERFACE:${mi_install_incdir}>
  )
  install(TARGETS mimalloc-static EXPORT mimalloc DESTINATION ${mi_install_objdir} LIBRARY)
  install(EXPORT mimalloc DESTINATION ${mi_install_cmakedir})
endif()

# install include files
install(FILES include/mimalloc.h DESTINATION ${mi_install_incdir})
install(FILES include/mimalloc-override.h DESTINATION ${mi_install_incdir})
install(FILES include/mimalloc-new-delete.h DESTINATION ${mi_install_incdir})
install(FILES include/mimalloc-stats.h DESTINATION ${mi_install_incdir})
install(FILES cmake/mimalloc-config.cmake DESTINATION ${mi_install_cmakedir})
install(FILES cmake/mimalloc-config-version.cmake DESTINATION ${mi_install_cmakedir})


# single object file for more predictable static overriding
if (MI_BUILD_OBJECT)
  add_library(mimalloc-obj OBJECT src/static.c)
  set_property(TARGET mimalloc-obj PROPERTY POSITION_INDEPENDENT_CODE ON)
  # set_property(TARGET mimalloc-obj APPEND PROPERTY OBJECT_DEPENDS ${mi_sources})
  target_compile_definitions(mimalloc-obj PRIVATE ${mi_defines})
  target_compile_options(mimalloc-obj PRIVATE ${mi_cflags} ${mi_cflags_static})
  target_include_directories(mimalloc-obj PUBLIC
      $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
      $<INSTALL_INTERFACE:${mi_install_incdir}>
  )

  # Copy the generated object file (`static.o`) to the output directory (as `mimalloc.o`)
  if(CMAKE_GENERATOR MATCHES "^Visual Studio.*$")
    set(mimalloc-obj-static "${CMAKE_CURRENT_BINARY_DIR}/mimalloc-obj.dir/$<CONFIG>/static${CMAKE_C_OUTPUT_EXTENSION}")
  else()
    set(mimalloc-obj-static "${CMAKE_CURRENT_BINARY_DIR}/CMakeFiles/mimalloc-obj.dir/src/static.c${CMAKE_C_OUTPUT_EXTENSION}")
  endif()
  set(mimalloc-obj-out "${CMAKE_CURRENT_BINARY_DIR}/${mi_libname}${CMAKE_C_OUTPUT_EXTENSION}")
  add_custom_command(OUTPUT ${mimalloc-obj-out} DEPENDS mimalloc-obj mimalloc-static COMMAND "${CMAKE_COMMAND}" -E copy "${mimalloc-obj-static}" "${mimalloc-obj-out}")
  add_custom_target(mimalloc-obj-target ALL DEPENDS ${mimalloc-obj-out})


  # the following seems to lead to cmake warnings/errors on some systems, disable for now :-(
  # install(TARGETS mimalloc-obj EXPORT mimalloc DESTINATION ${mi_install_objdir})

  # the FILES expression can also be: $<TARGET_OBJECTS:mimalloc-obj>
  # but that fails cmake versions less than 3.10 so we leave it as is for now
  install(FILES ${mimalloc-obj-static}
          DESTINATION ${mi_install_objdir}
          RENAME ${mi_libname}${CMAKE_C_OUTPUT_EXTENSION} )
endif()


# pkg-config file support
set(mi_pc_libraries "")
foreach(item IN LISTS mi_libraries)
  if(item MATCHES " *[-].*")
    set(mi_pc_libraries "${mi_pc_libraries} ${item}")
  else()
    set(mi_pc_libraries "${mi_pc_libraries} -l${item}")
  endif()
endforeach()

include("cmake/JoinPaths.cmake")
join_paths(mi_pc_includedir "\${prefix}" "${CMAKE_INSTALL_INCLUDEDIR}")
join_paths(mi_pc_libdir "\${prefix}" "${CMAKE_INSTALL_LIBDIR}")

configure_file(mimalloc.pc.in mimalloc.pc @ONLY)
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/mimalloc.pc"
        DESTINATION "${CMAKE_INSTALL_LIBDIR}/pkgconfig/")



# -----------------------------------------------------------------------------
# API surface testing
# -----------------------------------------------------------------------------

if (MI_BUILD_TESTS)
  enable_testing()

  # static link tests
  foreach(TEST_NAME api api-fill stress-heaps stress-subprocs stress)
    add_executable(mimalloc-test-${TEST_NAME} test/test-${TEST_NAME}.c)
    target_compile_definitions(mimalloc-test-${TEST_NAME} PRIVATE ${mi_defines})
    target_compile_options(mimalloc-test-${TEST_NAME} PRIVATE ${mi_cflags})
    target_include_directories(mimalloc-test-${TEST_NAME} PRIVATE include)
    if(MI_BUILD_STATIC AND NOT MI_DEBUG_TSAN)
      target_link_libraries(mimalloc-test-${TEST_NAME} PRIVATE mimalloc-static ${mi_libraries})
    elseif(MI_BUILD_SHARED)
      target_link_libraries(mimalloc-test-${TEST_NAME} PRIVATE mimalloc ${mi_libraries})
    else()
      message(STATUS "cannot build TSAN tests without MI_BUILD_SHARED being enabled")
    endif()
    if(MI_GUARDED)
    add_test(NAME test-${TEST_NAME} COMMAND ${CMAKE_COMMAND} -E env MIMALLOC_GUARDED_SAMPLE_RATE=1 $<TARGET_FILE:mimalloc-test-${TEST_NAME}>)
    elseif(TEST_NAME STREQUAL "stress-heaps")
    add_test(NAME test-${TEST_NAME} COMMAND ${CMAKE_COMMAND} -E env MIMALLOC_ARENA_EAGER_COMMIT=0 MIMALLOC_PAGE_COMMIT_ON_DEMAND=0 $<TARGET_FILE:mimalloc-test-${TEST_NAME}>)
    else()
    add_test(NAME test-${TEST_NAME} COMMAND mimalloc-test-${TEST_NAME})
    endif()
  endforeach()

  # static override test
  if(MI_BUILD_OBJECT AND NOT (MI_CYGWIN OR (MI_TRACK STREQUAL "ASAN") OR MI_DEBUG_TSAN OR MI_DEBUG_UBSAN))
    add_executable(mimalloc-test-stress-static test/test-stress.c)
    target_compile_definitions(mimalloc-test-stress-static PRIVATE ${mi_defines} "USE_STD_MALLOC=1")
    target_compile_options(mimalloc-test-stress-static PRIVATE ${mi_cflags})
    target_include_directories(mimalloc-test-stress-static PRIVATE include)
    target_link_libraries(mimalloc-test-stress-static PRIVATE mimalloc-obj  ${mi_libraries})  # override statically with an object file
    add_test(NAME test-stress-static COMMAND ${CMAKE_COMMAND} -E env MIMALLOC_VERBOSE=1 MIMALLOC_SHOW_STATS=1 $<TARGET_FILE:mimalloc-test-stress-static>)
  endif()

  # dynamic override test
  if(MI_BUILD_SHARED AND NOT (MI_CYGWIN OR (MI_TRACK STREQUAL "ASAN") OR MI_DEBUG_TSAN OR MI_DEBUG_UBSAN)) # AND NOT (APPLE AND MI_USE_CXX))
    add_executable(mimalloc-test-stress-dynamic test/test-stress.c)
    target_compile_definitions(mimalloc-test-stress-dynamic PRIVATE ${mi_defines} "USE_STD_MALLOC=1")
    target_compile_options(mimalloc-test-stress-dynamic PRIVATE ${mi_cflags})
    target_include_directories(mimalloc-test-stress-dynamic PRIVATE include)
    if(WIN32)
      target_compile_definitions(mimalloc-test-stress-dynamic PRIVATE "MI_LINK_VERSION=1")  # link mi_version
      target_link_libraries(mimalloc-test-stress-dynamic PRIVATE mimalloc ${mi_libraries})  # link mi_version
      if(MI_MINGW_UCRT64)
        # mingw always links mimalloc after system libraries.
        # post-process with `minject` to make sure mimalloc is the first dll in the load order.
        add_custom_command(TARGET mimalloc-test-stress-dynamic POST_BUILD COMMAND ${CMAKE_CURRENT_SOURCE_DIR}/bin/minject${MINJECT_SUFFIX} --postfix=${mi_libsuffix} -f -i $<TARGET_FILE:mimalloc-test-stress-dynamic>)
      endif()
      add_test(NAME test-stress-dynamic COMMAND ${CMAKE_COMMAND} -E env MIMALLOC_VERBOSE=1 $<TARGET_FILE:mimalloc-test-stress-dynamic>)
    else()
      target_link_libraries(mimalloc-test-stress-dynamic PRIVATE ${mi_libraries}) # pthreads, issue 1158
      if(APPLE)
        set(LD_PRELOAD "DYLD_INSERT_LIBRARIES")
      else()
        set(LD_PRELOAD "LD_PRELOAD")
      endif()
      add_test(NAME test-stress-dynamic COMMAND ${CMAKE_COMMAND} -E env MIMALLOC_VERBOSE=1 ${LD_PRELOAD}=$<TARGET_FILE:mimalloc> $<TARGET_FILE:mimalloc-test-stress-dynamic>)
    endif()
  endif()
endif()

# -----------------------------------------------------------------------------
# Set override properties
# -----------------------------------------------------------------------------
if (MI_OVERRIDE)
  if (MI_BUILD_SHARED)
    target_compile_definitions(mimalloc PRIVATE MI_MALLOC_OVERRIDE)
  endif()
  if (MI_BUILD_STATIC)
    target_compile_definitions(mimalloc-static PRIVATE MI_MALLOC_OVERRIDE)
  endif()
  if (MI_BUILD_OBJECT)
    target_compile_definitions(mimalloc-obj PRIVATE MI_MALLOC_OVERRIDE)
  endif()
endif()
