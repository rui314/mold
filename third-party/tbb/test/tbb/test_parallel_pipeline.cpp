/*
    Copyright (c) 2005-2022 Intel Corporation

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

// Before including parallel_pipeline.h, set up the variable to count heap allocated
// filter_node objects, and make it known for the header.
#include "common/test.h"
#include "common/utils.h"
#include "common/checktype.h"

int filter_node_count = 0;
#define __TBB_TEST_FILTER_NODE_COUNT filter_node_count
#include "tbb/parallel_pipeline.h"
#include "tbb/global_control.h"
#include "tbb/spin_mutex.h"
#include "tbb/task_group.h"

#include <atomic>
#include <string.h>
#include <memory> // std::unique_ptr

//! \file test_parallel_pipeline.cpp
//! \brief Test for [algorithms.parallel_pipeline algorithms.parallel_pipeline.flow_control] specification

const unsigned n_tokens = 8;
// we can conceivably have two buffers used in the middle filter for every token in flight, so
// we must allocate two buffers for every token.  Unlikely, but possible.
const unsigned n_buffers = 2*n_tokens;
const int max_counter = 16;

static std::size_t concurrency = 0;

static std::atomic<int> output_counter;
static std::atomic<int> input_counter;
static std::atomic<int> non_pointer_specialized_calls;
static std::atomic<int> pointer_specialized_calls;
static std::atomic<int> first_pointer_specialized_calls;
static std::atomic<int> second_pointer_specialized_calls;

static int intbuffer[max_counter];  // store results for <int,int> parallel pipeline test
static bool check_intbuffer;

static void* buffers[n_buffers];
static std::atomic_flag buf_in_use[n_buffers] = {ATOMIC_FLAG_INIT};

void *fetchNextBuffer() {
    for(size_t icnt = 0; icnt < n_buffers; ++icnt) {
        if(!buf_in_use[icnt].test_and_set()) {
            return buffers[icnt];
        }
    }
    CHECK_MESSAGE(false, "Ran out of buffers, p:"<< concurrency);
    return nullptr;
}
void freeBuffer(void *buf) {
    for(size_t i=0; i < n_buffers;++i) {
        if(buffers[i] == buf) {
            buf_in_use[i].clear();
            return;
        }
    }
    CHECK_MESSAGE(false, "Tried to free a buffer not in our list, p:" << concurrency);
}

template<typename T>
class free_on_scope_exit {
public:
    free_on_scope_exit(T *p) : my_p(p) {}
    ~free_on_scope_exit() { if(!my_p) return; my_p->~T(); freeBuffer(my_p); }
private:
    T *my_p;
};

// methods for testing CheckType< >, that return okay values for other types.
template<typename T>
bool middle_is_ready(T &/*p*/) { return false; }

template<typename U>
bool middle_is_ready(CheckType<U> &p) { return p.is_ready(); }

template<typename T>
bool output_is_ready(T &/*p*/) { return true; }

template<typename U>
bool output_is_ready(CheckType<U> &p) { return p.is_ready(); }

template<typename T>
int middle_my_id( T &/*p*/) { return 0; }

template<typename U>
int middle_my_id(CheckType<U> &p) { return p.id(); }

template<typename T>
int output_my_id( T &/*p*/) { return 1; }

template<typename U>
int output_my_id(CheckType<U> &p) { return p.id(); }

template<typename T>
void my_function(T &p) { p = 0; }

template<typename U>
void my_function(CheckType<U> &p) { p.get_ready(); }

// Filters must be copy-constructible, and be const-qualifiable.
template<typename U>
class input_filter : DestroyedTracker {
public:
    U operator()( tbb::flow_control& control ) const {
        CHECK(is_alive());
        if( --input_counter < 0 ) {
            control.stop();
        }
        else  // only count successful reads
            ++non_pointer_specialized_calls;
        return U();  // default constructed
    }

};

// specialization for pointer
template<typename U>
class input_filter<U*> : DestroyedTracker {
public:
    U* operator()(tbb::flow_control& control) const {
        CHECK(is_alive());
        int ival = --input_counter;
        if(ival < 0) {
            control.stop();
            return nullptr;
        }
        ++pointer_specialized_calls;
        if(ival == max_counter / 2) {
            return nullptr;  // non-stop nullptr
        }
        U* myReturn = new(fetchNextBuffer()) U();
        if (myReturn) {  // may have been passed in a nullptr
            CHECK_MESSAGE(!middle_my_id(*myReturn), "bad id value, p:" << concurrency);
            CHECK_MESSAGE(!middle_is_ready(*myReturn), "Already ready, p:" << concurrency);
        }
        return myReturn;
    }
};

template<>
class input_filter<void> : DestroyedTracker {
public:
    void operator()( tbb::flow_control& control ) const {
        CHECK(is_alive());
        if( --input_counter < 0 ) {
            control.stop();
        }
        else
            ++non_pointer_specialized_calls;
    }

};

// specialization for int that passes back a sequence of integers
template<>
class input_filter<int> : DestroyedTracker {
public:
    int
    operator()(tbb::flow_control& control ) const {
        CHECK(is_alive());
        int oldval = --input_counter;
        if( oldval < 0 ) {
            control.stop();
        }
        else
            ++non_pointer_specialized_calls;
        return oldval+1;
    }
};

template<typename T, typename U>
class middle_filter : DestroyedTracker {
public:
    U operator()(T t) const {
        CHECK(is_alive());
        CHECK_MESSAGE(!middle_my_id(t), "bad id value, p:" << concurrency);
        CHECK_MESSAGE(!middle_is_ready(t), "Already ready, p:" << concurrency );
        U out;
        my_function(out);
        ++non_pointer_specialized_calls;
        return out;
    }
};

template<typename T, typename U>
class middle_filter<T*,U> : DestroyedTracker {
public:
    U operator()(T* my_storage) const {
        free_on_scope_exit<T> my_ptr(my_storage);  // free_on_scope_exit marks the buffer available
        CHECK(is_alive());
        if(my_storage) {  // may have been passed in a nullptr
            CHECK_MESSAGE(!middle_my_id(*my_storage), "bad id value, p:" << concurrency);
            CHECK_MESSAGE(!middle_is_ready(*my_storage), "Already ready, p:" << concurrency );
        }
        ++first_pointer_specialized_calls;
        U out;
        my_function(out);
        return out;
    }
};

template<typename T, typename U>
class middle_filter<T,U*> : DestroyedTracker {
public:
    U* operator()(T my_storage) const {
        CHECK(is_alive());
        CHECK_MESSAGE(!middle_my_id(my_storage), "bad id value, p:" << concurrency);
        CHECK_MESSAGE(!middle_is_ready(my_storage), "Already ready, p:" << concurrency );
        // allocate new space from buffers
        U* my_return = new(fetchNextBuffer()) U();
        my_function(*my_return);
        ++second_pointer_specialized_calls;
        return my_return;
    }
};

template<typename T, typename U>
class middle_filter<T*,U*> : DestroyedTracker {
public:
    U* operator()(T* my_storage) const {
        free_on_scope_exit<T> my_ptr(my_storage);  // free_on_scope_exit marks the buffer available
        CHECK(is_alive());
        if(my_storage) {
            CHECK_MESSAGE(!middle_my_id(*my_storage), "bad id value, p:" << concurrency);
            CHECK_MESSAGE(!middle_is_ready(*my_storage), "Already ready, p:" << concurrency );
        }
        // may have been passed a nullptr
        ++pointer_specialized_calls;
        if(!my_storage) return nullptr;
        CHECK_MESSAGE(!middle_my_id(*my_storage), "bad id value, p:" << concurrency);
        CHECK_MESSAGE(!middle_is_ready(*my_storage), "Already ready, p:" << concurrency );
        U* my_return = new(fetchNextBuffer()) U();
        my_function(*my_return);
        return my_return;
    }
};

// specialization for int that squares the input and returns that.
template<>
class middle_filter<int,int> : DestroyedTracker {
public:
    int operator()(int my_input) const {
        CHECK(is_alive());
        ++non_pointer_specialized_calls;
        return my_input*my_input;
    }
};

// ---------------------------------
template<typename T>
class output_filter : DestroyedTracker {
public:
    void operator()(T c) const {
        CHECK(is_alive());
        CHECK_MESSAGE(output_my_id(c), "unset id value, p:" << concurrency);
        CHECK_MESSAGE(output_is_ready(c), "not yet ready, p:" << concurrency);
        ++non_pointer_specialized_calls;
        output_counter++;
    }
};

// specialization for int that puts the received value in an array
template<>
class output_filter<int> : DestroyedTracker {
public:
    void operator()(int my_input) const {
        CHECK(is_alive());
        ++non_pointer_specialized_calls;
        int myindx = output_counter++;
        intbuffer[myindx] = my_input;
    }
};

template<typename T>
class output_filter<T*> : DestroyedTracker {
public:
    void operator()(T* c) const {
        free_on_scope_exit<T> my_ptr(c);
        CHECK(is_alive());
        if(c) {
            CHECK_MESSAGE(output_my_id(*c), "unset id value, p:" << concurrency);
            CHECK_MESSAGE(output_is_ready(*c), "not yet ready, p:" << concurrency);
        }
        output_counter++;
        ++pointer_specialized_calls;
    }
};

typedef enum {
    no_pointer_counts,
    assert_nonpointer,
    assert_firstpointer,
    assert_secondpointer,
    assert_allpointer
} final_assert_type;

void resetCounters() {
    output_counter = 0;
    input_counter = max_counter;
    non_pointer_specialized_calls = 0;
    pointer_specialized_calls = 0;
    first_pointer_specialized_calls = 0;
    second_pointer_specialized_calls = 0;
    // we have to reset the buffer flags because our input filters return allocated space on end-of-input,
    // (on eof a default-constructed object is returned) and they do not pass through the filter further.
    for(size_t i = 0; i < n_buffers; ++i)
        buf_in_use[i].clear();
}

void checkCounters(final_assert_type my_t) {
    CHECK_MESSAGE(output_counter == max_counter, "Ran out of buffers, p:" << concurrency);
    switch(my_t) {
        case assert_nonpointer:
            CHECK_MESSAGE(pointer_specialized_calls+first_pointer_specialized_calls+second_pointer_specialized_calls == 0, "non-pointer filters specialized to pointer, p:" << concurrency);
            CHECK_MESSAGE(non_pointer_specialized_calls == 3*max_counter, "bad count for non-pointer filters, p:" << concurrency);
            if(check_intbuffer) {
                for(int i = 1; i <= max_counter; ++i) {
                    int j = i*i;
                    bool found_val = false;
                    for(int k = 0; k < max_counter; ++k) {
                        if(intbuffer[k] == j) {
                            found_val = true;
                            break;
                        }
                    }
                    CHECK_MESSAGE(found_val, "Missing value in output array, p:" << concurrency );
                }
            }
            break;
        case assert_firstpointer:
            {
                bool check = pointer_specialized_calls == max_counter &&  // input filter extra invocation
                    first_pointer_specialized_calls == max_counter &&
                    non_pointer_specialized_calls == max_counter &&
                    second_pointer_specialized_calls == 0;
                CHECK_MESSAGE(check, "incorrect specialization for firstpointer, p:" << concurrency);
            }
            break;
        case assert_secondpointer:
            {
                bool check = pointer_specialized_calls == max_counter &&
                    first_pointer_specialized_calls == 0 &&
                    non_pointer_specialized_calls == max_counter &&  // input filter
                    second_pointer_specialized_calls == max_counter;
                CHECK_MESSAGE(check, "incorrect specialization for firstpointer, p:" << concurrency);
            }
            break;
        case assert_allpointer:
            CHECK_MESSAGE(non_pointer_specialized_calls+first_pointer_specialized_calls+second_pointer_specialized_calls == 0, "pointer filters specialized to non-pointer, p:" << concurrency);
            CHECK_MESSAGE(pointer_specialized_calls == 3*max_counter, "bad count for pointer filters, p:" << concurrency);
            break;
        case no_pointer_counts:
            break;
    }
}

static const tbb::filter_mode filter_table[] = { tbb::filter_mode::parallel, tbb::filter_mode::serial_in_order, tbb::filter_mode::serial_out_of_order};
const unsigned number_of_filter_types = sizeof(filter_table)/sizeof(filter_table[0]);

using filter_chain = tbb::filter<void, void>;
using mode_array =tbb::filter_mode;

// The filters are passed by value, which forces a temporary copy to be created.  This is
// to reproduce the bug where a filter_chain uses refs to filters, which after a call
// would be references to destructed temporaries.
template<typename type1, typename type2>
void fill_chain( filter_chain &my_chain, mode_array *filter_type, input_filter<type1> i_filter,
         middle_filter<type1, type2> m_filter, output_filter<type2> o_filter ) {
    my_chain = tbb::filter<void, type1>(filter_type[0], i_filter) &
        tbb::filter<type1, type2>(filter_type[1], m_filter) &
        tbb::filter<type2, void>(filter_type[2], o_filter);
}

template<typename... Context>
void run_function_spec(Context&... context) {
    CHECK_MESSAGE(!filter_node_count, "invalid filter_node counter");
    input_filter<void> i_filter;
    // Test pipeline that contains only one filter
    for( unsigned i = 0; i<number_of_filter_types; i++) {
        tbb::filter<void, void> one_filter( filter_table[i], i_filter );
        CHECK_MESSAGE(filter_node_count==1, "some filter nodes left after previous iteration?");
        resetCounters();
        tbb::parallel_pipeline( n_tokens, one_filter, context... );
        // no need to check counters
        std::atomic<int> counter;
        counter = max_counter;
        // Construct filter using lambda-syntax when parallel_pipeline() is being run;
        tbb::parallel_pipeline( n_tokens,
            tbb::filter<void, void>(filter_table[i], [&counter]( tbb::flow_control& control ) {
                    if( counter-- == 0 )
                        control.stop();
                    }
            ),
            context...
        );
    }
    CHECK_MESSAGE(!filter_node_count, "filter_node objects leaked");
}

template<typename t1, typename t2, typename... Context>
void run_filter_set(
        input_filter<t1>& i_filter,
        middle_filter<t1,t2>& m_filter,
        output_filter<t2>& o_filter,
        mode_array *filter_type,
        final_assert_type my_t,
        Context&... context) {
    tbb::filter<void, t1> filter1( filter_type[0], i_filter );
    tbb::filter<t1, t2> filter2( filter_type[1], m_filter );
    tbb::filter<t2, void> filter3( filter_type[2], o_filter );

    CHECK_MESSAGE(filter_node_count==3, "some filter nodes left after previous iteration?");
    resetCounters();
    // Create filters sequence when parallel_pipeline() is being run
    tbb::parallel_pipeline( n_tokens, filter1, filter2, filter3, context...  );
    checkCounters(my_t);

    // Create filters sequence partially outside parallel_pipeline() and also when parallel_pipeline() is being run
    tbb::filter<void, t2> filter12;
    filter12 = filter1 & filter2;
    for( int i = 0; i<3; i++)
    {
        filter12 &= tbb::filter<t2,t2>(filter_type[i], [](t2 x) -> t2 { return x;});
    }
    resetCounters();
    tbb::parallel_pipeline( n_tokens, filter12, filter3, context...  );
    checkCounters(my_t);

    tbb::filter<void, void> filter123 = filter12 & filter3;
    // Run pipeline twice with the same filter sequence
    for( unsigned i = 0; i<2; i++ ) {
        resetCounters();
        tbb::parallel_pipeline( n_tokens, filter123, context...  );
        checkCounters(my_t);
    }

    // Now copy-and-move-construct another filter instance, and use it to run pipeline
    {
        tbb::filter<void, void> copy123( filter123 );
        resetCounters();
        tbb::parallel_pipeline( n_tokens, copy123, context...  );
        checkCounters(my_t);
        tbb::filter<void, void> move123( std::move(filter123) );
        resetCounters();
        tbb::parallel_pipeline( n_tokens, move123, context...  );
        checkCounters(my_t);
    }

    // Construct filters and create the sequence when parallel_pipeline() is being run
    resetCounters();
    tbb::parallel_pipeline( n_tokens,
               tbb::filter<void, t1>(filter_type[0], i_filter),
               tbb::filter<t1, t2>(filter_type[1], m_filter),
               tbb::filter<t2, void>(filter_type[2], o_filter),
               context...  );
    checkCounters(my_t);

    // Construct filters, make a copy, destroy the original filters, and run with the copy
    int cnt = filter_node_count;
    {
        tbb::filter<void, void>* p123 = new tbb::filter<void,void> (
               tbb::filter<void, t1>(filter_type[0], i_filter)&
               tbb::filter<t1, t2>(filter_type[1], m_filter)&
               tbb::filter<t2, void>(filter_type[2], o_filter) );
        CHECK_MESSAGE(filter_node_count==cnt+5, "filter node accounting error?");
        tbb::filter<void, void> copy123( *p123 );
        delete p123;
        CHECK_MESSAGE(filter_node_count==cnt+5, "filter nodes deleted prematurely?");
        resetCounters();
        tbb::parallel_pipeline( n_tokens, copy123, context...  );
        checkCounters(my_t);
    }

    // construct a filter with temporaries
    {
        tbb::filter<void, void> my_filter;
        fill_chain<t1,t2>( my_filter, filter_type, i_filter, m_filter, o_filter );
        resetCounters();
        tbb::parallel_pipeline( n_tokens, my_filter, context...  );
        checkCounters(my_t);
    }
    CHECK_MESSAGE(filter_node_count==cnt, "scope ended but filter nodes not deleted?");
}

template <typename t1, typename t2, typename... Context>
void run_lambdas_test( mode_array *filter_type, Context&... context ) {
    std::atomic<int> counter;
    counter = max_counter;
    // Construct filters using lambda-syntax and create the sequence when parallel_pipeline() is being run;
    resetCounters();  // only need the output_counter reset.
    tbb::parallel_pipeline( n_tokens,
        tbb::make_filter<void, t1>(filter_type[0], [&counter]( tbb::flow_control& control ) -> t1 {
                if( --counter < 0 )
                    control.stop();
                return t1(); }
        ),
        tbb::make_filter<t1, t2>(filter_type[1], []( t1 /*my_storage*/ ) -> t2 {
                return t2(); }
        ),
        tbb::make_filter<t2,void>(filter_type[2], [] ( t2 ) -> void {
                output_counter++; }
        ),
        context...
    );
    checkCounters(no_pointer_counts);  // don't have to worry about specializations
    counter = max_counter;
    // pointer filters
    resetCounters();
    tbb::parallel_pipeline( n_tokens,
        tbb::filter<void,t1*>(filter_type[0], [&counter]( tbb::flow_control& control ) -> t1* {
                if( --counter < 0 ) {
                    control.stop();
                    return nullptr;
                }
                return new(fetchNextBuffer()) t1(); }
        ),
		tbb::filter<t1*, t2*>(filter_type[1], []( t1* my_storage ) -> t2* {
                my_storage->~t1();
                return new(my_storage) t2(); }
        ),
		tbb::filter<t2*, void>(filter_type[2], [] ( t2* my_storage ) -> void {
                my_storage->~t2();
                freeBuffer(my_storage);
                output_counter++; }
        ),
        context...
    );
    checkCounters(no_pointer_counts);
    // first filter outputs pointer
    counter = max_counter;
    resetCounters();
    tbb::parallel_pipeline( n_tokens,
        tbb::make_filter(filter_type[0], [&counter]( tbb::flow_control& control ) -> t1* {
                if( --counter < 0 ) {
                    control.stop();
                    return nullptr;
                }
                return new(fetchNextBuffer()) t1(); }
        )&
        tbb::make_filter(filter_type[1], []( t1* my_storage ) -> t2 {
                my_storage->~t1();
                freeBuffer(my_storage);
                return t2(); }
        ),
        tbb::make_filter(filter_type[2], [] ( t2 /*my_storage*/) -> void {
                output_counter++; }
        ),
        context...
    );
    checkCounters(no_pointer_counts);
    // second filter outputs pointer
    counter = max_counter;
    resetCounters();
    tbb::parallel_pipeline( n_tokens,
        tbb::make_filter(filter_type[0], [&counter]( tbb::flow_control& control ) -> t1 {
                if( --counter < 0 ) {
                    control.stop();
                }
                return t1(); }
        ),
        tbb::filter<t1, t2*>(filter_type[1], []( t1 /*my_storage*/ ) -> t2* {
                return new(fetchNextBuffer()) t2(); }
        )&
        tbb::make_filter<t2*, void>(filter_type[2], [] ( t2* my_storage) -> void {
                my_storage->~t2();
                freeBuffer(my_storage);
                output_counter++; }
        ),
        context...
    );
    checkCounters(no_pointer_counts);
}

template<typename type1, typename type2>
void run_function(const char *l1, const char *l2) {
    CHECK_MESSAGE(!filter_node_count, "invalid filter_node counter");

    check_intbuffer = (!strcmp(l1,"int") && !strcmp(l2,"int"));

    Checker<type1> check1;  // check constructions/destructions
    Checker<type2> check2;  // for type1 or type2 === CheckType<T>

    const size_t number_of_filters = 3;

    input_filter<type1> i_filter;
    input_filter<type1*> p_i_filter;

    middle_filter<type1, type2> m_filter;
    middle_filter<type1*, type2> pr_m_filter;
    middle_filter<type1, type2*> rp_m_filter;
    middle_filter<type1*, type2*> pp_m_filter;

    output_filter<type2> o_filter;
    output_filter<type2*> p_o_filter;

    // allocate the buffers for the filters
    unsigned max_size = (sizeof(type1) > sizeof(type2) ) ? sizeof(type1) : sizeof(type2);
    for(unsigned i = 0; i < (unsigned)n_buffers; ++i) {
        buffers[i] = malloc(max_size);
        buf_in_use[i].clear();
    }

    unsigned limit = 1;
    // Test pipeline that contains number_of_filters filters
    for( unsigned i=0; i<number_of_filters; ++i)
        limit *= number_of_filter_types;
    // Iterate over possible filter sequences
    for( unsigned numeral=0; numeral<limit; ++numeral ) {
        unsigned temp = numeral;
        tbb::filter_mode filter_type[number_of_filter_types];
        for( unsigned i=0; i<number_of_filters; ++i, temp/=number_of_filter_types )
            filter_type[i] = filter_table[temp%number_of_filter_types];

        tbb::task_group_context context;
        run_filter_set<type1,type2>(i_filter, m_filter, o_filter, filter_type, assert_nonpointer);
        run_filter_set<type1,type2>(i_filter, m_filter, o_filter, filter_type, assert_nonpointer, context);
        run_filter_set<type1*,type2>(p_i_filter, pr_m_filter, o_filter, filter_type, assert_firstpointer);
        run_filter_set<type1*,type2>(p_i_filter, pr_m_filter, o_filter, filter_type, assert_firstpointer, context);
        run_filter_set<type1,type2*>(i_filter, rp_m_filter, p_o_filter, filter_type, assert_secondpointer);
        run_filter_set<type1,type2*>(i_filter, rp_m_filter, p_o_filter, filter_type, assert_secondpointer, context);
        run_filter_set<type1*,type2*>(p_i_filter, pp_m_filter, p_o_filter, filter_type, assert_allpointer);
        run_filter_set<type1*,type2*>(p_i_filter, pp_m_filter, p_o_filter, filter_type, assert_allpointer, context);

        run_lambdas_test<type1,type2>(filter_type);
        run_lambdas_test<type1,type2>(filter_type, context);
    }
    CHECK_MESSAGE(!filter_node_count, "filter_node objects leaked");

    for(unsigned i = 0; i < (unsigned)n_buffers; ++i) {
        free(buffers[i]);
    }
}

//! Testing single filter pipeline
//! \brief \ref error_guessing
TEST_CASE("Pipeline testing for single filter") {
    run_function_spec();
    tbb::task_group_context context;
    run_function_spec(context);
}

#define RUN_TYPED_TEST_CASE(type1, type2) TEST_CASE("Pipeline testing with "#type1" and "#type2) { \
                                                for ( std::size_t concurrency_level : {1, 2, 4, 5, 7, 8} ) { \
                                                    if ( concurrency_level > tbb::global_control::active_value(tbb::global_control::max_allowed_parallelism) ) \
                                                        break; \
                                                    concurrency = concurrency_level; \
                                                    tbb::global_control control(tbb::global_control::max_allowed_parallelism, concurrency_level); \
                                                    run_function<type1, type2>(#type1, #type2); \
                                                } \
                                            }
// Run test several times with different types
RUN_TYPED_TEST_CASE(std::size_t, int)
RUN_TYPED_TEST_CASE(int, double)
RUN_TYPED_TEST_CASE(std::size_t, double)
RUN_TYPED_TEST_CASE(std::size_t, bool)
RUN_TYPED_TEST_CASE(int, int)
RUN_TYPED_TEST_CASE(CheckType<unsigned int>, std::size_t)
RUN_TYPED_TEST_CASE(CheckType<unsigned short>, std::size_t)
RUN_TYPED_TEST_CASE(CheckType<unsigned int>, CheckType<unsigned int>)
RUN_TYPED_TEST_CASE(CheckType<unsigned int>, CheckType<unsigned short>)
RUN_TYPED_TEST_CASE(CheckType<unsigned short>, CheckType<unsigned short>)
RUN_TYPED_TEST_CASE(double, CheckType<unsigned short>)
RUN_TYPED_TEST_CASE(std::unique_ptr<int>, std::unique_ptr<int>) // move-only type

#undef RUN_TYPED_TEST_CASE
