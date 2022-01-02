/*
    Copyright (c) 2005-2021 Intel Corporation

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

//! \file test_openmp.cpp
//! \brief Test for [internal] functionality

#if _WIN32 || _WIN64
#define _CRT_SECURE_NO_WARNINGS
#endif

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_env.h"
#include "tbb/global_control.h"
#include "tbb/blocked_range.h"
#include "tbb/parallel_for.h"
#include "tbb/parallel_reduce.h"

// Test mixing OpenMP and TBB
#include <omp.h>

using data_type = short;

void SerialConvolve( data_type c[], const data_type a[], int m, const data_type b[], int n ) {
    for (int i = 0; i < m + n - 1; ++i) {
        int start = i < n ? 0 : i - n + 1;
        int finish = i < m ? i + 1 : m;
        data_type sum = 0;
        for (int j = start; j < finish; ++j)
            sum += a[j] * b[i - j];
        c[i] = sum;
    }
}

#if _MSC_VER && !defined(__INTEL_COMPILER)
    // Suppress overzealous warning about short+=short
    #pragma warning( push )
    #pragma warning( disable: 4244 )
#endif

class InnerBody: utils::NoAssign {
    const data_type* my_a;
    const data_type* my_b;
    const int i;
public:
    data_type sum;
    InnerBody( data_type /*c*/[], const data_type a[], const data_type b[], int ii ) :
        my_a(a), my_b(b), i(ii), sum(0)
    {}
    InnerBody( InnerBody& x, tbb::split ) :
        my_a(x.my_a), my_b(x.my_b), i(x.i), sum(0)
    {
    }
    void join( InnerBody& x ) { sum += x.sum; }
    void operator()( const tbb::blocked_range<int>& range ) {
        for (int j = range.begin(); j != range.end(); ++j)
            sum += my_a[j] * my_b[i - j];
    }
};

#if _MSC_VER && !defined(__INTEL_COMPILER)
    #pragma warning( pop )
#endif

//! Test OpenMP loop around TBB loop
void OpenMP_TBB_Convolve( data_type c[], const data_type a[], int m, const data_type b[], int n, int p ) {
    utils::suppress_unused_warning(p);
#pragma omp parallel num_threads(p)
    {
#pragma omp for
        for (int i = 0; i < m + n - 1; ++i) {
            int start = i < n ? 0 : i - n + 1;
            int finish = i < m ? i + 1 : m;
            InnerBody body(c, a, b, i);
            tbb::parallel_reduce(tbb::blocked_range<int>(start, finish, 10), body);
            c[i] = body.sum;
        }
    }
}

class OuterBody: utils::NoAssign {
    const data_type* my_a;
    const data_type* my_b;
    data_type* my_c;
    const int m;
    const int n;
#if __clang__ && !__INTEL_COMPILER
    #pragma clang diagnostic push
    #pragma clang diagnostic ignored "-Wunused-private-field"
#endif
    const int p;
#if __clang__ && !__INTEL_COMPILER
    #pragma clang diagnostic pop // "-Wunused-private-field"
#endif
public:
    OuterBody( data_type c[], const data_type a[], int m_, const data_type b[], int n_, int p_ ) :
        my_a(a), my_b(b), my_c(c), m(m_), n(n_), p(p_)
    {}
    void operator()( const tbb::blocked_range<int>& range ) const {
        for (int i = range.begin(); i != range.end(); ++i) {
            int start = i < n ? 0 : i - n + 1;
            int finish = i < m ? i + 1 : m;
            data_type sum = 0;
#pragma omp parallel for reduction(+:sum) num_threads(p)
            for (int j = start; j < finish; ++j)
                sum += my_a[j] * my_b[i - j];
            my_c[i] = sum;
        }
    }
};

//! Test TBB loop around OpenMP loop
void TBB_OpenMP_Convolve( data_type c[], const data_type a[], int m, const data_type b[], int n, int p ) {
    tbb::parallel_for(tbb::blocked_range<int>(0, m + n - 1, 10), OuterBody(c, a, m, b, n, p));
}

#if __INTEL_COMPILER
void TestNumThreads() {
    utils::SetEnv("KMP_AFFINITY", "compact");
    // Make an OpenMP call before initializing TBB
    int omp_nthreads = omp_get_max_threads();
    #pragma omp parallel
    {}
    int tbb_nthreads = tbb::this_task_arena::max_concurrency();
    // For the purpose of testing, assume that OpenMP and TBB should utilize the same # of threads.
    // If it's not true on some platforms, the test will need to be adjusted.
    REQUIRE_MESSAGE(tbb_nthreads == omp_nthreads, "Initialization of TBB is possibly affected by OpenMP");
}
#endif // __INTEL_COMPILER

const int M = 17 * 17;
const int N = 13 * 13;
data_type A[M], B[N];
data_type expected[M+N], actual[M+N];

template <class Func>
void RunTest( Func F, int m, int n, int p) {
    tbb::global_control limit(tbb::global_control::max_allowed_parallelism, p);
    memset(actual, -1, (m + n) * sizeof(data_type));
    F(actual, A, m, B, n, p);
    CHECK(memcmp(actual, expected, (m + n - 1) * sizeof(data_type)) == 0);
}

// Disable it because OpenMP isn't instrumented that leads to false positive
#if !__TBB_USE_THREAD_SANITIZER
//! \brief \ref error_guessing
TEST_CASE("Testing oneTBB with OpenMP") {
#if __INTEL_COMPILER
    TestNumThreads(); // Testing initialization-related behavior; must be the first
#endif // __INTEL_COMPILER
    for (int p = static_cast<int>(utils::MinThread); p <= static_cast<int>(utils::MaxThread); ++p) {
        for (int m = 1; m <= M; m *= 17) {
            for (int n = 1; n <= N; n *= 13) {
                for (int i = 0; i < m; ++i) A[i] = data_type(1 + i / 5);
                for (int i = 0; i < n; ++i) B[i] = data_type(1 + i / 7);
                SerialConvolve( expected, A, m, B, n );
                RunTest( OpenMP_TBB_Convolve, m, n, p );
                RunTest( TBB_OpenMP_Convolve, m, n, p );
            }
        }
    }
}
#endif
