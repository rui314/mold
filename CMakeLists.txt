# You can customize a build by specifying CMake options. An option may be
# given in the -Dvariable=value form. For a boolean variable, `ON` or `1`
# means true while `OFF` or `0` means false.
#
# Here are a couple of common cmake options:
#
#  -DCMAKE_C_COMPILER=<command-name>
#
#   Specifies a C compiler name to use. The default value is `cc`.
#
#  -DCMAKE_CXX_COMPILER=<command-name>
#
#   Specifies a C++ compiler name to use. The default value is `c++`.
#
#  -DCMAKE_INSTALL_PREFIX=<directory>
#
#   Specifies an install target directory. The default value is `/usr/local`.
#
#  -DCMAKE_BUILD_TYPE=[Debug | Release | RelWithDebInfo | MinSizeRel]
#
#   Specifies a build type. The default is `Release` which is the right
#   option unless you are debugging mold.
#
# An example of a cmake command line is shown below:
#
#   $ cmake -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_CXX_COMPILER=g++-12 ..
#
# where `..` refers this directory.
#
# With cmake, you may run `cmake --install .` instead of `make install` to
# install build artifacts to system directories. If you want to install
# artifacts to a temporary target directory, run `cmake --install . --prefix
# <dir>`.
#
# You can see the current cmake variables and their values by running
# `cmake -N -L .` in a build directory.
#
# Note that in this file, we provide various dials and knobs to configure
# how to build mold. However, as a policy, we do not provide a way to
# enable/disable any individual mold's feature. In other words, we do not
# provide options like `--enable-foo` or `--disable-foo`. The motivation
# behind it is build reproducibility. We want to guarantees that all builds
# of the mold linker of the same version will have the exactly same set of
# features and behave exactly the same.

cmake_minimum_required(VERSION 3.13)
project(mold VERSION 1.7.0)

include(CMakeDependentOption)
include(GNUInstallDirs)

# Build mold itself using mold if -DMOLD_USE_MOLD=ON
option(MOLD_USE_MOLD "Use mold to build mold" OFF)
if(MOLD_USE_MOLD)
  add_link_options(-fuse-ld=mold)
endif()

add_executable(mold)
target_compile_features(mold PRIVATE cxx_std_20)
target_link_libraries(mold PRIVATE ${CMAKE_DL_LIBS})

if(NOT "${CMAKE_CXX_COMPILER_FRONTEND_VARIANT}" STREQUAL "MSVC")
  target_compile_options(mold PRIVATE
    -fno-exceptions
    -fno-unwind-tables
    -fno-asynchronous-unwind-tables
    -Wno-sign-compare
    -Wno-unused-function)
endif()

# Build mold with -flto if -DMOLD_LTO=ON
option(MOLD_LTO "Build mold with link-time optimization enabled")
if(MOLD_LTO)
  set_property(TARGET mold PROPERTY INTERPROCEDURAL_OPTIMIZATION ON)
endif()

# Enable AddressSanitizer if -DMOLD_USE_ASAN=ON
option(MOLD_USE_ASAN "Build mold with AddressSanitizer" OFF)
if(MOLD_USE_ASAN)
  target_compile_options(mold PRIVATE -fsanitize=address -fsanitize=undefined)
  target_link_options(mold PRIVATE -fsanitize=address -fsanitize=undefined)
endif()

# Enabled ThreadSanitizer if -DMOLD_USE_TSAN=ON
option(MOLD_USE_TSAN "Build mold with ThreadSanitizer" OFF)
if(MOLD_USE_TSAN)
  target_compile_options(mold PRIVATE -fsanitize=thread)
  target_link_options(mold PRIVATE -fsanitize=thread)
endif()

# Statically-link libstdc++ and libcrypto if -DMOLD_MOSTLY_STATIC=ON.
#
# This option is intended to be used by `./dist.sh` script to create a
# mold binary that works on various Linux distros. You probably don't
# need nor want to set this to ON.
option(MOLD_MOSTLY_STATIC "Statically link libstdc++ and libcrypto" OFF)
if(MOLD_MOSTLY_STATIC)
  target_link_options(mold PRIVATE -static-libstdc++)
  target_link_libraries(mold PRIVATE libcrypto.a)
endif()

# Find zlib. If libz.so is not found, we compile a bundled one and
# statically-link it to mold.
find_package(ZLIB QUIET)
if(ZLIB_FOUND)
  target_link_libraries(mold PRIVATE ZLIB::ZLIB)
else()
  add_subdirectory(third-party/zlib EXCLUDE_FROM_ALL)
  target_include_directories(zlibstatic INTERFACE third-party/zlib
    $<TARGET_PROPERTY:zlibstatic,BINARY_DIR>)
  target_link_libraries(mold PRIVATE zlibstatic)
endif()

# Find zstd compression library. Just like zlib, if libzstd.so is not
# found, we compile a bundled one and statically-link it to mold.
include(CheckIncludeFile)
check_include_file(zstd.h HAVE_ZSTD_H)

if(HAVE_ZSTD_H)
  target_link_libraries(mold PRIVATE zstd)
else()
  add_subdirectory(third-party/zstd/build/cmake EXCLUDE_FROM_ALL)
  target_compile_definitions(libzstd_static PRIVATE
    ZSTD_BUILD_STATIC=1
    ZSTD_BUILD_SHARED=0
    ZSTD_BUILD_PROGRAMS=0
    ZSTD_MULTITHREAD_SUPPORT=0
    ZSTD_BUILD_TESTS=0)
  target_include_directories(mold PUBLIC third-party/zstd/lib)
  target_link_libraries(mold PRIVATE libzstd_static)
endif()

# Find mimalloc. mimalloc is an alternative malloc implementation
# optimized for multi-threaded applications.
#
# If you want to use the usual libc's malloc, pass -DMOLD_USE_MIMALLOC=OFF.
#
# We enable mimalloc by default for 64-bit targets. It doesn't seem to
# be stable on 32-bit targets.
include(CheckCXXSourceCompiles)
check_cxx_source_compiles(
  "static_assert(sizeof(void *) == 4); int main() {}" P32)

cmake_dependent_option(MOLD_USE_MIMALLOC "Use mimalloc" ON
  "NOT APPLE; NOT ANDROID; NOT P32" OFF)
cmake_dependent_option(
  MOLD_USE_SYSTEM_MIMALLOC "Use system or vendored mimalloc" OFF
  MOLD_USE_MIMALLOC OFF)

# By default, we build a bundled mimalloc and statically-link it to
# mold. If you want to dynamically link to the system's
# libmimalloc.so, pass -DMOLD_USE_SYSTEM_MIMALLOC=ON.
if(MOLD_USE_MIMALLOC)
  if(MOLD_USE_SYSTEM_MIMALLOC)
    find_package(mimalloc REQUIRED)
    target_link_libraries(mold PRIVATE mimalloc)
    target_compile_definitions(mold PRIVATE USE_SYSTEM_MIMALLOC)
  else()
    function(mold_add_mimalloc)
      set(MI_BUILD_STATIC ON)
      option(MI_BUILD_TESTS "Build test executables" OFF)
      add_subdirectory(third-party/mimalloc EXCLUDE_FROM_ALL)
      target_compile_definitions(mimalloc-static PRIVATE MI_USE_ENVIRON=0)
      target_link_libraries(mold PRIVATE mimalloc-static)
    endfunction()

    mold_add_mimalloc()
  endif()
endif()

# Find TBB. TBB (OneTBB or Intel TBB) is a high-level threading library.
# Use of this library is mandatory.
#
# By default, we build a bundled one and statically-link the library
# to mold. If you want to link to the system's libtbb2.so, pass
# -DMOLD_USE_SYSTEM_TBB=ON.
option(MOLD_USE_SYSTEM_TBB "Use system or vendored TBB" OFF)
if(MOLD_USE_SYSTEM_TBB)
  find_package(TBB REQUIRED)
  target_link_libraries(mold PRIVATE TBB::tbb)
else()
  function(mold_add_tbb)
    set(BUILD_SHARED_LIBS OFF)
    set(TBB_TEST OFF CACHE INTERNAL "")
    set(TBB_STRICT OFF CACHE INTERNAL "")
    add_subdirectory(third-party/tbb EXCLUDE_FROM_ALL)
    target_compile_definitions(tbb PRIVATE __TBB_DYNAMIC_LOAD_ENABLED=0)
    target_link_libraries(mold PRIVATE TBB::tbb)
  endfunction()

  mold_add_tbb()
endif()

# We always use Clang to build mold on Windows. MSVC can't compile mold.
if(WIN32)
  if(NOT CMAKE_CXX_COMPILER_ID STREQUAL "Clang")
    message(FATAL_ERROR
      "Your compiler is not supported; install Clang from Visual Studio Installer and re-run cmake with '-T clangcl'")
  endif()

  target_compile_definitions(mold PRIVATE NOGDI NOMINMAX)
else()
  include(CheckLibraryExists)
  check_library_exists(m pow "" LIBM_FOUND)
  if(LIBM_FOUND)
    target_link_libraries(mold PRIVATE m)
  endif()
endif()

# Build mold-wrapper.so
if(NOT APPLE AND NOT WIN32)
  add_library(mold-wrapper SHARED)
  install(TARGETS mold-wrapper DESTINATION ${CMAKE_INSTALL_LIBDIR}/mold)

  # Remove the default `lib` prefix
  set_target_properties(mold-wrapper PROPERTIES PREFIX "")
  target_link_libraries(mold-wrapper PRIVATE ${CMAKE_DL_LIBS})
  target_sources(mold-wrapper PRIVATE elf/mold-wrapper.c)
endif()

if(NOT APPLE AND NOT WIN32 AND NOT MOLD_MOSTLY_STATIC)
  find_package(OpenSSL REQUIRED COMPONENTS Crypto)
  target_link_libraries(mold PRIVATE OpenSSL::Crypto)
endif()

# If atomics doesn't work by default, add -latomic.
# We need the flag on riscv, armv6 and m68k.
include(CheckCXXSourceCompiles)
check_cxx_source_compiles("#include <cstdint>
int main() {
  uint64_t x = 1;
  __atomic_add_fetch(&x, 0, __ATOMIC_RELAXED);
  return x;
}" HAVE_ATOMIC_ADD_FETCH)

if(NOT HAVE_ATOMIC_ADD_FETCH)
  target_link_libraries(mold PRIVATE atomic)
endif()

# Create a .cc file containing the current git hash for `mold --version`.
add_custom_target(git_hash
  COMMAND ${CMAKE_COMMAND}
    -DSOURCE_DIR=${CMAKE_SOURCE_DIR}
    -DOUTPUT_FILE=${CMAKE_BINARY_DIR}/git-hash.cc
    -P ${CMAKE_SOURCE_DIR}/update-git-hash.cmake
  DEPENDS update-git-hash.cmake
  BYPRODUCTS git-hash.cc
  VERBATIM)

add_dependencies(mold git_hash)

# Create config.h file
configure_file(config.h.in config.h)
include_directories(${CMAKE_CURRENT_BINARY_DIR})

# Almost all functions are template in mold which take a target type
# (e.g. X86_64) as its type parameter. Since we suport more than 10
# targets, compiling a single source file for all the targets is very
# slow.
#
# As a workaround, we create a .cc file for each target and spawn many
# compiler instances. This is hacky but greatly reduces compile time
# on a multicore machine.
list(APPEND MOLD_ELF_TARGETS
  X86_64 I386 ARM64 ARM32 RV32LE RV32BE RV64LE RV64BE
  PPC64V1 PPC64V2 S390X SPARC64 M68K)

list(APPEND MOLD_ELF_TEMPLATE_FILES
  elf/cmdline.cc
  elf/dwarf.cc
  elf/gc-sections.cc
  elf/icf.cc
  elf/input-files.cc
  elf/input-sections.cc
  elf/linker-script.cc
  elf/lto.cc
  elf/main.cc
  elf/mapfile.cc
  elf/output-chunks.cc
  elf/passes.cc
  elf/relocatable.cc
  elf/subprocess.cc
  elf/thunks.cc
  )

list(APPEND MOLD_MACHO_TARGETS X86_64 ARM64)

list(APPEND MOLD_MACHO_TEMPLATE_FILES
  macho/cmdline.cc
  macho/dead-strip.cc
  macho/input-files.cc
  macho/input-sections.cc
  macho/lto.cc
  macho/main.cc
  macho/mapfile.cc
  macho/output-chunks.cc
  macho/tapi.cc
  )

function(mold_instantiate_templates SOURCE TARGET)
  set(PATH ${CMAKE_BINARY_DIR}/${SOURCE}.${TARGET}.cc)
  file(WRITE ${PATH} "#define MOLD_${TARGET} 1
#define MOLD_TARGET ${TARGET}
#include \"${CMAKE_SOURCE_DIR}/${SOURCE}\"
")
  target_sources(mold PRIVATE ${PATH})
endfunction()

foreach (SOURCE IN LISTS MOLD_ELF_TEMPLATE_FILES)
  foreach(TARGET IN LISTS MOLD_ELF_TARGETS)
    mold_instantiate_templates(${SOURCE} ${TARGET})
  endforeach()
endforeach()

foreach (SOURCE IN LISTS MOLD_MACHO_TEMPLATE_FILES)
  foreach(TARGET IN LISTS MOLD_MACHO_TARGETS)
    mold_instantiate_templates(${SOURCE} ${TARGET})
  endforeach()
endforeach()

# Add other non-template source files.
target_sources(mold PRIVATE
  compress.cc
  demangle.cc
  elf/arch-arm32.cc
  elf/arch-arm64.cc
  elf/arch-i386.cc
  elf/arch-m68k.cc
  elf/arch-ppc64v1.cc
  elf/arch-ppc64v2.cc
  elf/arch-riscv.cc
  elf/arch-s390x.cc
  elf/arch-sparc64.cc
  elf/arch-x86-64.cc
  filepath.cc
  git-hash.cc
  glob.cc
  hyperloglog.cc
  macho/arch-arm64.cc
  macho/arch-x86-64.cc
  macho/yaml.cc
  main.cc
  multi-glob.cc
  perf.cc
  tar.cc
  third-party/rust-demangle/rust-demangle.c
  uuid.cc
  )

# Add frequently included header files for pre-compiling.
# target_precompile_headers is supported by CMake 3.16.0 or newer.
if(${CMAKE_VERSION} VERSION_GREATER_EQUAL "3.16.0")
  target_precompile_headers(mold PRIVATE
    "$<$<COMPILE_LANGUAGE:CXX>:${CMAKE_SOURCE_DIR}/elf/mold.h>"
    "$<$<COMPILE_LANGUAGE:CXX>:${CMAKE_SOURCE_DIR}/macho/mold.h>")

  # ccache needs this flag along with `sloppiness = pch_defines,time_macros`
  # to enable caching
  target_compile_options(mold PRIVATE -fpch-preprocess)
endif()

include(CTest)

if(BUILD_TESTING)
  # Create the ld and ld64 symlinks required for testing
  if(NOT WIN32)
    add_custom_command(
      TARGET mold POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E create_symlink mold ld
      COMMAND ${CMAKE_COMMAND} -E create_symlink mold ld64
      BYPRODUCTS ld ld64
      WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
      VERBATIM)
  endif()

  if(${APPLE})
    add_subdirectory(test/macho)
  elseif(${UNIX})
    add_subdirectory(test/elf)
  endif()
endif()

if(NOT CMAKE_SKIP_INSTALL_RULES)
  install(TARGETS mold RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
  install(FILES LICENSE DESTINATION ${CMAKE_INSTALL_DOCDIR})
  install(FILES docs/mold.1 DESTINATION ${CMAKE_INSTALL_MANDIR}/man1/)

  function(mold_install_relative_symlink OLD NEW)
    install(CODE "
      get_filename_component(PREFIX_ABS \${CMAKE_INSTALL_PREFIX} ABSOLUTE)
      get_filename_component(OLD_ABS ${OLD} ABSOLUTE BASE_DIR \${PREFIX_ABS})
      get_filename_component(NEW_ABS ${NEW} ABSOLUTE BASE_DIR \${PREFIX_ABS})
      get_filename_component(NEW_DIR \${NEW_ABS} DIRECTORY)
      file(RELATIVE_PATH OLD_REL \${NEW_DIR} \${OLD_ABS})
      message(STATUS \"Installing symlink: \$ENV{DESTDIR}/\${NEW_ABS} -> \${OLD_REL}\")
      file(MAKE_DIRECTORY \$ENV{DESTDIR}/\${NEW_DIR})
      file(CREATE_LINK \${OLD_REL} \$ENV{DESTDIR}/\${NEW_ABS} SYMBOLIC)")
  endfunction()
  mold_install_relative_symlink(${CMAKE_INSTALL_BINDIR}/mold
    ${CMAKE_INSTALL_LIBEXECDIR}/mold/ld)
  mold_install_relative_symlink(${CMAKE_INSTALL_BINDIR}/mold
    ${CMAKE_INSTALL_BINDIR}/ld.mold)
  mold_install_relative_symlink(${CMAKE_INSTALL_BINDIR}/mold
    ${CMAKE_INSTALL_BINDIR}/ld64.mold)
  mold_install_relative_symlink(${CMAKE_INSTALL_MANDIR}/man1/mold.1
    ${CMAKE_INSTALL_MANDIR}/man1/ld.mold.1)
endif()
