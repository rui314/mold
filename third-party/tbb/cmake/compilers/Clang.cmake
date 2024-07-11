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

if (EMSCRIPTEN)
  set(TBB_EMSCRIPTEN 1)
  set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -fexceptions)
  set(TBB_TEST_LINK_FLAGS  ${TBB_COMMON_LINK_FLAGS} -fexceptions -sINITIAL_MEMORY=65536000 -sALLOW_MEMORY_GROWTH=1 -sEXIT_RUNTIME=1)
  if (NOT EMSCRIPTEN_WITHOUT_PTHREAD)
      set_property(TARGET Threads::Threads PROPERTY INTERFACE_LINK_LIBRARIES "-pthread")
  endif()
endif()

if (MINGW)
    set(TBB_LINK_DEF_FILE_FLAG "")
    set(TBB_DEF_FILE_PREFIX "")
elseif (APPLE)
    set(TBB_LINK_DEF_FILE_FLAG -Wl,-exported_symbols_list,)
    set(TBB_DEF_FILE_PREFIX mac${TBB_ARCH})

    # For correct ucontext.h structures layout
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -D_XOPEN_SOURCE)
elseif (MSVC)
    include(${CMAKE_CURRENT_LIST_DIR}/MSVC.cmake)
    return()
else()
    set(TBB_LINK_DEF_FILE_FLAG -Wl,--version-script=)
    set(TBB_DEF_FILE_PREFIX lin${TBB_ARCH})
    set(TBB_TEST_COMPILE_FLAGS ${TBB_TEST_COMPILE_FLAGS} $<$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},10.0>>:-ffp-model=precise>)
endif()

# Depfile options (e.g. -MD) are inserted automatically in some cases.
# Don't add -MMD to avoid conflicts in such cases.
if (NOT CMAKE_GENERATOR MATCHES "Ninja" AND NOT CMAKE_CXX_DEPENDS_USE_COMPILER)
    set(TBB_MMD_FLAG -MMD)
endif()

set(TBB_WARNING_LEVEL -Wall -Wextra $<$<BOOL:${TBB_STRICT}>:-Werror>)
set(TBB_TEST_WARNING_FLAGS -Wshadow -Wcast-qual -Woverloaded-virtual -Wnon-virtual-dtor)

# Ignore -Werror set through add_compile_options() or added to CMAKE_CXX_FLAGS if TBB_STRICT is disabled.
if (NOT TBB_STRICT AND COMMAND tbb_remove_compile_flag)
    tbb_remove_compile_flag(-Werror)
endif()

# Enable Intel(R) Transactional Synchronization Extensions (-mrtm) and WAITPKG instructions support (-mwaitpkg) on relevant processors
if (CMAKE_SYSTEM_PROCESSOR MATCHES "(AMD64|amd64|i.86|x86)" AND NOT EMSCRIPTEN)
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -mrtm $<$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},12.0>>:-mwaitpkg>)
endif()

# Clang flags to prevent compiler from optimizing out security checks
set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -Wformat -Wformat-security -Werror=format-security -fPIC $<$<NOT:$<BOOL:${EMSCRIPTEN}>>:-fstack-protector-strong>)

# -z switch is not supported on MacOS
if (NOT APPLE)
    set(TBB_LIB_LINK_FLAGS ${TBB_LIB_LINK_FLAGS} -Wl,-z,relro,-z,now)
endif()

set(TBB_COMMON_LINK_LIBS ${CMAKE_DL_LIBS})

if (NOT CMAKE_CXX_FLAGS MATCHES "_FORTIFY_SOURCE")
  set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} $<$<NOT:$<CONFIG:Debug>>:-D_FORTIFY_SOURCE=2>)
endif ()

if (MINGW)
    list(APPEND TBB_COMMON_COMPILE_FLAGS -U__STRICT_ANSI__)
endif()

set(TBB_IPO_COMPILE_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)
set(TBB_IPO_LINK_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)

# TBB malloc settings
set(TBBMALLOC_LIB_COMPILE_FLAGS -fno-rtti -fno-exceptions)
