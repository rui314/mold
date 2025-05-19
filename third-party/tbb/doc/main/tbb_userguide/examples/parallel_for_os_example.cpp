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

void Foo(float) {}

/*begin_parallel_for_os_1*/
#include "oneapi/tbb.h"

using namespace oneapi::tbb;

class ApplyFoo {
    float *const my_a;
public:
    void operator()( const blocked_range<size_t>& r ) const {
        float *a = my_a;
        for( size_t i=r.begin(); i!=r.end(); ++i )
            Foo(a[i]);
    }
    ApplyFoo( float a[] ) :
        my_a(a)
    {}
};
/*end_parallel_for_os_1*/

/*begin_parallel_for_os_2*/
#include "oneapi/tbb.h"

void ParallelApplyFoo( float a[], size_t n ) {
    parallel_for(blocked_range<size_t>(0,n), ApplyFoo(a));
}
/*end_parallel_for_os_2*/

int main() {
    constexpr std::size_t size = 10;
    float array[size] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10};

    ParallelApplyFoo(array, size);
}
