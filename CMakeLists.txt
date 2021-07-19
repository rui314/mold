cmake_minimum_required(VERSION 3.13)

project(mold
        LANGUAGES C CXX
        VERSION 0.1.1)

###############################################################################
# Configuration options
###############################################################################

option(MOLD_ASAN "Use address sanitizer." OFF)
option(MOLD_TSAN "Use thread sanitizer." OFF)

# If this is a Git checkout, use Git to define the GIT_HASH that will be
# appended to the version string.

find_package(Git QUIET)

if(Git_FOUND)
  execute_process(
    COMMAND ${GIT_EXECUTABLE} rev-parse --git-dir
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    RESULT_VARIABLE   NOT_GIT_DIR
    OUTPUT_QUIET
    ERROR_QUIET)

  if(NOT ${NOT_GIT_DIR})
  execute_process(
    COMMAND ${GIT_EXECUTABLE} rev-parse HEAD
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    OUTPUT_VARIABLE GIT_HASH
    OUTPUT_STRIP_TRAILING_WHITESPACE)
  endif()
endif()

###############################################################################
# Project-wide options
###############################################################################

set(CMAKE_C_STANDARD   11)
set(CMAKE_CXX_STANDARD 20)
set(FETCHCONTENT_QUIET OFF)

if(MOLD_ASAN)
  add_compile_options(-fsanitize=address)
  add_link_options(-fsanitize=address)
endif()

if(MOLD_TSAN)
  add_compile_options(-fsanitize=thread)
  add_link_options(-fsanitize=thread)
endif()

set_property(GLOBAL PROPERTY USE_FOLDERS ON)

###############################################################################
# Third-party dependencies
###############################################################################

# Strategy: search for the dependency using 'find_package' first. If the
#           installed system version matches the one we want, use that.
#           If there's a version mismatch, or if the library is not available,
#           fetch the library using CMake, build and use it.

include(FetchContent)

# mimalloc
FetchContent_Declare(mimalloc
  GIT_REPOSITORY  https://github.com/microsoft/mimalloc.git
  GIT_TAG         dc6bce256d4f3ce87761f9337977dff3d8b1776c
  GIT_PROGRESS    TRUE)

FetchContent_GetProperties(mimalloc)

if(NOT mimalloc_POPULATED)
  FetchContent_Populate(mimalloc)
  set(MI_BUILD_TESTS  OFF CACHE BOOL "Build mimalloc test executables.")
  set(MI_BUILD_SHARED OFF CACHE BOOL "Build shared library")
  set(MI_BUILD_OBJECT OFF CACHE BOOL "Build object library")
  list(APPEND mi_cflags "-DMI_USE_ENVIRON=0")
  add_subdirectory(${mimalloc_SOURCE_DIR} EXCLUDE_FROM_ALL)
endif()

# oneTBB
find_package(TBB QUIET COMPONENTS tbb)
if(NOT TBB_tbb_FOUND)
  FetchContent_Declare(TBB
    GIT_REPOSITORY  https://github.com/oneapi-src/oneTBB.git
    GIT_TAG         eca91f16d7490a8abfdee652dadf457ec820cc37
    GIT_PROGRESS    TRUE)

  FetchContent_GetProperties(TBB)
  if(NOT TBB_POPULATED)
    FetchContent_Populate(TBB)
    include(${tbb_SOURCE_DIR}/cmake/TBBBuild.cmake)
    tbb_build(TBB_ROOT ${tbb_SOURCE_DIR}
              CONFIG_DIR ${tbb_BINARY_DIR})
    find_package(TBB REQUIRED
                 CONFIG
                 PATHS ${tbb_SOURCE_DIR}/cmake)
  endif()
else()
  if(NOT TARGET TBB::tbb)
    add_library(TBB::tbb ALIAS tbb)
  endif()
endif()

# pthreads
if(UNIX)
  set(THREADS_PREFER_PTHREAD_FLAG ON)
  find_package(Threads REQUIRED)
endif()

# xxHash
FetchContent_Declare(xxHash
  GIT_REPOSITORY  https://github.com/Cyan4973/xxHash.git
  GIT_TAG         94e5f23e736f2bb67ebdf90727353e65344f9fc0
  GIT_PROGRESS    TRUE)

FetchContent_GetProperties(xxHash)

if(NOT xxHash_POPULATED)
  FetchContent_Populate(xxHash)
  add_subdirectory(${xxhash_SOURCE_DIR}/cmake_unofficial EXCLUDE_FROM_ALL)
endif()

# zlib
find_package(ZLIB 1.2 QUIET)
if(NOT ZLIB_FOUND)
  FetchContent_Declare(zlib
    GIT_REPOSITORY  https://github.com/madler/zlib.git
    GIT_TAG         cacf7f1d4e3d44d871b605da3b647f07d718623f)

  FetchContent_GetProperties(zlib)
  if(NOT zlib_POPULATED)
    FetchContent_Populate(zlib)
    add_subdirectory(${zlib_SOURCE_DIR} ${zlib_BINARY_DIR} EXCLUDE_FROM_ALL)
  endif()
endif()

###############################################################################
# Targets
###############################################################################

add_executable(mold)

target_sources(mold
  PRIVATE
    arch_i386.cc
    arch_x86_64.cc
    archive_file.cc
    cmdline.cc
    compress.cc
    elf.h
    filepath.cc
    gc_sections.cc
    glob.cc
    icf.cc
    input_sections.cc
    linker_script.cc
    main.cc
    mapfile.cc
    mold.h
    object_file.cc
    output_chunks.cc
    output_file.cc
    passes.cc
    perf.cc
    subprocess.cc
    symbols.cc
    tar.cc)

target_compile_definitions(mold
  PRIVATE
    "GIT_HASH=\"${GIT_HASH}\""
    "MOLD_VERSION=\"${PROJECT_VERSION}\"")

target_compile_options(mold
  PRIVATE
    $<$<PLATFORM_ID:Linux>:-Wno-deprecated-volatile>)

target_link_libraries(mold
  PRIVATE
    mimalloc-static
    TBB::tbb
    xxHash::xxhash
    ZLIB::ZLIB
    crypto
    $<$<PLATFORM_ID:Linux>:dl>
    $<$<PLATFORM_ID:Linux>:Threads::Threads>)

add_library(mold-wrapper SHARED)

target_sources(mold-wrapper
  PRIVATE
    mold-wrapper.c)

target_link_libraries(mold-wrapper
  PRIVATE
    $<$<PLATFORM_ID:Linux>:dl>)

# mold-wrapper.so is searched for by name, so disable the 'lib' prefix
set_target_properties(mold-wrapper
  PROPERTIES PREFIX "")
