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

#if __cplusplus >= 201703L

/*begin_rvalue_reduce_example*/
// C++17
#include <oneapi/tbb/parallel_reduce.h>
#include <oneapi/tbb/blocked_range.h>
#include <vector>
#include <set>

int main() {
    std::vector<std::set<int>> sets;

    oneapi::tbb::parallel_reduce(oneapi::tbb::blocked_range<size_t>(0, sets.size()),
                                    std::set<int>{}, // identity element - empty set
                                    [&](const oneapi::tbb::blocked_range<size_t>& range, std::set<int>&& value) {
                                        for (size_t i = range.begin(); i < range.end(); ++i) {
                                            // Having value as a non-const rvalue reference allows to efficiently
                                            // transfer nodes from sets[i] without copying/moving the data
                                            value.merge(std::move(sets[i]));
                                        }
                                        return value;
                                    },
                                    [&](std::set<int>&& x, std::set<int>&& y) {
                                        x.merge(std::move(y));
                                        return x;
                                    }
                                    );
}
/*end_rvalue_reduce_example*/

#else
// Skip
int main() {}
#endif
