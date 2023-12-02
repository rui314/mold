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

if (MINGW)
    set(TBB_LINK_DEF_FILE_FLAG "")
    set(TBB_DEF_FILE_PREFIX "")
elseif (APPLE)
    set(TBB_LINK_DEF_FILE_FLAG -Wl,-exported_symbols_list,)
    set(TBB_DEF_FILE_PREFIX mac${TBB_ARCH})

    # For correct ucontext.h structures layout
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -D_XOPEN_SOURCE)
else()
    set(TBB_LINK_DEF_FILE_FLAG -Wl,--version-script=)
    set(TBB_DEF_FILE_PREFIX lin${TBB_ARCH})
endif()

set(TBB_WARNING_LEVEL -Wall -Wextra $<$<BOOL:${TBB_STRICT}>:-Werror> -Wfatal-errors)
set(TBB_TEST_WARNING_FLAGS -Wshadow -Wcast-qual -Woverloaded-virtual -Wnon-virtual-dtor)

# Depfile options (e.g. -MD) are inserted automatically in some cases.
# Don't add -MMD to avoid conflicts in such cases.
if (NOT CMAKE_GENERATOR MATCHES "Ninja" AND NOT CMAKE_CXX_DEPENDS_USE_COMPILER)
    set(TBB_MMD_FLAG -MMD)
endif()

# Enable Intel(R) Transactional Synchronization Extensions (-mrtm) and WAITPKG instructions support (-mwaitpkg) on relevant processors
if (CMAKE_SYSTEM_PROCESSOR MATCHES "(AMD64|amd64|i.86|x86)")
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -mrtm $<$<AND:$<NOT:$<CXX_COMPILER_ID:Intel>>,$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},11.0>>>:-mwaitpkg>)
endif()

set(TBB_COMMON_LINK_LIBS ${CMAKE_DL_LIBS})

# Ignore -Werror set through add_compile_options() or added to CMAKE_CXX_FLAGS if TBB_STRICT is disabled.
if (NOT TBB_STRICT AND COMMAND tbb_remove_compile_flag)
    tbb_remove_compile_flag(-Werror)
endif()

if (NOT ${CMAKE_CXX_COMPILER_ID} STREQUAL Intel)
    # gcc 6.0 and later have -flifetime-dse option that controls elimination of stores done outside the object lifetime
    set(TBB_DSE_FLAG $<$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},6.0>>:-flifetime-dse=1>)
    set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} $<$<NOT:$<VERSION_LESS:${CMAKE_CXX_COMPILER_VERSION},8.0>>:-fstack-clash-protection>)
endif()

# Workaround for heavy tests and too many symbols in debug (rellocation truncated to fit: R_MIPS_CALL16)
if ("${CMAKE_SYSTEM_PROCESSOR}" MATCHES "mips")
    set(TBB_TEST_COMPILE_FLAGS ${TBB_TEST_COMPILE_FLAGS} -DTBB_TEST_LOW_WORKLOAD $<$<CONFIG:DEBUG>:-fPIE -mxgot>)
    set(TBB_TEST_LINK_FLAGS ${TBB_TEST_LINK_FLAGS} $<$<CONFIG:DEBUG>:-pie>)
endif()

set(TBB_IPO_COMPILE_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)
set(TBB_IPO_LINK_FLAGS $<$<NOT:$<CONFIG:Debug>>:-flto>)


if (MINGW AND CMAKE_SYSTEM_PROCESSOR MATCHES "i.86")
    list (APPEND TBB_COMMON_COMPILE_FLAGS -msse2)
endif ()

# Gnu flags to prevent compiler from optimizing out security checks
set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -fno-strict-overflow -fno-delete-null-pointer-checks -fwrapv)
set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} -Wformat -Wformat-security -Werror=format-security
    -fstack-protector-strong )
# -z switch is not supported on MacOS
if (NOT APPLE)
    set(TBB_LIB_LINK_FLAGS ${TBB_LIB_LINK_FLAGS} -Wl,-z,relro,-z,now,-z,noexecstack)
endif()
set(TBB_COMMON_COMPILE_FLAGS ${TBB_COMMON_COMPILE_FLAGS} $<$<NOT:$<CONFIG:Debug>>:-D_FORTIFY_SOURCE=2> )


# TBB malloc settings
set(TBBMALLOC_LIB_COMPILE_FLAGS -fno-rtti -fno-exceptions)
set(TBB_OPENMP_FLAG -fopenmp)
