cmake_minimum_required(VERSION 3.9 FATAL_ERROR)

# respect C_EXTENSIONS OFF without explicitly setting C_STANDARD
if (POLICY CMP0128)
  cmake_policy(SET CMP0128 NEW)
endif()
# mark_as_advanced does not implicitly create UNINITIALIZED cache entries
if (POLICY CMP0102)
  cmake_policy(SET CMP0102 NEW)
endif()

project(libblake3
  VERSION 1.8.2
  DESCRIPTION "BLAKE3 C implementation"
  LANGUAGES C CXX ASM
)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

option(BLAKE3_USE_TBB "Enable oneTBB parallelism" OFF)
option(BLAKE3_FETCH_TBB "Allow fetching oneTBB from GitHub if not found on system" OFF)

include(CTest)
include(FeatureSummary)
include(GNUInstallDirs)

add_subdirectory(dependencies)

# architecture lists for which to enable assembly / SIMD sources
set(BLAKE3_AMD64_NAMES amd64 AMD64 x86_64)
set(BLAKE3_X86_NAMES i686 x86 X86)
set(BLAKE3_ARMv8_NAMES aarch64 AArch64 arm64 ARM64 armv8 armv8a)
# default SIMD compiler flag configuration (can be overriden by toolchains or CLI)
if(MSVC)
  set(BLAKE3_CFLAGS_SSE2 "/arch:SSE2" CACHE STRING "the compiler flags to enable SSE2")
  # MSVC has no dedicated sse4.1 flag (see https://learn.microsoft.com/en-us/cpp/build/reference/arch-x86?view=msvc-170)
  set(BLAKE3_CFLAGS_SSE4.1 "/arch:AVX" CACHE STRING "the compiler flags to enable SSE4.1")
  set(BLAKE3_CFLAGS_AVX2 "/arch:AVX2" CACHE STRING "the compiler flags to enable AVX2")
  set(BLAKE3_CFLAGS_AVX512 "/arch:AVX512" CACHE STRING "the compiler flags to enable AVX512")

  set(BLAKE3_AMD64_ASM_SOURCES
    blake3_avx2_x86-64_windows_msvc.asm
    blake3_avx512_x86-64_windows_msvc.asm
    blake3_sse2_x86-64_windows_msvc.asm
    blake3_sse41_x86-64_windows_msvc.asm
  )

elseif(CMAKE_C_COMPILER_ID STREQUAL "GNU"
       OR CMAKE_C_COMPILER_ID STREQUAL "Clang"
       OR CMAKE_C_COMPILER_ID STREQUAL "AppleClang")
  set(BLAKE3_CFLAGS_SSE2 "-msse2" CACHE STRING "the compiler flags to enable SSE2")
  set(BLAKE3_CFLAGS_SSE4.1 "-msse4.1" CACHE STRING "the compiler flags to enable SSE4.1")
  set(BLAKE3_CFLAGS_AVX2 "-mavx2" CACHE STRING "the compiler flags to enable AVX2")
  set(BLAKE3_CFLAGS_AVX512 "-mavx512f -mavx512vl" CACHE STRING "the compiler flags to enable AVX512")

  if (WIN32 OR CYGWIN)
    set(BLAKE3_AMD64_ASM_SOURCES
      blake3_avx2_x86-64_windows_gnu.S
      blake3_avx512_x86-64_windows_gnu.S
      blake3_sse2_x86-64_windows_gnu.S
      blake3_sse41_x86-64_windows_gnu.S
    )

  elseif(UNIX)
    set(BLAKE3_AMD64_ASM_SOURCES
      blake3_avx2_x86-64_unix.S
      blake3_avx512_x86-64_unix.S
      blake3_sse2_x86-64_unix.S
      blake3_sse41_x86-64_unix.S
    )
  endif()

  if (CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_ARMv8_NAMES
      AND NOT CMAKE_SIZEOF_VOID_P EQUAL 8)
    # 32-bit ARMv8 needs NEON to be enabled explicitly
    set(BLAKE3_CFLAGS_NEON "-mfpu=neon" CACHE STRING "the compiler flags to enable NEON")
  endif()
endif()

mark_as_advanced(BLAKE3_CFLAGS_SSE2 BLAKE3_CFLAGS_SSE4.1 BLAKE3_CFLAGS_AVX2 BLAKE3_CFLAGS_AVX512 BLAKE3_CFLAGS_NEON)
mark_as_advanced(BLAKE3_AMD64_ASM_SOURCES)

message(STATUS "BLAKE3 SIMD configuration: ${CMAKE_C_COMPILER_ARCHITECTURE_ID}")
if(MSVC AND DEFINED CMAKE_C_COMPILER_ARCHITECTURE_ID)
  if(CMAKE_C_COMPILER_ARCHITECTURE_ID MATCHES "[Xx]86")
    set(BLAKE3_SIMD_TYPE "x86-intrinsics" CACHE STRING "the SIMD acceleration type to use")

  elseif(CMAKE_C_COMPILER_ARCHITECTURE_ID MATCHES "[Xx]64")
    set(BLAKE3_SIMD_TYPE "amd64-asm" CACHE STRING "the SIMD acceleration type to use")

  elseif(CMAKE_C_COMPILER_ARCHITECTURE_ID MATCHES "[Aa][Rr][Mm]64")
    set(BLAKE3_SIMD_TYPE "neon-intrinsics" CACHE STRING "the SIMD acceleration type to use")

  else()
    set(BLAKE3_SIMD_TYPE "none" CACHE STRING "the SIMD acceleration type to use")
  endif()

elseif(CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_AMD64_NAMES)
  set(BLAKE3_SIMD_TYPE "amd64-asm" CACHE STRING "the SIMD acceleration type to use")

elseif(CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_X86_NAMES
       AND DEFINED BLAKE3_CFLAGS_SSE2
       AND DEFINED BLAKE3_CFLAGS_SSE4.1
       AND DEFINED BLAKE3_CFLAGS_AVX2
       AND DEFINED BLAKE3_CFLAGS_AVX512)
  set(BLAKE3_SIMD_TYPE "x86-intrinsics" CACHE STRING "the SIMD acceleration type to use")

elseif((CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_ARMv8_NAMES
          OR ANDROID_ABI STREQUAL "armeabi-v7a"
          OR BLAKE3_USE_NEON_INTRINSICS)
        AND (DEFINED BLAKE3_CFLAGS_NEON
          OR CMAKE_SIZEOF_VOID_P EQUAL 8))
  set(BLAKE3_SIMD_TYPE "neon-intrinsics" CACHE STRING "the SIMD acceleration type to use")

else()
  set(BLAKE3_SIMD_TYPE "none" CACHE STRING "the SIMD acceleration type to use")
endif()

mark_as_advanced(BLAKE3_SIMD_TYPE)

# library target
add_library(blake3
  blake3.c
  blake3_dispatch.c
  blake3_portable.c
)
add_library(BLAKE3::blake3 ALIAS blake3)

# library configuration
set(PKG_CONFIG_CFLAGS)
if (BUILD_SHARED_LIBS)
  target_compile_definitions(blake3
    PUBLIC BLAKE3_DLL
    PRIVATE BLAKE3_DLL_EXPORTS
  )
  list(APPEND PKG_CONFIG_CFLAGS -DBLAKE3_DLL)
endif()
target_include_directories(blake3 PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}>
  $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)
set_target_properties(blake3 PROPERTIES
  VERSION ${PROJECT_VERSION}
  SOVERSION 0
  C_VISIBILITY_PRESET hidden
  C_EXTENSIONS OFF
)
target_compile_features(blake3 PUBLIC c_std_99)
if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.12)
  target_compile_features(blake3 PUBLIC cxx_std_20)
  # else: add it further below through `BLAKE3_CMAKE_CXXFLAGS_*`
endif()

# ensure C_EXTENSIONS OFF is respected without overriding CMAKE_C_STANDARD
# which may be set by the user or toolchain file
if (NOT POLICY CMP0128 AND NOT DEFINED CMAKE_C_STANDARD)
  set_target_properties(blake3 PROPERTIES C_STANDARD 99)
endif()

# optional SIMD sources
if(BLAKE3_SIMD_TYPE STREQUAL "amd64-asm")
  if (NOT DEFINED BLAKE3_AMD64_ASM_SOURCES)
    message(FATAL_ERROR "BLAKE3_SIMD_TYPE is set to 'amd64-asm' but no assembly sources are available for the target architecture.")
  endif()
  set(BLAKE3_SIMD_AMD64_ASM ON)

  if(MSVC)
    enable_language(ASM_MASM)
  endif()

  target_sources(blake3 PRIVATE ${BLAKE3_AMD64_ASM_SOURCES})

elseif(BLAKE3_SIMD_TYPE STREQUAL "x86-intrinsics")
  if (NOT DEFINED BLAKE3_CFLAGS_SSE2
      OR NOT DEFINED BLAKE3_CFLAGS_SSE4.1
      OR NOT DEFINED BLAKE3_CFLAGS_AVX2
      OR NOT DEFINED BLAKE3_CFLAGS_AVX512)
    message(FATAL_ERROR "BLAKE3_SIMD_TYPE is set to 'x86-intrinsics' but no compiler flags are available for the target architecture.")
  endif()
  set(BLAKE3_SIMD_X86_INTRINSICS ON)

  target_sources(blake3 PRIVATE
    blake3_avx2.c
    blake3_avx512.c
    blake3_sse2.c
    blake3_sse41.c
  )
  set_source_files_properties(blake3_avx2.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_AVX2}")
  set_source_files_properties(blake3_avx512.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_AVX512}")
  set_source_files_properties(blake3_sse2.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_SSE2}")
  set_source_files_properties(blake3_sse41.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_SSE4.1}")

elseif(BLAKE3_SIMD_TYPE STREQUAL "neon-intrinsics")
  set(BLAKE3_SIMD_NEON_INTRINSICS ON)

  target_sources(blake3 PRIVATE
    blake3_neon.c
  )
  target_compile_definitions(blake3 PRIVATE
    BLAKE3_USE_NEON=1
  )

  if (DEFINED BLAKE3_CFLAGS_NEON)
    set_source_files_properties(blake3_neon.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_NEON}")
  endif()

elseif(BLAKE3_SIMD_TYPE STREQUAL "none")
  target_compile_definitions(blake3 PRIVATE
    BLAKE3_USE_NEON=0
    BLAKE3_NO_SSE2
    BLAKE3_NO_SSE41
    BLAKE3_NO_AVX2
    BLAKE3_NO_AVX512
  )

else()
  message(FATAL_ERROR "BLAKE3_SIMD_TYPE is set to an unknown value: '${BLAKE3_SIMD_TYPE}'")
endif()

if(BLAKE3_USE_TBB)
  find_package(TBB 2021.11.0 QUIET)
  if(NOT TBB_FOUND AND NOT TARGET TBB::tbb)
    message(WARNING
      "oneTBB not found; disabling BLAKE3_USE_TBB\n"
      "Enable BLAKE3_FETCH_TBB to automatically fetch and build oneTBB"
    )
    set(BLAKE3_USE_TBB OFF)
  else()
    target_sources(blake3
      PRIVATE
        blake3_tbb.cpp)
    target_link_libraries(blake3
      PUBLIC
        # Make shared TBB a transitive dependency. The consuming program is technically not required
        # to link TBB in order for libblake3 to function but we do this in order to prevent the
        # possibility of multiple separate TBB runtimes being linked into a final program in case
        # the consuming program also happens to already use TBB.
        TBB::tbb)
    target_compile_definitions(blake3
      PUBLIC
        BLAKE3_USE_TBB)
  endif()
  list(APPEND PKG_CONFIG_REQUIRES "tbb >= ${TBB_VERSION}")
  list(APPEND PKG_CONFIG_CFLAGS -DBLAKE3_USE_TBB)
  include(CheckCXXSymbolExists)
  check_cxx_symbol_exists(_LIBCPP_VERSION "version" BLAKE3_HAVE_LIBCPP)
  check_cxx_symbol_exists(__GLIBCXX__ "version" BLAKE3_HAVE_GLIBCXX)
  if(BLAKE3_HAVE_GLIBCXX)
    list(APPEND PKG_CONFIG_LIBS -lstdc++)
  elseif(BLAKE3_HAVE_LIBCPP)
    list(APPEND PKG_CONFIG_LIBS -lc++)
  endif()
endif()

if(BLAKE3_USE_TBB)
  # Define some scratch variables for building appropriate flags per compiler
  if(CMAKE_VERSION VERSION_LESS 3.12)
    set(APPEND BLAKE3_CXX_STANDARD_FLAGS_GNU -std=c++20)
    set(APPEND BLAKE3_CXX_STANDARD_FLAGS_MSVC /std:c++20)
  endif()
  set(BLAKE3_CXXFLAGS_GNU "-fno-exceptions;-fno-rtti;${BLAKE3_CXX_STANDARD_FLAGS_GNU}" CACHE STRING "C++ flags used for compiling private BLAKE3 library components with GNU-like compiler frontends.")
  set(BLAKE3_CXXFLAGS_MSVC "/EHs-c-;/GR-;${BLAKE3_CXX_STANDARD_FLAGS_MSVC}" CACHE STRING "C++ flags used for compiling private BLAKE3 library components with MSVC-like compiler frontends.")
  # Get the C++ compiler name without extension
  get_filename_component(BLAKE3_CMAKE_CXX_COMPILER_NAME "${CMAKE_CXX_COMPILER}" NAME_WE)
  # Strip any trailing versioning from the C++ compiler name
  string(REGEX MATCH "^(clang\\+\\+|clang-cl)" BLAKE3_CMAKE_CXX_COMPILER_NAME "${BLAKE3_CMAKE_CXX_COMPILER_NAME}")

  # TODO: Simplify with CMAKE_CXX_COMPILER_FRONTEND_VARIANT once min CMake version is 3.14.
  if(CMAKE_CXX_COMPILER_ID STREQUAL "AppleClang")
    target_compile_options(blake3 PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${BLAKE3_CXXFLAGS_GNU}>)
  elseif(CMAKE_CXX_COMPILER_ID STREQUAL "Clang")
    if(BLAKE3_CMAKE_CXX_COMPILER_NAME STREQUAL "clang++")
      target_compile_options(blake3 PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${BLAKE3_CXXFLAGS_GNU}>)
    elseif(BLAKE3_CMAKE_CXX_COMPILER_NAME STREQUAL "clang-cl")
      target_compile_options(blake3 PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${BLAKE3_CXXFLAGS_MSVC}>)
    endif()
  elseif(CMAKE_CXX_COMPILER_ID STREQUAL "GNU")
    target_compile_options(blake3 PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${BLAKE3_CXXFLAGS_GNU}>)
  elseif(CMAKE_CXX_COMPILER_ID STREQUAL "MSVC")
    target_compile_options(blake3 PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${BLAKE3_CXXFLAGS_MSVC}>)
  endif()

  # Undefine scratch variables
  unset(BLAKE3_CXX_STANDARD_FLAGS_GNU)
  unset(BLAKE3_CXX_STANDARD_FLAGS_MSVC)
  unset(BLAKE3_CMAKE_CXX_COMPILER_NAME)
  unset(BLAKE3_CXXFLAGS_GNU)
  unset(BLAKE3_CXXFLAGS_MSVC)
endif()

# cmake install support
install(FILES blake3.h DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(TARGETS blake3 EXPORT blake3-targets
  ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}"
  LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}"
  RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}"
)
install(EXPORT blake3-targets
  NAMESPACE BLAKE3::
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/blake3"
)

include(CMakePackageConfigHelpers)
configure_package_config_file(blake3-config.cmake.in
    "${CMAKE_CURRENT_BINARY_DIR}/blake3-config.cmake"

    INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/blake3"
)
write_basic_package_version_file(
    "${CMAKE_CURRENT_BINARY_DIR}/blake3-config-version.cmake"
    VERSION ${libblake3_VERSION}
    COMPATIBILITY SameMajorVersion
)
install(FILES
        "${CMAKE_CURRENT_BINARY_DIR}/blake3-config.cmake"
        "${CMAKE_CURRENT_BINARY_DIR}/blake3-config-version.cmake"
    DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/blake3"
)

# Function for joining paths known from most languages
#
# SPDX-License-Identifier: (MIT OR CC0-1.0)
# Copyright 2020 Jan Tojnar
# https://github.com/jtojnar/cmake-snips
#
# Modelled after Python’s os.path.join
# https://docs.python.org/3.7/library/os.path.html#os.path.join
# Windows not supported
function(join_paths joined_path first_path_segment)
    set(temp_path "${first_path_segment}")
    foreach(current_segment IN LISTS ARGN)
        if(NOT ("${current_segment}" STREQUAL ""))
            if(IS_ABSOLUTE "${current_segment}")
                set(temp_path "${current_segment}")
            else()
                set(temp_path "${temp_path}/${current_segment}")
            endif()
        endif()
    endforeach()
    set(${joined_path} "${temp_path}" PARENT_SCOPE)
endfunction()

# In-place rewrite a string and and join by `sep`.
#
# TODO: Replace function with list(JOIN) when updating to CMake 3.12
function(join_pkg_config_field sep requires)
  set(_requires "${${requires}}") # avoid shadowing issues, e.g. "${requires}"=len
  list(LENGTH "${requires}" len)
  set(idx 1)
  foreach(req IN LISTS _requires)
    string(APPEND acc "${req}")
    if(idx LESS len)
      string(APPEND acc "${sep}")
    endif()
    math(EXPR idx "${idx} + 1")
  endforeach()
  set("${requires}" "${acc}" PARENT_SCOPE)
endfunction()

# pkg-config support
join_pkg_config_field(", " PKG_CONFIG_REQUIRES)
join_pkg_config_field(" " PKG_CONFIG_LIBS)
join_pkg_config_field(" " PKG_CONFIG_CFLAGS)
join_paths(PKG_CONFIG_INSTALL_LIBDIR "\${prefix}" "${CMAKE_INSTALL_LIBDIR}")
join_paths(PKG_CONFIG_INSTALL_INCLUDEDIR "\${prefix}" "${CMAKE_INSTALL_INCLUDEDIR}")
configure_file(libblake3.pc.in libblake3.pc @ONLY)
install(FILES "${CMAKE_BINARY_DIR}/libblake3.pc"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/pkgconfig")

# print feature summary
# add_feature_info cannot directly use the BLAKE3_SIMD_TYPE :(
add_feature_info("AMD64 assembly" BLAKE3_SIMD_AMD64_ASM "The library uses hand written amd64 SIMD assembly.")
add_feature_info("x86 SIMD intrinsics" BLAKE3_SIMD_X86_INTRINSICS "The library uses x86 SIMD intrinsics.")
add_feature_info("NEON SIMD intrinsics" BLAKE3_SIMD_NEON_INTRINSICS "The library uses NEON SIMD intrinsics.")
add_feature_info("oneTBB parallelism" BLAKE3_USE_TBB "The library uses oneTBB parallelism.")
feature_summary(WHAT ENABLED_FEATURES)

if(BLAKE3_EXAMPLES)
  include(BLAKE3/Examples)
endif()
if(BLAKE3_TESTING)
  include(BLAKE3/Testing)
endif()
