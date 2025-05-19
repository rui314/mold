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

#define TBB_PREVIEW_BLOCKED_ND_RANGE_DEDUCTION_GUIDES 1
#include <oneapi/tbb/blocked_nd_range.h>

int main() {
    {
        /*begin_blocked_nd_range_ctad_example_1*/
        oneapi::tbb::blocked_range<int> range1(0, 100);
        oneapi::tbb::blocked_range<int> range2(0, 200);
        oneapi::tbb::blocked_range<int> range3(0, 300);

        // Since 3 unidimensional ranges of type int are provided, the type of nd_range
        // can be deduced as oneapi::tbb::blocked_nd_range<int, 3>
        oneapi::tbb::blocked_nd_range nd_range(range1, range2, range3);
        /*end_blocked_nd_range_ctad_example_1*/
    }
    /*begin_blocked_nd_range_ctad_example_2*/
    {
        oneapi::tbb::blocked_range<int> range1(0, 100);
        oneapi::tbb::blocked_range<int> range2(0, 200);

        // Deduced as blocked_nd_range<int, 2>
        oneapi::tbb::blocked_nd_range nd_range(range1, range2);
    }
    {
        // Deduced as blocked_nd_range<int, 2>
        oneapi::tbb::blocked_nd_range nd_range({0, 100}, {0, 200, 5});
    }
    {
        int endings[3] = {100, 200, 300};

        // Deduced as blocked_nd_range<int, 3>
        oneapi::tbb::blocked_nd_range nd_range1(endings);

        // Deduced as blocked_nd_range<int, 3>
        oneapi::tbb::blocked_nd_range nd_range2({100, 200, 300}, /*grainsize = */10);
    }
    /*end_blocked_nd_range_ctad_example_2*/
}

#else 
// Skip
int main() {}
#endif
