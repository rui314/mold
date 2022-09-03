cmake_minimum_required(VERSION 3.18)
project(mold VERSION 1.4.1)

include(CMakeDependentOption)
include(GNUInstallDirs)

# Add -fuse-ld=mold if accepted by the compiler.
option(MOLD_USE_MOLD "Use mold to build mold" ON)
if(MOLD_USE_MOLD)
  include(CheckLinkerFlag)
  check_linker_flag(CXX -fuse-ld=mold CXX_SUPPORTS_FUSE_MOLD)
  if(CXX_SUPPORTS_FUSE_MOLD)
    add_link_options(-fuse-ld=mold)
  endif()
endif()

add_executable(mold)
target_compile_features(mold PRIVATE cxx_std_20)
target_link_libraries(mold PRIVATE ${CMAKE_DL_LIBS})

if(NOT "${CMAKE_CXX_COMPILER_FRONTEND_VARIANT}" STREQUAL "MSVC")
  target_compile_options(mold PRIVATE
    -fno-exceptions
    -fno-unwind-tables
    -fno-asynchronous-unwind-tables)
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

# Setup mimalloc
cmake_dependent_option(MOLD_USE_MIMALLOC "Use mimalloc" ON
  "NOT APPLE;NOT ANDROID" OFF)
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

# MOLD_X86_64_ONLY is a developer-only option. You should not use it
# for creating an executable for production use.
option(MOLD_X86_64_ONLY "Developer-only option" OFF)
if(MOLD_X86_64_ONLY)
  set(CMAKE_BUILD_TYPE "Debug")
  target_compile_options(mold PRIVATE -ffunction-sections -fdata-sections
    -DMOLD_DEBUG_X86_64_ONLY)
  target_link_options(mold PRIVATE -Wl,--gc-sections)
endif()

if(WIN32)
  if(NOT CMAKE_CXX_COMPILER_ID STREQUAL "Clang")
    message(FATAL_ERROR
      "Your compiler is not supported; install Clang from Visual Studio Installer and re-run cmake with '-T clangcl'")
  endif()

  target_compile_definitions(mold PRIVATE NOGDI NOMINMAX)

  target_sources(mold PRIVATE
    elf/lto-win32.cc
    macho/lto-win32.cc)
else()
  include(CheckLibraryExists)
  check_library_exists(m pow "" LIBM_FOUND)
  if(LIBM_FOUND)
    target_link_libraries(mold PRIVATE m)
  endif()

  target_sources(mold PRIVATE
    elf/lto-unix.cc
    elf/subprocess.cc
    macho/lto-unix.cc)
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

set_property(SOURCE main.cc elf/lto-unix.cc macho/output-chunks.cc APPEND PROPERTY
  COMPILE_DEFINITIONS "MOLD_VERSION=\"${CMAKE_PROJECT_VERSION}\"")

add_custom_target(git_hash
  COMMAND ${CMAKE_COMMAND}
    -DSOURCE_DIR=${CMAKE_SOURCE_DIR}
    -DOUTPUT_FILE=${CMAKE_BINARY_DIR}/git-hash.cc
    -P ${CMAKE_SOURCE_DIR}/update-git-hash.cmake
  DEPENDS update-git-hash.cmake
  BYPRODUCTS git-hash.cc
  VERBATIM)

add_dependencies(mold git_hash)

target_sources(mold PRIVATE
  compress.cc
  demangle.cc
  elf/arch-arm32.cc
  elf/arch-arm64.cc
  elf/arch-i386.cc
  elf/arch-riscv.cc
  elf/arch-x86-64.cc
  elf/cmdline.cc
  elf/dwarf.cc
  elf/gc-sections.cc
  elf/icf.cc
  elf/input-files.cc
  elf/input-sections.cc
  elf/linker-script.cc
  elf/main.cc
  elf/mapfile.cc
  elf/output-chunks.cc
  elf/passes.cc
  elf/relocatable.cc
  elf/thunks.cc
  filepath.cc
  git-hash.cc
  glob.cc
  hyperloglog.cc
  macho/arch-arm64.cc
  macho/arch-x86-64.cc
  macho/cmdline.cc
  macho/dead-strip.cc
  macho/input-files.cc
  macho/input-sections.cc
  macho/main.cc
  macho/mapfile.cc
  macho/output-chunks.cc
  macho/tapi.cc
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
  install(TARGETS mold)
  install(FILES LICENSE DESTINATION ${CMAKE_INSTALL_DOCDIR})
  install(FILES docs/mold.1 DESTINATION ${CMAKE_INSTALL_MANDIR}/man1/)
  install(CODE "
    set(DEST \"\$ENV{DESTDIR}\${CMAKE_INSTALL_PREFIX}\")
    file(RELATIVE_PATH RELPATH
       /${CMAKE_INSTALL_LIBEXECDIR}/mold /${CMAKE_INSTALL_BINDIR}/mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E make_directory
      \${DEST}/${CMAKE_INSTALL_LIBEXECDIR}/mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink \${RELPATH}
      \${DEST}/${CMAKE_INSTALL_LIBEXECDIR}/mold/ld)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold
      \${DEST}/${CMAKE_INSTALL_BINDIR}/ld.mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold
      \${DEST}/${CMAKE_INSTALL_BINDIR}/ld64.mold)
    execute_process(COMMAND ${CMAKE_COMMAND} -E create_symlink mold.1
      \${DEST}/${CMAKE_INSTALL_MANDIR}/man1/ld.mold.1)")
endif()
