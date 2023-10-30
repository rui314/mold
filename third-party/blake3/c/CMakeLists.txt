cmake_minimum_required(VERSION 3.9)

project(libblake3
  VERSION 1.4.1
  DESCRIPTION "BLAKE3 C implementation"
  LANGUAGES C ASM
)

include(CheckCCompilerFlag)
include(FeatureSummary)
include(GNUInstallDirs)

# default SIMD compiler flag configuration (can be overriden by toolchains or CLI)
if(MSVC)
  set(BLAKE3_CFLAGS_SSE2 "/arch:SSE2" CACHE STRING "the compiler flags to enable SSE2")
  # MSVC has no dedicated sse4.1 flag (see https://learn.microsoft.com/en-us/cpp/build/reference/arch-x86?view=msvc-170)
  set(BLAKE3_CFLAGS_SSE4.1 "/arch:AVX" CACHE STRING "the compiler flags to enable SSE4.1")
  set(BLAKE3_CFLAGS_AVX2 "/arch:AVX2" CACHE STRING "the compiler flags to enable AVX2")
  set(BLAKE3_CFLAGS_AVX512 "/arch:AVX512" CACHE STRING "the compiler flags to enable AVX512")

elseif(CMAKE_C_COMPILER_ID STREQUAL "GNU"
       OR CMAKE_C_COMPILER_ID STREQUAL "Clang"
       OR CMAKE_C_COMPILER_ID STREQUAL "AppleClang")
  set(BLAKE3_CFLAGS_SSE2 "-msse2" CACHE STRING "the compiler flags to enable SSE2")
  set(BLAKE3_CFLAGS_SSE4.1 "-msse4.1" CACHE STRING "the compiler flags to enable SSE4.1")
  set(BLAKE3_CFLAGS_AVX2 "-mavx2" CACHE STRING "the compiler flags to enable AVX2")
  set(BLAKE3_CFLAGS_AVX512 "-mavx512f -mavx512vl" CACHE STRING "the compiler flags to enable AVX512")
endif()
# architecture lists for which to enable assembly / SIMD sources
set(BLAKE3_AMD64_NAMES amd64 AMD64 x86_64)
set(BLAKE3_X86_NAMES i686 x86 X86)
set(BLAKE3_ARMv8_NAMES aarch64 AArch64 arm64 ARM64 armv8 armv8a)

# library target
add_library(blake3
  blake3.c
  blake3_dispatch.c
  blake3_portable.c
)
add_library(BLAKE3::blake3 ALIAS blake3)

# library configuration
set(BLAKE3_PKGCONFIG_CFLAGS)
if (BUILD_SHARED_LIBS)
  target_compile_definitions(blake3 
    PUBLIC BLAKE3_DLL
    PRIVATE BLAKE3_DLL_EXPORTS
  )
  list(APPEND BLAKE3_PKGCONFIG_CFLAGS -DBLAKE3_DLL)
endif()
target_include_directories(blake3 PUBLIC $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>)
set_target_properties(blake3 PROPERTIES
  VERSION ${PROJECT_VERSION}
  SOVERSION 0
  C_VISIBILITY_PRESET hidden
)

# optional SIMD sources
macro(BLAKE3_DISABLE_SIMD)
  set(BLAKE3_SIMD_AMD64_ASM OFF)
  set(BLAKE3_SIMD_X86_INTRINSICS OFF)
  set(BLAKE3_SIMD_NEON_INTRINSICS OFF)
  set_source_files_properties(blake3_dispatch.c PROPERTIES 
    COMPILE_DEFINITIONS BLAKE3_USE_NEON=0;BLAKE3_NO_SSE2;BLAKE3_NO_SSE41;BLAKE3_NO_AVX2;BLAKE3_NO_AVX512
  )
endmacro()

if(CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_AMD64_NAMES OR BLAKE3_USE_AMD64_ASM)
  set(BLAKE3_SIMD_AMD64_ASM ON)

  if(MSVC)
    enable_language(ASM_MASM)
    target_sources(blake3 PRIVATE
      blake3_avx2_x86-64_windows_msvc.asm
      blake3_avx512_x86-64_windows_msvc.asm
      blake3_sse2_x86-64_windows_msvc.asm
      blake3_sse41_x86-64_windows_msvc.asm
    )

  elseif(CMAKE_C_COMPILER_ID STREQUAL "GNU"
         OR CMAKE_C_COMPILER_ID STREQUAL "Clang"
         OR CMAKE_C_COMPILER_ID STREQUAL "AppleClang")
    if (WIN32)
      target_sources(blake3 PRIVATE
        blake3_avx2_x86-64_windows_gnu.S
        blake3_avx512_x86-64_windows_gnu.S
        blake3_sse2_x86-64_windows_gnu.S
        blake3_sse41_x86-64_windows_gnu.S
      )

    elseif(UNIX)
      target_sources(blake3 PRIVATE
        blake3_avx2_x86-64_unix.S
        blake3_avx512_x86-64_unix.S
        blake3_sse2_x86-64_unix.S
        blake3_sse41_x86-64_unix.S
      )

    else()
      BLAKE3_DISABLE_SIMD()
    endif()

  else()  
    BLAKE3_DISABLE_SIMD()
  endif()

elseif((CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_X86_NAMES OR BLAKE3_USE_X86_INTRINSICS)
       AND DEFINED BLAKE3_CFLAGS_SSE2
       AND DEFINED BLAKE3_CFLAGS_SSE4.1
       AND DEFINED BLAKE3_CFLAGS_AVX2
       AND DEFINED BLAKE3_CFLAGS_AVX512)
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

elseif(CMAKE_SYSTEM_PROCESSOR IN_LIST BLAKE3_ARMv8_NAMES
        OR ((ANDROID_ABI STREQUAL "armeabi-v7a"
            OR BLAKE3_USE_NEON_INTRINSICS)
          AND (DEFINED BLAKE3_CFLAGS_NEON
            OR CMAKE_SIZEOF_VOID_P EQUAL 8)))
  set(BLAKE3_SIMD_NEON_INTRINSICS ON)

  target_sources(blake3 PRIVATE
    blake3_neon.c
  )
  target_compile_options(blake3 PRIVATE -DBLAKE3_USE_NEON=1)

  check_c_compiler_flag(-mfpu=neon BLAKE3_MFPU_NEON_SUPPORTED)
  if (BLAKE3_MFPU_NEON_SUPPORTED)
    target_compile_options(blake3 PRIVATE -mfpu=neon)
  endif()

  if (DEFINED BLAKE3_CFLAGS_NEON)
    set_source_files_properties(blake3_neon.c PROPERTIES COMPILE_FLAGS "${BLAKE3_CFLAGS_NEON}")
  endif()

else()
  BLAKE3_DISABLE_SIMD()
endif()

# cmake install support
install(FILES blake3.h DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
install(TARGETS blake3 EXPORT blake3-targets)
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

configure_file(libblake3.pc.in libblake3.pc @ONLY)
install(FILES "${CMAKE_BINARY_DIR}/libblake3.pc"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/pkgconfig")

# print feature summary
add_feature_info("AMD64 assembly" BLAKE3_SIMD_AMD64_ASM "The library uses hand written amd64 SIMD assembly.")
add_feature_info("x86 SIMD intrinsics" BLAKE3_SIMD_X86_INTRINSICS "The library uses x86 SIMD intrinsics.")
add_feature_info("NEON SIMD intrinsics" BLAKE3_SIMD_NEON_INTRINSICS "The library uses NEON SIMD intrinsics.")
feature_summary(WHAT ENABLED_FEATURES)
