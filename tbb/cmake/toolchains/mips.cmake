# Copyright (c) 2020-2021 Intel Corporation
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

# Prevent double invocation.
if (MIPS_TOOLCHAIN_INCLUDED)
    return()
endif()
set(MIPS_TOOLCHAIN_INCLUDED TRUE)

set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_VERSION 1)
set(CMAKE_SYSTEM_PROCESSOR mips)

set(CMAKE_C_COMPILER ${CMAKE_FIND_ROOT_PATH}/bin/mips-img-linux-gnu-gcc)
set(CMAKE_CXX_COMPILER ${CMAKE_FIND_ROOT_PATH}/bin/mips-img-linux-gnu-g++)
set(CMAKE_LINKER ${CMAKE_FIND_ROOT_PATH}/bin/mips-img-linux-gnu-ld)

# Define result for try_run used in find_package(Threads).
# In old CMake versions (checked on 3.5) there is invocation of try_run command in FindThreads.cmake module.
# It can't be executed on host system in case of cross-compilation for MIPS architecture.
# Define return code for this try_run as 0 since threads are expected to be available on target machine.
set(THREADS_PTHREAD_ARG "0" CACHE STRING "Result from TRY_RUN" FORCE)

set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)

set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -EL -mabi=64 -march=mips64r6 -mcrc -mfp64 -mmt -mtune=mips64r6 -ggdb -ffp-contract=off -mhard-float" CACHE INTERNAL "")
set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} -mvirt -mxpa" CACHE INTERNAL "")
set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -mvirt -mxpa" CACHE INTERNAL "")  # for tests
