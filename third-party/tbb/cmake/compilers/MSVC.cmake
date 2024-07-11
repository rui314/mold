# Copyright (c) 2020-2024 Intel Corporation
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

set(TBB_LINK_DEF_FILE_FLAG ${CMAKE_LINK_DEF_FILE_FLAG})
set(TBB_DEF_FILE_PREFIX win${TBB_ARCH})

# Workaround for CMake issue https://gitlab.kitware.com/cmake/cmake/issues/18317.
# TODO: consider use of CMP0092 CMake policy.
string(REGEX REPLACE "/W[0-4]" "" CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}")

set(TBB_WARNING_LEVEL $<$<NOT:$<CXX_COMPILER_ID:Intel>>:/W4> $<$<BOOL:${TBB_STRICT}>:/WX>)

# Warning suppression C4324: structure was padded due to alignment specifier
set(TBB_WARNING_SUPPRESS /wd4324)

set(TBB_TEST_COMPILE_FLAGS ${TBB_TEST_COMPILE_FLAGS} /bigobj)
if (MSVC_VERSION LESS_EQUAL 1900)
    # Warning suppression C4503 for VS2015 and earlier:
    # decorated name length exceeded, name was truncated.
    # More info can be found at
    # https://docs.microsoft.com/en-us/cpp/error-messages/compiler-warnings/compiler-warning-level-1-c4503
    set(TBB_TEST_COMPILE_FLAGS ${TBB_TEST_COMPILE_FLAGS} /wd4503)
endif()
set(TBB_LIB_COMPILE_FLAGS -D_CRT_SECURE_NO_WARNINGS /GS)
set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} /volatile:iso /FS /EHsc)

set(TBB_LIB_LINK_FLAGS ${TBB_LIB_LINK_FLAGS} /DEPENDENTLOADFLAG:0x2000 /DYNAMICBASE /NXCOMPAT)

if (TBB_ARCH EQUAL 32)
    set(TBB_LIB_LINK_FLAGS ${TBB_LIB_LINK_FLAGS} /SAFESEH )
endif()

# Ignore /WX set through add_compile_options() or added to CMAKE_CXX_FLAGS if TBB_STRICT is disabled.
if (NOT TBB_STRICT AND COMMAND tbb_remove_compile_flag)
    tbb_remove_compile_flag(/WX)
endif()

if (WINDOWS_STORE OR TBB_WINDOWS_DRIVER)
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} /D_WIN32_WINNT=0x0A00)
    set(TBB_COMMON_LINK_FLAGS -NODEFAULTLIB:kernel32.lib -INCREMENTAL:NO)
    set(TBB_COMMON_LINK_LIBS OneCore.lib)
endif()

if (WINDOWS_STORE)
    if (NOT CMAKE_SYSTEM_VERSION EQUAL 10.0)
        message(FATAL_ERROR "CMAKE_SYSTEM_VERSION must be equal to 10.0")
    endif()

    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} /ZW /ZW:nostdlib)

    # CMake define this extra lib, remove it for this build type
    string(REGEX REPLACE "WindowsApp.lib" "" CMAKE_CXX_STANDARD_LIBRARIES "${CMAKE_CXX_STANDARD_LIBRARIES}")

    if (TBB_NO_APPCONTAINER)
        set(TBB_LIB_LINK_FLAGS ${TBB_LIB_LINK_FLAGS} -APPCONTAINER:NO)
    endif()
endif()

if (TBB_WINDOWS_DRIVER)
    # Since this is universal driver disable this variable
    set(CMAKE_SYSTEM_PROCESSOR "")

    # CMake define list additional libs, remove it for this build type
    set(CMAKE_CXX_STANDARD_LIBRARIES "")

    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} /D _UNICODE /DUNICODE /DWINAPI_FAMILY=WINAPI_FAMILY_APP /D__WRL_NO_DEFAULT_LIB__)
endif()

if (CMAKE_CXX_COMPILER_ID MATCHES "(Clang|IntelLLVM)")
    if (CMAKE_SYSTEM_PROCESSOR MATCHES "(x86|AMD64|i.86)")
        set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -mrtm -mwaitpkg)
    endif()
    set(TBB_IPO_COMPILE_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)
    set(TBB_IPO_LINK_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)
else()
    set(TBB_IPO_COMPILE_FLAGS $<$<NOT:$<CONFIG:Debug>>:/GL>)
    set(TBB_IPO_LINK_FLAGS $<$<NOT:$<CONFIG:Debug>>:-LTCG> $<$<NOT:$<CONFIG:Debug>>:-INCREMENTAL:NO>)
endif()

set(TBB_OPENMP_FLAG /openmp)
set(TBB_OPENMP_NO_LINK_FLAG TRUE) # TBB_OPENMP_FLAG will be used only on compilation but not on linkage
