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

// Header that sets HAVE_m128/HAVE_m256 if vector types (__m128/__m256) are available

//! Class for testing safety of using vector types.
/** Uses circuitous logic forces compiler to put __m128/__m256 objects on stack while
    executing various methods, and thus tempt it to use aligned loads and stores
    on the stack. */
//  Do not create file-scope objects of the class, because MinGW (as of May 2010)
//  did not always provide proper stack alignment in destructors of such objects.

#ifndef __TBB_test_common_vector_types_H_
#define __TBB_test_common_vector_types_H_

#include "config.h"

#if (_MSC_VER>=1600)
//TODO: handle /arch:AVX in the right way.
#pragma warning (push)
#pragma warning (disable: 4752)
#endif

#if __TBB_GCC_WARNING_IGNORED_ATTRIBUTES_PRESENT
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wignored-attributes"
#endif

template <typename Mvec>
class ClassWithVectorType {
    static constexpr int n = 16;
    static constexpr int F = sizeof(Mvec)/sizeof(float);
    Mvec field[n];

    void init( int );
public:
    ClassWithVectorType() { init(-n); }
    ClassWithVectorType( int i ) { init(i); }
    ClassWithVectorType( const ClassWithVectorType& src ) {
        for (int i = 0; i < n; ++i) {
            field[i] = src.field[i];
        }
    }

    ClassWithVectorType& operator=( const ClassWithVectorType& src ) {
        Mvec stack[n];
        for (int i = 0; i < n; ++i) {
            stack[i^5] = src.field[i];
        }
        for (int i = 0; i < n; ++i) {
            field[i^5] = stack[i];
        }
        return *this;
    }

    ~ClassWithVectorType() { init(-2 * n); }

    friend bool operator==( const ClassWithVectorType& x, const ClassWithVectorType& y ) {
        for( int i = 0; i < F*n; ++i ) {
            if( ((const float*)x.field)[i] != ((const float*)y.field)[i] )
                return false;
        }
        return true;
    }

    friend bool operator!=( const ClassWithVectorType& x, const ClassWithVectorType& y ) {
        return !(x == y);
    }
}; // class ClassWithVectorType

template <typename Mvec>
void ClassWithVectorType<Mvec>::init( int start ) {
   Mvec stack[n];
    for( int i = 0; i < n; ++i ) {
        // Declaring value as a one-element array instead of a scalar quites
        // gratuitous warnings about possible use of "value" before it was set.
        Mvec value[1];
        for( int j = 0; j < F; ++j )
            ((float*)value)[j] = float(n*start+F*i+j);
        stack[i^5] = value[0];
    }
    for( int i = 0; i < n; ++i )
        field[i^5] = stack[i];
}

#if (defined(__AVX__) || (_MSC_VER >= 1600 && defined(_M_X64))) && !defined(__sun)
#include <immintrin.h>
#if __clang__
#include <avxintrin.h>
#endif
#define HAVE_m256 1
using ClassWithAVX = ClassWithVectorType<__m256>;
#if _MSC_VER
#include <intrin.h> // for __cpuid
#endif
bool have_AVX() {
    bool result = false;
    const int avx_mask = 1 << 28;
#if _MSC_VER || defined(__INTEL_COMPILER)
    int info[4] = {0, 0, 0, 0};
    const int ECX = 2;
    __cpuid(info, 1);
    result = (info[ECX] & avx_mask) != 0;
#elif defined(__GNUC__)
    int ECX;
    __asm__( "cpuid"
             : "=c"(ECX)
             : "a" (1)
             : "ebx", "edx" );
    result = (ECX & avx_mask);
#endif
    return result;
}
#endif // __AVX__ etc

#if (defined(__SSE__) || defined(_M_IX86_FP) || defined(_M_X64)) && !defined(__sun)
#include <xmmintrin.h>
#define HAVE_m128 1
using ClassWithSSE = ClassWithVectorType<__m128>;
#endif

#if __TBB_GCC_WARNING_IGNORED_ATTRIBUTES_PRESENT
#pragma GCC diagnostic pop
#endif

#if (_MSC_VER>=1600)
#pragma warning (pop)
#endif
#endif // __TBB_test_common_vector_types_H_
