cmake_minimum_required(VERSION 3.13 FATAL_ERROR)

if(BUILD_SHARED_LIBS)
  message(FATAL_ERROR "BUILD_SHARED_LIBS is incompatible with BLAKE3_TESTING_CI")
endif()

include(CTest)

# Declare a testing specific variant of the `blake3` library target.
#
# We use a separate library target in order to be able to perform compilation with various
# combinations of features which are too noisy to specify in the main CMake config as options for
# the normal `blake3` target.
#
# Initially this target has no properties but eventually we will populate them by copying all of the
# relevant properties from the normal `blake3` target.
add_library(blake3-testing
  blake3.c
  blake3_dispatch.c
  blake3_portable.c
)

if(BLAKE3_USE_TBB AND TBB_FOUND)
  target_sources(blake3-testing
    PRIVATE
      blake3_tbb.cpp)
endif()

if(BLAKE3_SIMD_TYPE STREQUAL "amd64-asm")
  # Conditionally add amd64 asm files to `blake3-testing` sources
  if(MSVC)
    if(NOT BLAKE3_NO_AVX2)
      list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx2_x86-64_windows_msvc.asm)
    endif()
    if(NOT BLAKE3_NO_AVX512)
      list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx512_x86-64_windows_msvc.asm)
    endif()
    if(NOT BLAKE3_NO_SSE2)
      list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse2_x86-64_windows_msvc.asm)
    endif()
    if(NOT BLAKE3_NO_SSE41)
      list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse41_x86-64_windows_msvc.asm)
    endif()
  elseif(CMAKE_C_COMPILER_ID STREQUAL "GNU"
        OR CMAKE_C_COMPILER_ID STREQUAL "Clang"
        OR CMAKE_C_COMPILER_ID STREQUAL "AppleClang")
    if (WIN32)
      if(NOT BLAKE3_NO_AVX2)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx2_x86-64_windows_gnu.S)
      endif()
      if(NOT BLAKE3_NO_AVX512)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx512_x86-64_windows_gnu.S)
      endif()
      if(NOT BLAKE3_NO_SSE2)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse2_x86-64_windows_gnu.S)
      endif()
      if(NOT BLAKE3_NO_SSE41)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse41_x86-64_windows_gnu.S)
      endif()
    elseif(UNIX)
      if(NOT BLAKE3_NO_AVX2)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx2_x86-64_unix.S)
      endif()
      if(NOT BLAKE3_NO_AVX512)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_avx512_x86-64_unix.S)
      endif()
      if(NOT BLAKE3_NO_SSE2)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse2_x86-64_unix.S)
      endif()
      if(NOT BLAKE3_NO_SSE41)
        list(APPEND BLAKE3_TESTING_AMD64_ASM_SOURCES blake3_sse41_x86-64_unix.S)
      endif()
    endif()
  endif()
  target_sources(blake3-testing PRIVATE ${BLAKE3_AMD64_ASM_SOURCES})
elseif(BLAKE3_SIMD_TYPE STREQUAL "x86-intrinsics")
  # Conditionally add amd64 C files to `blake3-testing` sources
  if (NOT DEFINED BLAKE3_CFLAGS_SSE2
      OR NOT DEFINED BLAKE3_CFLAGS_SSE4.1
      OR NOT DEFINED BLAKE3_CFLAGS_AVX2
      OR NOT DEFINED BLAKE3_CFLAGS_AVX512)
    message(WARNING "BLAKE3_SIMD_TYPE is set to 'x86-intrinsics' but no compiler flags are available for the target architecture.")
  else()
    set(BLAKE3_SIMD_X86_INTRINSICS ON)
  endif()

  if(NOT BLAKE3_NO_AVX2)
    target_sources(blake3-testing PRIVATE blake3_avx2.c)
    set_source_files_properties(blake3_avx2.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_AVX2}")
  endif()
  if(NOT BLAKE3_NO_AVX512)
    target_sources(blake3-testing PRIVATE blake3_avx512.c)
    set_source_files_properties(blake3_avx512.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_AVX512}")
  endif()
  if(NOT BLAKE3_NO_SSE2)
    target_sources(blake3-testing PRIVATE blake3_sse2.c)
    set_source_files_properties(blake3_sse2.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_SSE2}")
  endif()
  if(NOT BLAKE3_NO_SSE41)
    target_sources(blake3-testing PRIVATE blake3_sse41.c)
    set_source_files_properties(blake3_sse41.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_SSE4.1}")
  endif()

elseif(BLAKE3_SIMD_TYPE STREQUAL "neon-intrinsics")
  # Conditionally add neon C files to `blake3-testing` sources

  target_sources(blake3-testing PRIVATE
    blake3_neon.c
  )
  target_compile_definitions(blake3-testing PRIVATE
    BLAKE3_USE_NEON=1
  )

  if (DEFINED BLAKE3_CFLAGS_NEON)
    set_source_files_properties(blake3_neon.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_NEON}")
  endif()

elseif(BLAKE3_SIMD_TYPE STREQUAL "none")
  # Disable neon if simd type is "none". We check for individual amd64 features further below.

  target_compile_definitions(blake3-testing PRIVATE
    BLAKE3_USE_NEON=0
  )

endif()

if(BLAKE3_NO_AVX2)
  target_compile_definitions(blake3-testing PRIVATE BLAKE3_NO_AVX2)
endif()
if(BLAKE3_NO_AVX512)
  target_compile_definitions(blake3-testing PRIVATE BLAKE3_NO_AVX512)
endif()
if(BLAKE3_NO_SSE2)
  target_compile_definitions(blake3-testing PRIVATE BLAKE3_NO_SSE2)
endif()
if(BLAKE3_NO_SSE41)
  target_compile_definitions(blake3-testing PRIVATE BLAKE3_NO_SSE41)
endif()

target_compile_definitions(blake3-testing PUBLIC BLAKE3_TESTING)

get_target_property(BLAKE3_COMPILE_DEFINITIONS blake3 COMPILE_DEFINITIONS)
if(BLAKE3_COMPILE_DEFINITIONS)
  target_compile_definitions(blake3-testing PUBLIC
    ${BLAKE3_COMPILE_DEFINITIONS})
endif()

get_target_property(BLAKE3_COMPILE_OPTIONS blake3 COMPILE_OPTIONS)
if(BLAKE3_COMPILE_OPTIONS)
  target_compile_options(blake3-testing PRIVATE
    ${BLAKE3_COMPILE_OPTIONS}
    -O3
    -Wall
    -Wextra
    -pedantic
    -fstack-protector-strong
    -D_FORTIFY_SOURCE=2
    -fPIE
    -fvisibility=hidden
    -fsanitize=address,undefined
  )
endif()

get_target_property(BLAKE3_INCLUDE_DIRECTORIES blake3 INCLUDE_DIRECTORIES)
if(BLAKE3_INCLUDE_DIRECTORIES)
  target_include_directories(blake3-testing PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}>
    $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
  )
endif()

get_target_property(BLAKE3_LINK_LIBRARIES blake3 LINK_LIBRARIES)
if(BLAKE3_LINK_LIBRARIES)
  target_link_libraries(blake3-testing PRIVATE ${BLAKE3_LINK_LIBRARIES})
endif()

get_target_property(BLAKE3_LINK_OPTIONS blake3 LINK_OPTIONS)
if(BLAKE3_LINK_OPTIONS)
  target_link_options(blake3-testing PRIVATE
    ${BLAKE3_LINK_OPTIONS}
    -fsanitize=address,undefined
    -pie
    -Wl,-z,relro,-z,now
  )
endif()

# test asm target
add_executable(blake3-asm-test
  main.c
)
set_target_properties(blake3-asm-test PROPERTIES
  OUTPUT_NAME blake3
  RUNTIME_OUTPUT_DIRECTORY ${CMAKE_SOURCE_DIR})
target_link_libraries(blake3-asm-test PRIVATE blake3-testing)
target_compile_definitions(blake3-asm-test PRIVATE BLAKE3_TESTING)
target_compile_options(blake3-asm-test PRIVATE
  -O3
  -Wall
  -Wextra
  -pedantic
  -fstack-protector-strong
  -D_FORTIFY_SOURCE=2
  -fPIE
  -fvisibility=hidden
  -fsanitize=address,undefined
)
target_link_options(blake3-asm-test PRIVATE
  -fsanitize=address,undefined
  -pie
  -Wl,-z,relro,-z,now
)

add_test(NAME blake3-testing
  COMMAND "${CMAKE_CTEST_COMMAND}"
    --verbose
    --extra-verbose
    --build-and-test "${CMAKE_SOURCE_DIR}" "${CMAKE_BINARY_DIR}"
    --build-generator "${CMAKE_GENERATOR}"
    --build-makeprogram "${CMAKE_MAKE_PROGRAM}"
    --build-project libblake3
    --build-target blake3-asm-test
    --build-options
      --fresh
      "-DBUILD_SHARED_LIBS=${BUILD_SHARED_LIBS}"
      "-DBLAKE3_TESTING=${BLAKE3_TESTING}"
      "-DBLAKE3_TESTING_CI=${BLAKE3_TESTING_CI}"
      "-DBLAKE3_USE_TBB=${BLAKE3_USE_TBB}"
      "-DBLAKE3_SIMD_TYPE=${BLAKE3_SIMD_TYPE}"
      "-DBLAKE3_NO_SSE2=${BLAKE3_NO_SSE2}"
      "-DBLAKE3_NO_SSE41=${BLAKE3_NO_SSE41}"
      "-DBLAKE3_NO_AVX2=${BLAKE3_NO_AVX2}"
      "-DBLAKE3_NO_AVX512=${BLAKE3_NO_AVX512}"
    --test-command
      "${CMAKE_SOURCE_DIR}/test.py"
  )
