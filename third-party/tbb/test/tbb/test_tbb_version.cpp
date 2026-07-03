/*
    Copyright (c) 2025 UXL Foundation Contributors

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

//! Test for the availability of extensions
//! \file test_tbb_version.cpp
//! \brief Test for [configuration.feature_macros] specification

#include "tbb/version.h"
// checking that inclusion of version.h is enough to get TBB_EXT_CUSTOM_ASSERTION_HANDLER
#if TBB_EXT_CUSTOM_ASSERTION_HANDLER != 202510
    #error "TBB_EXT_CUSTOM_ASSERTION_HANDLER must be set to 202510"
#endif

int main() {
    return 0;
}
