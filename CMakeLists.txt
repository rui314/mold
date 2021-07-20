cmake_minimum_required(VERSION 3.13)

project(mold
  LANGUAGES C CXX
  VERSION 0.9.2)

option(MOLD_USE_ASAN "Use address sanitizer" OFF)
option(MOLD_USE_TSAN "Use thread sanitizer" OFF)
option(MOLD_USE_SYSTEM_MIMALLOC "Use system mimalloc library" OFF)
option(MOLD_USE_SYSTEM_TBB "Use system OneTBB library" OFF)
option(MOLD_BUILD_STATIC_EXE "Build statically-linked mold executable" OFF)

set(MOLD_LIBS "")
set(MOLD_INCLUDES "")

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

# Project-wide options

set(CMAKE_C_STANDARD   11)
set(CMAKE_CXX_STANDARD 20)
set(FETCHCONTENT_QUIET OFF)

if(MOLD_USE_ASAN)
  add_compile_options(-fsanitize=address)
  add_link_options(-fsanitize=address)
endif()

if(MOLD_USE_TSAN)
  add_compile_options(-fsanitize=thread)
  add_link_options(-fsanitize=thread)
endif()

set_property(GLOBAL PROPERTY USE_FOLDERS ON)

# Third-party dependencies

if(MOLD_BUILD_STATIC_EXE)
  set(CMAKE_FIND_LIBRARY_SUFFIXES ".a")
  set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -static")
endif()

if(MOLD_USE_SYSTEM_MIMALLOC)
  list(APPEND MOLD_LIBS mimalloc)
else()
  option(MI_BUILD_SHARED "Build shared library" OFF)
  option(MI_BUILD_OBJECT "Build object library" OFF)
  option(MI_BUILD_TESTS  "Build test executables" OFF)

  add_subdirectory(mimalloc EXCLUDE_FROM_ALL)

  list(APPEND MOLD_LIBS mimalloc-static)
  list(APPEND MOLD_INCLUDES ${CMAKE_CURRENT_SOURCE_DIR}/mimalloc/include)
endif()

if(NOT MOLD_USE_SYSTEM_TBB)
  set(BUILD_SHARED_LIBS OFF)
  set(CMAKE_CXX_FLAGS "-D__TBB_DYNAMIC_LOAD_ENABLED=0")
  option(TBB_TEST "Enable testing" OFF)

  add_subdirectory(tbb EXCLUDE_FROM_ALL)

  list(APPEND MOLD_INCLUDES ${CMAKE_CURRENT_SOURCE_DIR}/tbb/include)
endif()

list(APPEND MOLD_LIBS tbb)

if(UNIX)
  set(THREADS_PREFER_PTHREAD_FLAG ON)
  find_package(Threads REQUIRED)
endif()

find_package(OpenSSL REQUIRED)
find_package(ZLIB REQUIRED)

# Targets

add_executable(mold)

target_compile_definitions(mold
  PRIVATE
  "GIT_HASH=\"${GIT_HASH}\""
  "MOLD_VERSION=\"${PROJECT_VERSION}\"")

target_sources(mold
  PRIVATE
  arch_aarch64.cc
  arch_i386.cc
  archive_file.cc
  arch_x86_64.cc
  cmdline.cc
  compress.cc
  concurrent_map.cc
  filepath.cc
  gc_sections.cc
  hyperloglog.cc
  icf.cc
  input_sections.cc
  linker_script.cc
  main.cc
  mapfile.cc
  memory_mapped_file.cc
  object_file.cc
  output_chunks.cc
  output_file.cc
  passes.cc
  perf.cc
  relocatable.cc
  subprocess.cc
  symbols.cc
  tar.cc
  )

target_include_directories(mold
  PUBLIC
  ${PROJECT_BINARY_DIR}
  ${MOLD_INCLUDES}
  )

target_link_libraries(mold
  PRIVATE
  ${MOLD_LIBS}
  xxhash
  OpenSSL::Crypto
  ZLIB::ZLIB
  $<$<PLATFORM_ID:Linux>:dl>
  $<$<PLATFORM_ID:Linux>:Threads::Threads>)

add_library(mold-wrapper SHARED)

target_sources(mold-wrapper PRIVATE mold-wrapper.c)

target_link_libraries(mold-wrapper
  PRIVATE
  $<$<PLATFORM_ID:Linux>:dl>)

# mold-wrapper.so is searched for by name, so disable the 'lib' prefix
set_target_properties(mold-wrapper PROPERTIES PREFIX "")

file(CREATE_LINK mold${CMAKE_EXECUTABLE_SUFFIX}
  ${CMAKE_CURRENT_BINARY_DIR}/ld SYMBOLIC)

# Tests

enable_testing()

file(GLOB TEST_SCRIPTS RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} test/*.sh)

foreach(testScript ${TEST_SCRIPTS})
  add_test(${testScript} ${CMAKE_CURRENT_SOURCE_DIR}/${testScript}
    ${CMAKE_CURRENT_BINARY_DIR}/mold${CMAKE_EXECUTABLE_SUFFIX})
endforeach(testScript)

# Install

install(TARGETS mold DESTINATION bin)
install(TARGETS mold-wrapper DESTINATION lib/mold)
