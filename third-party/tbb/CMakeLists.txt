# Copyright (c) 2020-2023 Intel Corporation
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

cmake_minimum_required(VERSION 3.1)

# Enable CMake policies

if (POLICY CMP0091)
    # The NEW behavior for this policy is to not place MSVC runtime library flags in the default
    # CMAKE_<LANG>_FLAGS_<CONFIG> cache entries and use CMAKE_MSVC_RUNTIME_LIBRARY abstraction instead.
    cmake_policy(SET CMP0091 NEW)
elseif (DEFINED CMAKE_MSVC_RUNTIME_LIBRARY)
    message(FATAL_ERROR "CMAKE_MSVC_RUNTIME_LIBRARY was defined while policy CMP0091 is not available. Use CMake 3.15 or newer.")
endif()

if (TBB_WINDOWS_DRIVER AND (NOT ("${CMAKE_MSVC_RUNTIME_LIBRARY}" STREQUAL MultiThreaded OR "${CMAKE_MSVC_RUNTIME_LIBRARY}" STREQUAL MultiThreadedDebug)))
    message(FATAL_ERROR "Enabled TBB_WINDOWS_DRIVER requires CMAKE_MSVC_RUNTIME_LIBRARY to be set to MultiThreaded or MultiThreadedDebug.")
endif()

# Enable support of minimum supported macOS version flag
if (APPLE)
    if (NOT CMAKE_CXX_OSX_DEPLOYMENT_TARGET_FLAG)
        set(CMAKE_CXX_OSX_DEPLOYMENT_TARGET_FLAG "-mmacosx-version-min=" CACHE STRING "Minimum macOS version flag")
    endif()
    if (NOT CMAKE_C_OSX_DEPLOYMENT_TARGET_FLAG)
        set(CMAKE_C_OSX_DEPLOYMENT_TARGET_FLAG "-mmacosx-version-min=" CACHE STRING "Minimum macOS version flag")
    endif()
endif()

# Until CMake 3.4.0 FindThreads.cmake requires C language enabled.
# Enable C language before CXX to avoid possible override of CMAKE_SIZEOF_VOID_P.
if (CMAKE_VERSION VERSION_LESS 3.4)
    enable_language(C)
endif()

file(READ include/oneapi/tbb/version.h _tbb_version_info)
string(REGEX REPLACE ".*#define TBB_VERSION_MAJOR ([0-9]+).*" "\\1" _tbb_ver_major "${_tbb_version_info}")
string(REGEX REPLACE ".*#define TBB_VERSION_MINOR ([0-9]+).*" "\\1" _tbb_ver_minor "${_tbb_version_info}")
string(REGEX REPLACE ".*#define TBB_VERSION_PATCH ([0-9]+).*" "\\1" _tbb_ver_patch "${_tbb_version_info}")
string(REGEX REPLACE ".*#define TBB_INTERFACE_VERSION ([0-9]+).*" "\\1" TBB_INTERFACE_VERSION "${_tbb_version_info}")
string(REGEX REPLACE ".*#define __TBB_BINARY_VERSION ([0-9]+).*" "\\1" TBB_BINARY_VERSION "${_tbb_version_info}")
set(TBB_BINARY_MINOR_VERSION ${_tbb_ver_minor})
set(TBBMALLOC_BINARY_VERSION 2)
set(TBBBIND_BINARY_VERSION 3)

project(TBB VERSION ${_tbb_ver_major}.${_tbb_ver_minor}.${_tbb_ver_patch} LANGUAGES CXX)
unset(_tbb_ver_major)
unset(_tbb_ver_minor)

include(CheckCXXCompilerFlag)
include(GNUInstallDirs)
include(CMakeDependentOption)

# ---------------------------------------------------------------------------------------------------------
# Handle C++ standard version.
if (NOT MSVC)  # no need to cover MSVC as it uses C++14 by default.
    if (NOT CMAKE_CXX_STANDARD)
        set(CMAKE_CXX_STANDARD 11)
    endif()

    if (CMAKE_CXX${CMAKE_CXX_STANDARD}_STANDARD_COMPILE_OPTION)  # if standard option was detected by CMake
        set(CMAKE_CXX_STANDARD_REQUIRED ON)
    else()  # if standard option wasn't detected by CMake (e.g. for Intel Compiler with CMake 3.1)
        # TBB_CXX_STD_FLAG should be added to targets via target_compile_options
        set(TBB_CXX_STD_FLAG -std=c++${CMAKE_CXX_STANDARD})

        check_cxx_compiler_flag(${TBB_CXX_STD_FLAG} c++${CMAKE_CXX_STANDARD})
        if (NOT c++${CMAKE_CXX_STANDARD})
            message(FATAL_ERROR "C++${CMAKE_CXX_STANDARD} (${TBB_CXX_STD_FLAG}) support is required")
        endif()
        unset(c++${CMAKE_CXX_STANDARD})
    endif()
endif()

set(CMAKE_CXX_EXTENSIONS OFF) # use -std=c++... instead of -std=gnu++...
# ---------------------------------------------------------------------------------------------------------

# Detect architecture (bitness).
if (CMAKE_SIZEOF_VOID_P EQUAL 4)
    set(TBB_ARCH 32)
else()
    set(TBB_ARCH 64)
endif()

option(TBB_TEST "Enable testing" ON)
option(TBB_EXAMPLES "Enable examples" OFF)
option(TBB_STRICT "Treat compiler warnings as errors" ON)
option(TBB_WINDOWS_DRIVER "Build as Universal Windows Driver (UWD)" OFF)
option(TBB_NO_APPCONTAINER "Apply /APPCONTAINER:NO (for testing binaries for Windows Store)" OFF)
option(TBB4PY_BUILD "Enable tbb4py build" OFF)
option(TBB_BUILD "Enable tbb build" ON)
option(TBBMALLOC_BUILD "Enable tbbmalloc build" ON)
cmake_dependent_option(TBBMALLOC_PROXY_BUILD "Enable tbbmalloc_proxy build" ON "TBBMALLOC_BUILD" OFF)
option(TBB_CPF "Enable preview features of the library" OFF)
option(TBB_FIND_PACKAGE "Enable search for external oneTBB using find_package instead of build from sources" OFF)
option(TBB_DISABLE_HWLOC_AUTOMATIC_SEARCH "Disable HWLOC automatic search by pkg-config tool" OFF)
option(TBB_ENABLE_IPO "Enable Interprocedural Optimization (IPO) during the compilation" ON)
option(TBB_FUZZ_TESTING "Enable fuzz testing" OFF)

if (NOT DEFINED BUILD_SHARED_LIBS)
    set(BUILD_SHARED_LIBS ON)
endif()

if (NOT BUILD_SHARED_LIBS)
    if(NOT DEFINED CMAKE_POSITION_INDEPENDENT_CODE)
        set(CMAKE_POSITION_INDEPENDENT_CODE ON)
    endif()
    message(WARNING "You are building oneTBB as a static library. This is highly discouraged and such configuration is not supported. Consider building a dynamic library to avoid unforeseen issues.")
endif()

if (NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE RelWithDebInfo CACHE STRING "Build type" FORCE)
    message(STATUS "CMAKE_BUILD_TYPE is not specified. Using default: ${CMAKE_BUILD_TYPE}")
    # Possible values of build type for cmake-gui
    set_property(CACHE CMAKE_BUILD_TYPE PROPERTY STRINGS "Debug" "Release" "MinSizeRel" "RelWithDebInfo")
endif()

if (CMAKE_BUILD_TYPE)
    string(TOLOWER ${CMAKE_BUILD_TYPE} _tbb_build_type)
    if (_tbb_build_type STREQUAL "debug")
        set(TBB_ENABLE_IPO OFF)
    endif()
    unset(_tbb_build_type)
endif()

# -------------------------------------------------------------------
# Files and folders naming
set(CMAKE_DEBUG_POSTFIX _debug)

if (NOT DEFINED TBB_OUTPUT_DIR_BASE)
    if (MSVC)
        if (NOT DEFINED CMAKE_MSVC_RUNTIME_LIBRARY OR CMAKE_MSVC_RUNTIME_LIBRARY MATCHES DLL)
            set(_tbb_msvc_runtime _md)
        else()
            set(_tbb_msvc_runtime _mt)
        endif()

        if (WINDOWS_STORE)
            if (TBB_NO_APPCONTAINER)
                set(_tbb_win_store _wsnoappcont)
            else()
                set(_tbb_win_store _ws)
            endif()
        elseif(TBB_WINDOWS_DRIVER)
            set(_tbb_win_store _wd)
        endif()
    endif()

     string(REGEX MATCH "^([0-9]+\.[0-9]+|[0-9]+)" _tbb_compiler_version_short ${CMAKE_CXX_COMPILER_VERSION})
     string(TOLOWER ${CMAKE_CXX_COMPILER_ID}_${_tbb_compiler_version_short}_cxx${CMAKE_CXX_STANDARD}_${TBB_ARCH}${_tbb_msvc_runtime}${_tbb_win_store} TBB_OUTPUT_DIR_BASE)
     unset(_tbb_msvc_runtime)
     unset(_tbb_win_store)
     unset(_tbb_compiler_version_short)
endif()

foreach(output_type LIBRARY ARCHIVE PDB RUNTIME)
    if (CMAKE_BUILD_TYPE)
        string(TOLOWER ${CMAKE_BUILD_TYPE} _tbb_build_type_lower)
        set(CMAKE_${output_type}_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/${TBB_OUTPUT_DIR_BASE}_${_tbb_build_type_lower})
        unset(_tbb_build_type_lower)
    endif()

    if (CMAKE_CONFIGURATION_TYPES)
        foreach(suffix ${CMAKE_CONFIGURATION_TYPES})
            string(TOUPPER ${suffix} _tbb_suffix_upper)
            string(TOLOWER ${suffix} _tbb_suffix_lower)
            set(CMAKE_${output_type}_OUTPUT_DIRECTORY_${_tbb_suffix_upper} ${CMAKE_BINARY_DIR}/${TBB_OUTPUT_DIR_BASE}_${_tbb_suffix_lower})
        endforeach()
        unset(_tbb_suffix_lower)
        unset(_tbb_suffix_upper)
    endif()
endforeach()

if (CMAKE_CONFIGURATION_TYPES)
    # We can't use generator expressions in a cmake variable name.
    set(TBB_TEST_WORKING_DIRECTORY ${CMAKE_BINARY_DIR}/${TBB_OUTPUT_DIR_BASE}_$<LOWER_CASE:$<CONFIG>>)
else()
    set(TBB_TEST_WORKING_DIRECTORY ${CMAKE_RUNTIME_OUTPUT_DIRECTORY})
endif()

# -------------------------------------------------------------------

# -------------------------------------------------------------------
# Common dependencies
#force -pthread during compilation for Emscripten
if (EMSCRIPTEN)
   set(THREADS_HAVE_PTHREAD_ARG TRUE)
endif()

set(THREADS_PREFER_PTHREAD_FLAG TRUE)
find_package(Threads REQUIRED)
# -------------------------------------------------------------------

file(GLOB FILES_WITH_EXTRA_TARGETS ${CMAKE_CURRENT_SOURCE_DIR}/cmake/*.cmake)
foreach(FILE_WITH_EXTRA_TARGETS ${FILES_WITH_EXTRA_TARGETS})
    include(${FILE_WITH_EXTRA_TARGETS})
endforeach()

# - Enabling LTO on Android causes the NDK bug.
#   NDK throws the warning: "argument unused during compilation: '-Wa,--noexecstack'"
# - For some reason GCC does not instrument code with Thread Sanitizer when lto is enabled and C linker is used.
if (TBB_ENABLE_IPO AND BUILD_SHARED_LIBS AND NOT ANDROID_PLATFORM AND NOT TBB_SANITIZE MATCHES "thread")
    if (NOT CMAKE_VERSION VERSION_LESS 3.9)
        cmake_policy(SET CMP0069 NEW)
        include(CheckIPOSupported)
        check_ipo_supported(RESULT TBB_IPO_PROPERTY)
    else()
        set(TBB_IPO_FLAGS TRUE)
    endif()
    if (TBB_IPO_PROPERTY OR TBB_IPO_FLAGS)
        message(STATUS "IPO enabled")
    endif()
endif()

set(TBB_COMPILER_SETTINGS_FILE ${CMAKE_CURRENT_SOURCE_DIR}/cmake/compilers/${CMAKE_CXX_COMPILER_ID}.cmake)
if (EXISTS ${TBB_COMPILER_SETTINGS_FILE})
    include(${TBB_COMPILER_SETTINGS_FILE})
else()
    message(WARNING "TBB compiler settings not found ${TBB_COMPILER_SETTINGS_FILE}")
endif()

if (TBB_FIND_PACKAGE OR TBB_DIR)
    # Allow specifying external TBB to test with.
    # Do not add main targets and installation instructions in that case.
    message(STATUS "Using external TBB for testing")
    find_package(TBB REQUIRED)
else()
    if (TBB_BUILD)
        add_subdirectory(src/tbb)
    endif()
    if (TBBMALLOC_BUILD)
        add_subdirectory(src/tbbmalloc)
        if(TBBMALLOC_PROXY_BUILD AND NOT "${MSVC_CXX_ARCHITECTURE_ID}" MATCHES "ARM64")
            add_subdirectory(src/tbbmalloc_proxy)
        endif()
    endif()
    if (NOT BUILD_SHARED_LIBS)
        message(STATUS "TBBBind build targets are disabled due to unsupported environment")
    else()
        add_subdirectory(src/tbbbind)
    endif()

    # -------------------------------------------------------------------
    # Installation instructions
    include(CMakePackageConfigHelpers)

    install(DIRECTORY include/
            DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}
            COMPONENT devel)

    install(EXPORT ${PROJECT_NAME}Targets
            NAMESPACE TBB::
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/${PROJECT_NAME}
            COMPONENT devel)
    file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/${PROJECT_NAME}Config.cmake
               "include(\${CMAKE_CURRENT_LIST_DIR}/${PROJECT_NAME}Targets.cmake)\n")

    write_basic_package_version_file("${CMAKE_CURRENT_BINARY_DIR}/${PROJECT_NAME}ConfigVersion.cmake"
                                     COMPATIBILITY AnyNewerVersion)

    install(FILES "${CMAKE_CURRENT_BINARY_DIR}/${PROJECT_NAME}Config.cmake"
                  "${CMAKE_CURRENT_BINARY_DIR}/${PROJECT_NAME}ConfigVersion.cmake"
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/${PROJECT_NAME}
            COMPONENT devel)

    install(FILES "README.md"
            DESTINATION ${CMAKE_INSTALL_DOCDIR}
            COMPONENT devel)
    # -------------------------------------------------------------------
endif()

if (TBB_TEST)
    enable_testing()
    add_subdirectory(test)
endif()

if (TBB_EXAMPLES)
    add_subdirectory(examples)
endif()

if (TBB_BENCH)
    if (NOT EXISTS ${CMAKE_CURRENT_LIST_DIR}/benchmark)
        message(FATAL_ERROR "Benchmarks are not supported yet")
    endif()

    enable_testing()
    add_subdirectory(benchmark)
endif()

if (ANDROID_PLATFORM)
    if ("${ANDROID_STL}" STREQUAL "c++_shared")
        if (${ANDROID_NDK_MAJOR} GREATER_EQUAL "25")
            if(ANDROID_ABI STREQUAL "arm64-v8a")
                set(ANDROID_TOOLCHAIN_NAME "aarch64-linux-android")
            elseif(ANDROID_ABI STREQUAL "x86_64")
                set(ANDROID_TOOLCHAIN_NAME "x86_64-linux-android")
            elseif(ANDROID_ABI STREQUAL "armeabi-v7a")
                set(ANDROID_TOOLCHAIN_NAME "arm-linux-androideabi")
            elseif(ANDROID_ABI STREQUAL "x86")
                set(ANDROID_TOOLCHAIN_NAME "i686-linux-android")
            endif()

            configure_file(
            "${ANDROID_TOOLCHAIN_ROOT}/sysroot/usr/lib/${ANDROID_TOOLCHAIN_NAME}/libc++_shared.so"
            "${CMAKE_LIBRARY_OUTPUT_DIRECTORY}/libc++_shared.so"
            COPYONLY)
        else()
            configure_file(
                "${ANDROID_NDK}/sources/cxx-stl/llvm-libc++/libs/${ANDROID_ABI}/libc++_shared.so"
                "${CMAKE_LIBRARY_OUTPUT_DIRECTORY}/libc++_shared.so"
                COPYONLY)
        endif()
    endif()
    # This custom target may be implemented without separate CMake script, but it requires
    # ADB(Android Debug Bridge) executable file availability, so to incapsulate this requirement
    # only for corresponding custom target, it was implemented by this way.
    add_custom_target(device_environment_cleanup COMMAND ${CMAKE_COMMAND}
                      -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/android/device_environment_cleanup.cmake)
endif()

if (TBB4PY_BUILD)
    add_subdirectory(python)
endif()

# Keep it the last instruction.
add_subdirectory(cmake/post_install)
