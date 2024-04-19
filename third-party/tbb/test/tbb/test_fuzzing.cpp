/*
    Copyright (c) 2023 Intel Corporation

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

//! \file test_fuzzing.cpp
//! \brief Test the [fuzzing] of environment variables

#include <fuzzer/FuzzedDataProvider.h>

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
  FuzzedDataProvider provider(data, size);
  for (auto var : {"INTEL_ITTNOTIFY_GROUPS", "INTEL_LIBITTNOTIFY32",
                   "INTEL_LIBITTNOTIFY64", "KMP_FOR_TCHECK", "KMP_FOR_TPROFILE",
                   "TBB_ENABLE_SANITIZERS", "TBB_MALLOC_DISABLE_REPLACEMENT",
                   "TBB_MALLOC_SET_HUGE_SIZE_THRESHOLD",
                   "TBB_MALLOC_USE_HUGE_PAGES", "TBB_VERSION"}) {
    std::string val = provider.ConsumeRandomLengthString();
#if _WIN32
    _putenv_s(var, val.c_str());
#else
    setenv(var, val.c_str(), 1);
#endif
  }

  if (std::system(CMD) != 0)
    __builtin_trap();

  return 0;
}
