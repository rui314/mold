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

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_report.h"
#include "common/checktype.h"

#include "oneapi/tbb/detail/_utils.h"

#include "tbb/enumerable_thread_specific.h"
#include "tbb/parallel_for.h"
#include "tbb/blocked_range.h"
#include "tbb/tbb_allocator.h"
#include "tbb/global_control.h"
#include "tbb/cache_aligned_allocator.h"

#include <cstring>
#include <cstdio>
#include <vector>
#include <deque>
#include <list>
#include <map>
#include <utility>
#include <atomic>

//! \file test_enumerable_thread_specific.cpp
//! \brief Test for [tls.enumerable_thread_specific] specification

//! Minimum number of threads
static int MinThread = 1;

//! Maximum number of threads
static int MaxThread = 4;

static std::atomic<int> construction_counter;
static std::atomic<int> destruction_counter;

const int VALID_NUMBER_OF_KEYS = 100;

//! A minimal class that occupies N bytes.
/** Defines default and copy constructor, and allows implicit operator&. Hides operator=. */
template<size_t N = tbb::detail::max_nfs_size>
class minimalN: utils::NoAssign {
private:
    int my_value;
    bool is_constructed;
    char pad[N-sizeof(int) - sizeof(bool)];
public:
    minimalN() : utils::NoAssign(), my_value(0) { ++construction_counter; is_constructed = true; }
    minimalN( const minimalN&m ) : utils::NoAssign(), my_value(m.my_value) { ++construction_counter; is_constructed = true; }
    ~minimalN() { ++destruction_counter; REQUIRE(is_constructed); is_constructed = false; }
    void set_value( const int i ) { REQUIRE(is_constructed); my_value = i; }
    int value( ) const { REQUIRE(is_constructed); return my_value; }
};

static size_t AlignMask = 0;  // set to cache-line-size - 1

template<typename T>
T& check_alignment(T& t, const char *aname) {
    if( !tbb::detail::is_aligned(&t, AlignMask)) {
        // TBB_REVAMP_TODO: previously was REPORT_ONCE
        REPORT("alignment error with %s allocator (%x)\n", aname, (int)size_t(&t) & (AlignMask-1));
    }
    return t;
}

template<typename T>
const T& check_alignment(const T& t, const char *aname) {
    if( !tbb::detail::is_aligned(&t, AlignMask)) {
        // TBB_REVAMP_TODO: previously was REPORT_ONCE
        REPORT("alignment error with %s allocator (%x)\n", aname, (int)size_t(&t) & (AlignMask-1));
    }
    return t;
}

//
// A helper class that simplifies writing the tests since minimalN does not
// define = or + operators.
//

const size_t line_size = tbb::detail::max_nfs_size;

typedef tbb::enumerable_thread_specific<minimalN<line_size> > flogged_ets;

class set_body {
    flogged_ets *a;

public:
    set_body( flogged_ets*_a ) : a(_a) { }

    void operator() ( ) const {
        for (int i = 0; i < VALID_NUMBER_OF_KEYS; ++i) {
            check_alignment(a[i].local(), "default").set_value(i + 1);
        }
    }

};

void do_std_threads( int max_threads, flogged_ets a[] ) {
    std::vector< std::thread * > threads;

    for (int p = 0; p < max_threads; ++p) {
        threads.push_back( new std::thread ( set_body( a ) ) );
    }

    for (int p = 0; p < max_threads; ++p) {
        threads[p]->join();
    }

    for(int p = 0; p < max_threads; ++p) {
        delete threads[p];
    }
}

void flog_key_creation_and_deletion() {
    const int FLOG_REPETITIONS = 100;

    for (int p = MinThread; p <= MaxThread; ++p) {
        for (int j = 0; j < FLOG_REPETITIONS; ++j) {
            construction_counter = 0;
            destruction_counter = 0;
            // causes VALID_NUMBER_OF_KEYS exemplar instances to be constructed
            flogged_ets* a = new flogged_ets[VALID_NUMBER_OF_KEYS];
            REQUIRE(int(construction_counter) == 0);   // no exemplars or actual locals have been constructed
            REQUIRE(int(destruction_counter) == 0);    // and none have been destroyed
            // causes p * VALID_NUMBER_OF_KEYS minimals to be created
            do_std_threads(p, a);
            for (int i = 0; i < VALID_NUMBER_OF_KEYS; ++i) {
                int pcnt = 0;
                for ( flogged_ets::iterator tli = a[i].begin(); tli != a[i].end(); ++tli ) {
                    REQUIRE( (*tli).value() == i+1 );
                    ++pcnt;
                }
                REQUIRE( pcnt == p);  // should be one local per thread.
            }
            delete[] a;
        }
        REQUIRE( int(construction_counter) == (p)*VALID_NUMBER_OF_KEYS );
        REQUIRE( int(destruction_counter) == (p)*VALID_NUMBER_OF_KEYS );

        construction_counter = 0;
        destruction_counter = 0;

        // causes VALID_NUMBER_OF_KEYS exemplar instances to be constructed
        flogged_ets* a = new flogged_ets[VALID_NUMBER_OF_KEYS];

        for (int j = 0; j < FLOG_REPETITIONS; ++j) {
            // causes p * VALID_NUMBER_OF_KEYS minimals to be created
            do_std_threads(p, a);

            for (int i = 0; i < VALID_NUMBER_OF_KEYS; ++i) {
                for ( flogged_ets::iterator tli = a[i].begin(); tli != a[i].end(); ++tli ) {
                    REQUIRE( (*tli).value() == i+1 );
                }
                a[i].clear();
                REQUIRE( static_cast<int>(a[i].end() - a[i].begin()) == 0 );
            }
        }
        delete[] a;
        REQUIRE( int(construction_counter) == (FLOG_REPETITIONS*p)*VALID_NUMBER_OF_KEYS );
        REQUIRE( int(destruction_counter) == (FLOG_REPETITIONS*p)*VALID_NUMBER_OF_KEYS );
    }

}

template <typename inner_container>
void flog_segmented_interator() {

    bool found_error = false;
    typedef typename inner_container::value_type T;
    typedef std::vector< inner_container > nested_vec;
    inner_container my_inner_container;
    my_inner_container.clear();
    nested_vec my_vec;

    // simple nested vector (neither level empty)
    const int maxval = 10;
    for(int i=0; i < maxval; i++) {
        my_vec.push_back(my_inner_container);
        for(int j = 0; j < maxval; j++) {
            my_vec.at(i).push_back((T)(maxval * i + j));
        }
    }

    tbb::detail::d1::segmented_iterator<nested_vec, T> my_si(my_vec);

    T ii;
    for(my_si=my_vec.begin(), ii=0; my_si != my_vec.end(); ++my_si, ++ii) {
        if((*my_si) != ii) {
            found_error = true;
        }
    }

    // outer level empty
    my_vec.clear();
    for(my_si=my_vec.begin(); my_si != my_vec.end(); ++my_si) {
        found_error = true;
    }

    // inner levels empty
    my_vec.clear();
    for(int i =0; i < maxval; ++i) {
        my_vec.push_back(my_inner_container);
    }
    for(my_si = my_vec.begin(); my_si != my_vec.end(); ++my_si) {
        found_error = true;
    }

    // every other inner container is empty
    my_vec.clear();
    for(int i=0; i < maxval; ++i) {
        my_vec.push_back(my_inner_container);
        if(i%2) {
            for(int j = 0; j < maxval; ++j) {
                my_vec.at(i).push_back((T)(maxval * (i/2) + j));
            }
        }
    }
    for(my_si = my_vec.begin(), ii=0; my_si != my_vec.end(); ++my_si, ++ii) {
        if((*my_si) != ii) {
            found_error = true;
        }
    }

    tbb::detail::d1::segmented_iterator<nested_vec, const T> my_csi(my_vec);
    for(my_csi=my_vec.begin(), ii=0; my_csi != my_vec.end(); ++my_csi, ++ii) {
        if((*my_csi) != ii) {
            found_error = true;
        }
    }

    // outer level empty
    my_vec.clear();
    for(my_csi=my_vec.begin(); my_csi != my_vec.end(); ++my_csi) {
        found_error = true;
    }

    // inner levels empty
    my_vec.clear();
    for(int i =0; i < maxval; ++i) {
        my_vec.push_back(my_inner_container);
    }
    for(my_csi = my_vec.begin(); my_csi != my_vec.end(); ++my_csi) {
        found_error = true;
    }

    // every other inner container is empty
    my_vec.clear();
    for(int i=0; i < maxval; ++i) {
        my_vec.push_back(my_inner_container);
        if(i%2) {
            for(int j = 0; j < maxval; ++j) {
                my_vec.at(i).push_back((T)(maxval * (i/2) + j));
            }
        }
    }
    for(my_csi = my_vec.begin(), ii=0; my_csi != my_vec.end(); ++my_csi, ++ii) {
        if((*my_csi) != ii) {
            found_error = true;
        }
    }


    if(found_error) REPORT("segmented_iterator failed\n");
}

template <typename Key, typename Val>
void flog_segmented_iterator_map() {
   typedef typename std::map<Key, Val> my_map;
   typedef std::vector< my_map > nested_vec;
   my_map my_inner_container;
   my_inner_container.clear();
   nested_vec my_vec;
   my_vec.clear();
   bool found_error = false;

   // simple nested vector (neither level empty)
   const int maxval = 4;
   for(int i=0; i < maxval; i++) {
       my_vec.push_back(my_inner_container);
       for(int j = 0; j < maxval; j++) {
           my_vec.at(i).insert(std::make_pair<Key,Val>(maxval * i + j, 2*(maxval*i + j)));
       }
   }

   tbb::detail::d1::segmented_iterator<nested_vec, std::pair<const Key, Val> > my_si(my_vec);
   Key ii;
   for(my_si=my_vec.begin(), ii=0; my_si != my_vec.end(); ++my_si, ++ii) {
       if(((*my_si).first != ii) || ((*my_si).second != 2*ii)) {
           found_error = true;
       }
   }

   tbb::detail::d1::segmented_iterator<nested_vec, const std::pair<const Key, Val> > my_csi(my_vec);
   for(my_csi=my_vec.begin(), ii=0; my_csi != my_vec.end(); ++my_csi, ++ii) {
       if(((*my_csi).first != ii) || ((*my_csi).second != 2*ii)) {
           found_error = true;
           // INFO( "ii=%d, (*my_csi).first=%d, second=%d\n",ii, int((*my_csi).first), int((*my_csi).second));
       }
   }
   if(found_error) REPORT("segmented_iterator_map failed\n");
}

void run_segmented_iterator_tests() {
   // only the following containers can be used with the segmented iterator.
   flog_segmented_interator<std::vector< int > >();
   flog_segmented_interator<std::vector< double > >();
   flog_segmented_interator<std::deque< int > >();
   flog_segmented_interator<std::deque< double > >();
   flog_segmented_interator<std::list< int > >();
   flog_segmented_interator<std::list< double > >();

   flog_segmented_iterator_map<int, int>();
   flog_segmented_iterator_map<int, double>();
}

int align_val(void * const p) {
    size_t tmp = (size_t)p;
    int a = 1;
    while((tmp&0x1) == 0) { a <<=1; tmp >>= 1; }
    return a;
}

bool is_between(void* lowp, void *highp, void *testp) {
    if((size_t)lowp < (size_t)testp && (size_t)testp < (size_t)highp) return true;
    return (size_t)lowp > (size_t)testp && (size_t)testp > (size_t)highp;
}

template<class U> struct alignment_of {
    typedef struct { char t; U    padded; } test_alignment;
    static const size_t value = sizeof(test_alignment) - sizeof(U);
};
using tbb::detail::d1::ets_element;
template<typename T, typename OtherType>
void allocate_ets_element_on_stack(const char* /* name */) {
    typedef T aligning_element_type;
    const size_t my_align = alignment_of<aligning_element_type>::value;
    OtherType c1;
    ets_element<aligning_element_type> my_stack_element;
    OtherType c2;
    ets_element<aligning_element_type> my_stack_element2;
    struct {
        OtherType cxx;
        ets_element<aligning_element_type> my_struct_element;
    } mystruct1;
    tbb::detail::suppress_unused_warning(c1,c2);
    REQUIRE_MESSAGE(tbb::detail::is_aligned(my_stack_element.value(), my_align), "Error in first stack alignment" );
    REQUIRE_MESSAGE(tbb::detail::is_aligned(my_stack_element2.value(), my_align), "Error in second stack alignment" );
    REQUIRE_MESSAGE(tbb::detail::is_aligned(mystruct1.my_struct_element.value(), my_align), "Error in struct element alignment" );
}

class BigType {
public:
    BigType() { /* avoid cl warning C4345 about default initialization of POD types */ }
    char my_data[12 * 1024 * 1024];
};

template<template<class> class Allocator>
void TestConstructorWithBigType(const char* allocator_name) {
    typedef tbb::enumerable_thread_specific<BigType, Allocator<BigType> > CounterBigType;
    // Test default constructor
    CounterBigType MyCounters;
    // Create a local instance.
    typename CounterBigType::reference my_local = MyCounters.local();
    my_local.my_data[0] = 'a';
    // Test copy constructor
    CounterBigType MyCounters2(MyCounters);
    REQUIRE(check_alignment(MyCounters2.local(), allocator_name).my_data[0]=='a');
}

size_t init_tbb_alloc_mask() {
    // TODO: use __TBB_alignof(T) to check for local() results instead of using internal knowledges of ets element padding
    if(tbb::tbb_allocator<int>::allocator_type() == tbb::tbb_allocator<int>::standard) {
        // scalable allocator is not available.
        // INFO("tbb::tbb_allocator is not available\n");
        return 1;
    }
    else {
        // this value is for large objects, but will be correct for small.
        return 64; // TBB_REVAMP_TODO: enable as estimatedCacheLineSize when tbbmalloc is available;
    }
}

static const size_t cache_allocator_mask = tbb::detail::r1::cache_line_size();
static const size_t tbb_allocator_mask = init_tbb_alloc_mask();

//! Test for internal segmented_iterator type, used inside flattened2d class
//! \brief \ref error_guessing
TEST_CASE("Segmented iterator") {
    AlignMask = tbb_allocator_mask;
    run_segmented_iterator_tests();
}

//! Test ETS keys creation/deletion
//! \brief \ref error_guessing \ref boundary
TEST_CASE("Key creation and deletion") {
    AlignMask = tbb_allocator_mask;
    flog_key_creation_and_deletion();
}

//! Test construction with big ETS types
//! \brief \ref error_guessing
TEST_CASE("Constructor with big type") {
    AlignMask = cache_allocator_mask;
    TestConstructorWithBigType<tbb::cache_aligned_allocator>("tbb::cache_aligned_allocator");
    AlignMask = tbb_allocator_mask;
    TestConstructorWithBigType<tbb::tbb_allocator>("tbb::tbb_allocator");
}

//! Test allocation of ETS elements on the stack (internal types)
//! \brief \ref error_guessing
TEST_CASE("Allocate ETS on stack") {
    AlignMask = tbb_allocator_mask;
    allocate_ets_element_on_stack<int,char>("int vs. char");
    allocate_ets_element_on_stack<int,short>("int vs. short");
    allocate_ets_element_on_stack<int,char[3]>("int vs. char[3]");
    allocate_ets_element_on_stack<float,char>("float vs. char");
    allocate_ets_element_on_stack<float,short>("float vs. short");
    allocate_ets_element_on_stack<float,char[3]>("float vs. char[3]");
}

