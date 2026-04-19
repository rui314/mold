/*
    Copyright (c) 2025 Intel Corporation

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

#if __cplusplus >= 202002L

/*begin_parallel_sort_ranges_extension_example*/
#include <array>
#include <span> // requires C++20
#include <oneapi/tbb/parallel_sort.h>

std::span<int> get_span() {
    static std::array<int, 3> arr = {3, 2, 1};
    return std::span<int>(arr);
}

int main() {
    tbb::parallel_sort(get_span());
}
/*end_parallel_sort_ranges_extension_example*/

#else
// Skip
int main() {}
#endif
