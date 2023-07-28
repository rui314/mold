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

# Add code signing as post-install step.
if (DEFINED TBB_SIGNTOOL)
    file(TO_CMAKE_PATH "${TBB_SIGNTOOL}" TBB_SIGNTOOL)
    install(CODE "
    file(GLOB_RECURSE FILES_TO_SIGN \${CMAKE_INSTALL_PREFIX}/*${CMAKE_SHARED_LIBRARY_SUFFIX})
    execute_process(COMMAND ${TBB_SIGNTOOL} \${FILES_TO_SIGN} ${TBB_SIGNTOOL_ARGS})
")
endif()
