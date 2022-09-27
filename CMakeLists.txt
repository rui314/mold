cmake_minimum_required(VERSION 3.13)
project(mold VERSION 1.5.0)

include(CMakeDependentOption)
include(GNUInstallDirs)

# Add -fuse-ld=mold if accepted by the compiler.
option(MOLD_USE_MOLD "Use mold to build mold" OFF)
if(MOLD_USE_MOLD)
  add_link_options(-fuse-ld=mold)
endif()

add_executable(mold)
target_compile_features(mold PRIVATE cxx_std_20)
target_compile_definitions(mold PRIVATE
  "LIBDIR=\"${CMAKE_INSTALL_FULL_LIBDIR}\"")
target_link_libraries(mold PRIVATE ${CMAKE_DL_LIBS})

if(NOT "${CMAKE_CXX_COMPILER_FRONTEND_VARIANT}" STREQUAL "MSVC")
  target_compile_options(mold PRIVATE
    -fno-exceptions
    -fno-unwind-tables
    -fno-asynchronous-unwind-tables
    -Wno-sign-compare
    -Wno-unused-function)
endif()

# Build mold with -flto if MOLD_LTO=On
option(MOLD_LTO "Build mold with link-time optimization enabled")
if(MOLD_LTO)
  set_property(TARGET mold PROPERTY INTERPROCEDURAL_OPTIMIZATION ON)
endif()

# Handle MOLD_USE_ASAN and MOLD_USE_TSAN
option(MOLD_USE_ASAN "Build mold with AddressSanitizer" OFF)
if(MOLD_USE_ASAN)
  target_compile_options(mold PRIVATE -fsanitize=address -fsanitize=undefined)
  target_link_options(mold PRIVATE -fsanitize=address -fsanitize=undefined)
endif()

option(MOLD_USE_TSAN "Build mold with ThreadSanitizer" OFF)
if(MOLD_USE_TSAN)
  target_compile_options(mold PRIVATE -fsanitize=thread)
  target_link_options(mold PRIVATE -fsanitize=thread)
endif()

# Static link libstdc++ and libcrypto if MOLD_MOSTLY_STATIC=On
option(MOLD_MOSTLY_STATIC "Statically link libstdc++ and libcrypto" OFF)
if(MOLD_MOSTLY_STATIC)
  target_link_options(mold PRIVATE -static-libstdc++)
  target_link_libraries(mold PRIVATE libcrypto.a)
endif()

# Setup zlib
find_package(ZLIB QUIET)
if(ZLIB_FOUND)
  target_link_libraries(mold PRIVATE ZLIB::ZLIB)
else()
  add_subdirectory(third-party/zlib EXCLUDE_FROM_ALL)
  target_include_directories(zlibstatic INTERFACE third-party/zlib
    $<TARGET_PROPERTY:zlibstatic,BINARY_DIR>)
  target_link_libraries(mold PRIVATE zlibstatic)
endif()

# Setup zstd
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

# Setup mimalloc
include(CheckCSourceCompiles)
check_c_source_compiles("#ifdef __i386__\nint main() {}\n#endif" I386)

cmake_dependent_option(MOLD_USE_MIMALLOC "Use mimalloc" ON
  "NOT APPLE; NOT ANDROID; NOT I386" OFF)
cmake_dependent_option(
  MOLD_USE_SYSTEM_MIMALLOC "Use system or vendored mimalloc" OFF
  MOLD_USE_MIMALLOC OFF)

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

# Setup TBB
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

if(CMAKE_SYSTEM_PROCESSOR MATCHES "^(riscv64|armv)")
  target_link_libraries(mold PRIVATE atomic)
endif()

set_property(SOURCE main.cc APPEND PROPERTY
  COMPILE_DEFINITIONS "MOLD_VERSION=\"${CMAKE_PROJECT_VERSION}\"")

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

# Almost all functions are template in mold which take a target type
# (e.g. X86_64) as its type parameter. Since we suport more than 10
# targets, compiling a single source file for all the targets is very
# slow.
#
# As a workaround, we create a .cc file for each target and spawn many
# compiler instances. This is hacky but greatly reduces compile time.
list(APPEND MOLD_ELF_TARGETS
  X86_64 I386 ARM64 ARM32 RV32LE RV32BE RV64LE RV64BE PPC64V2 SPARC64)

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
  elf/arch-ppc64v2.cc
  elf/arch-riscv.cc
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
  strerror.cc
  tar.cc
  third-party/rust-demangle/rust-demangle.c
  uuid.cc
  )

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
  install(TARGETS mold RUNTIME DESTINATION bin)
  install(FILES LICENSE DESTINATION ${CMAKE_INSTALL_DOCDIR})
  install(FILES docs/mold.1 DESTINATION ${CMAKE_INSTALL_MANDIR}/man1/)
  install(CODE "
    set(DEST \"\$ENV{DESTDIR}\")
    file(RELATIVE_PATH RELPATH
       /${CMAKE_INSTALL_FULL_LIBEXECDIR}/mold /${CMAKE_INSTALL_FULL_BINDIR}/mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E make_directory
      \${DEST}/${CMAKE_INSTALL_FULL_LIBEXECDIR}/mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink \${RELPATH}
      \${DEST}/${CMAKE_INSTALL_FULL_LIBEXECDIR}/mold/ld)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold
      \${DEST}/${CMAKE_INSTALL_FULL_BINDIR}/ld.mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold
      \${DEST}/${CMAKE_INSTALL_FULL_BINDIR}/ld64.mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold.1
      \${DEST}/${CMAKE_INSTALL_FULL_MANDIR}/man1/ld.mold.1)")
endif()
