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

# Note: current implementation uses CMAKE_BUILD_TYPE,
# this parameter is not defined for multi-config generators.
set(CPACK_PACKAGE_NAME "${PROJECT_NAME}")
set(CPACK_PACKAGE_VERSION "${TBB_VERSION}")
string(TOLOWER ${CPACK_PACKAGE_NAME}-${PROJECT_VERSION}-${CMAKE_SYSTEM_NAME}_${TBB_OUTPUT_DIR_BASE}_${CMAKE_BUILD_TYPE} CPACK_PACKAGE_FILE_NAME)
set(CPACK_GENERATOR ZIP)
# Note: this is an internal non-documented variable set by CPack 
if (NOT CPack_CMake_INCLUDED)
    include(CPack)
endif()
