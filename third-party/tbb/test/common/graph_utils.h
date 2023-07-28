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

/** @file harness_graph.cpp
    This contains common helper classes and functions for testing graph nodes
**/

#ifndef __TBB_harness_graph_H
#define __TBB_harness_graph_H

#include "config.h"

#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/task.h"
#include "oneapi/tbb/null_rw_mutex.h"
#include "oneapi/tbb/concurrent_unordered_set.h"

#include <atomic>
#include <thread>
#include <mutex>
#include <condition_variable>

#include "common/spin_barrier.h"

using tbb::detail::d1::SUCCESSFULLY_ENQUEUED;

// Needed conversion to and from continue_msg, but didn't want to add
// conversion operators to the class, since we don't want it in general,
// only in these tests.
template<typename InputType, typename OutputType>
struct converter {
    static OutputType convert_value(const InputType &i) {
        return OutputType(i);
    }
};

template<typename InputType>
struct converter<InputType,tbb::flow::continue_msg> {
    static tbb::flow::continue_msg convert_value(const InputType &/*i*/) {
        return tbb::flow::continue_msg();
    }
};

template<typename OutputType>
struct converter<tbb::flow::continue_msg,OutputType> {
    static OutputType convert_value(const tbb::flow::continue_msg &/*i*/) {
        return OutputType();
    }
};

// helper for multifunction_node tests.
template<size_t N>
struct mof_helper {
    template<typename InputType, typename ports_type>
    static inline void output_converted_value(const InputType &i, ports_type &p) {
        (void)std::get<N-1>(p).try_put(converter<InputType,typename std::tuple_element<N-1,ports_type>::type::output_type>::convert_value(i));
        output_converted_value<N-1>(i, p);
    }
};

template<>
struct mof_helper<1> {
    template<typename InputType, typename ports_type>
    static inline void output_converted_value(const InputType &i, ports_type &p) {
        // just emit a default-constructed object
        (void)std::get<0>(p).try_put(converter<InputType,typename std::tuple_element<0,ports_type>::type::output_type>::convert_value(i));
    }
};

template< typename InputType, typename OutputType >
struct harness_graph_default_functor {
    static OutputType construct( InputType v ) {
        return OutputType(v);
    }
};

template< typename OutputType >
struct harness_graph_default_functor< tbb::flow::continue_msg, OutputType > {
    static OutputType construct( tbb::flow::continue_msg ) {
        return OutputType();
    }
};

template< typename InputType >
struct harness_graph_default_functor< InputType, tbb::flow::continue_msg > {
    static tbb::flow::continue_msg construct( InputType ) {
        return tbb::flow::continue_msg();
    }
};

template< >
struct harness_graph_default_functor< tbb::flow::continue_msg, tbb::flow::continue_msg > {
    static tbb::flow::continue_msg construct( tbb::flow::continue_msg ) {
        return tbb::flow::continue_msg();
    }
};

template<typename InputType, typename OutputSet>
struct harness_graph_default_multifunction_functor {
    static const int N = std::tuple_size<OutputSet>::value;
    typedef typename tbb::flow::multifunction_node<InputType,OutputSet>::output_ports_type ports_type;
    static void construct(const InputType &i, ports_type &p) {
        mof_helper<N>::output_converted_value(i, p);
    }
};

//! An executor that accepts InputType and generates OutputType
template< typename InputType, typename OutputType >
struct harness_graph_executor {

    typedef OutputType (*function_ptr_type)( InputType v );

    template<typename RW>
    struct mutex_holder { static RW mutex; };

    static function_ptr_type fptr;
    static std::atomic<size_t> execute_count;
    static std::atomic<size_t> current_executors;
    static size_t max_executors;

    static inline OutputType func( InputType v ) {
        size_t c; // Declaration separate from initialization to avoid ICC internal error on IA-64 architecture
        c = current_executors++;
        if (max_executors != 0) {
            CHECK(c <= max_executors);
        }
        ++execute_count;
        OutputType v2 = (*fptr)(v);
        --current_executors;
        return v2;
    }

    template< typename RW >
    static inline OutputType tfunc( InputType v ) {
        // Invocations allowed to be concurrent, the lock is acquired in shared ("read") mode.
        // A test can take it exclusively, thus creating a barrier for invocations.
        typename RW::scoped_lock l( mutex_holder<RW>::mutex, /*write=*/false );
        return func(v);
    }

    template< typename RW >
    struct tfunctor {
        std::atomic<size_t> my_execute_count;
        tfunctor() { my_execute_count = 0; }
        tfunctor( const tfunctor &f ) { my_execute_count = static_cast<size_t>(f.my_execute_count); }
        OutputType operator()( InputType i ) {
           typename RW::scoped_lock l( harness_graph_executor::mutex_holder<RW>::mutex, /*write=*/false );
           ++my_execute_count;
           return harness_graph_executor::func(i);
        }
    };
    typedef tfunctor<tbb::null_rw_mutex> functor;

};

//! A multifunction executor that accepts InputType and has only one Output of OutputType.
template< typename InputType, typename OutputTuple >
struct harness_graph_multifunction_executor {
    typedef typename tbb::flow::multifunction_node<InputType,OutputTuple>::output_ports_type ports_type;
    typedef typename std::tuple_element<0,OutputTuple>::type OutputType;

    typedef void (*mfunction_ptr_type)( const InputType& v, ports_type &p );

    template<typename RW>
    struct mutex_holder { static RW mutex; };

    static mfunction_ptr_type fptr;
    static std::atomic<size_t> execute_count;
    static std::atomic<size_t> current_executors;
    static size_t max_executors;

    static inline void empty_func( const InputType&, ports_type& ) {
    }

    static inline void func( const InputType &v, ports_type &p ) {
        size_t c; // Declaration separate from initialization to avoid ICC internal error on IA-64 architecture
        c = current_executors++;
        CHECK( (max_executors == 0 || c <= max_executors) );
        CHECK( (std::tuple_size<OutputTuple>::value == 1) );
        ++execute_count;
        (*fptr)(v,p);
        --current_executors;
    }

    template< typename RW >
    static inline void tfunc( const InputType& v, ports_type &p ) {
        // Shared lock in invocations, exclusive in a test; see a comment in harness_graph_executor.
        typename RW::scoped_lock l( mutex_holder<RW>::mutex, /*write=*/false );
        func(v,p);
    }

    template< typename RW >
    struct tfunctor {
        std::atomic<size_t> my_execute_count;
        tfunctor() { my_execute_count = 0; }
        tfunctor( const tfunctor &f ) { my_execute_count = static_cast<size_t>(f.my_execute_count); }
        void operator()( const InputType &i, ports_type &p ) {
           typename RW::scoped_lock l( harness_graph_multifunction_executor::mutex_holder<RW>::mutex, /*write=*/false );
           ++my_execute_count;
           harness_graph_multifunction_executor::func(i,p);
        }
    };
    typedef tfunctor<tbb::null_rw_mutex> functor;

};

// static vars for function_node tests
template< typename InputType, typename OutputType >
template< typename RW >
RW harness_graph_executor<InputType, OutputType>::mutex_holder<RW>::mutex;

template< typename InputType, typename OutputType >
std::atomic<size_t> harness_graph_executor<InputType, OutputType>::execute_count;

template< typename InputType, typename OutputType >
typename harness_graph_executor<InputType, OutputType>::function_ptr_type harness_graph_executor<InputType, OutputType>::fptr
    = harness_graph_default_functor< InputType, OutputType >::construct;

template< typename InputType, typename OutputType >
std::atomic<size_t> harness_graph_executor<InputType, OutputType>::current_executors;

template< typename InputType, typename OutputType >
size_t harness_graph_executor<InputType, OutputType>::max_executors = 0;

// static vars for multifunction_node tests
template< typename InputType, typename OutputTuple >
template< typename RW >
RW harness_graph_multifunction_executor<InputType, OutputTuple>::mutex_holder<RW>::mutex;

template< typename InputType, typename OutputTuple >
std::atomic<size_t> harness_graph_multifunction_executor<InputType, OutputTuple>::execute_count;

template< typename InputType, typename OutputTuple >
typename harness_graph_multifunction_executor<InputType, OutputTuple>::mfunction_ptr_type harness_graph_multifunction_executor<InputType, OutputTuple>::fptr
    = harness_graph_default_multifunction_functor< InputType, OutputTuple >::construct;

template< typename InputType, typename OutputTuple >
std::atomic<size_t> harness_graph_multifunction_executor<InputType, OutputTuple>::current_executors;

template< typename InputType, typename OutputTuple >
size_t harness_graph_multifunction_executor<InputType, OutputTuple>::max_executors = 0;

//! Counts the number of puts received
template< typename T >
struct harness_counting_receiver : public tbb::flow::receiver<T> {
    harness_counting_receiver& operator=(const harness_counting_receiver&) = delete;

    std::atomic< size_t > my_count;
    T max_value;
    size_t num_copies;
    tbb::flow::graph& my_graph;

    harness_counting_receiver(tbb::flow::graph& g) : num_copies(1), my_graph(g) {
       my_count = 0;
    }

    void initialize_map( const T& m, size_t c ) {
       my_count = 0;
       max_value = m;
       num_copies = c;
    }

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }

    tbb::detail::d1::graph_task *try_put_task( const T & ) override {
      ++my_count;
      return const_cast<tbb::detail::d1::graph_task*>(SUCCESSFULLY_ENQUEUED);
    }

    void validate() {
        size_t n = my_count;
        CHECK( n == num_copies*max_value );
    }
 };

//! Counts the number of puts received
template< typename T >
struct harness_mapped_receiver : public tbb::flow::receiver<T> {
    harness_mapped_receiver(const harness_mapped_receiver&) = delete;
    harness_mapped_receiver& operator=(const harness_mapped_receiver&) = delete;

    std::atomic< size_t > my_count;
    T max_value;
    size_t num_copies;
    typedef tbb::concurrent_unordered_multiset<T> multiset_type;
    multiset_type *my_multiset;
    tbb::flow::graph& my_graph;

    harness_mapped_receiver(tbb::flow::graph& g) : my_multiset(nullptr), my_graph(g) {
       my_count = 0;
    }

#if __INTEL_COMPILER <= 2021
    // Suppress superfluous diagnostic about virtual keyword absence in a destructor of an inherited
    // class while the parent class has the virtual keyword for the destrocutor.
    virtual
#endif
    ~harness_mapped_receiver() {
        delete my_multiset;
        my_multiset = nullptr;
    }

    void initialize_map( const T& m, size_t c ) {
       my_count = 0;
       max_value = m;
       num_copies = c;
       delete my_multiset;
       my_multiset = new multiset_type;
    }

    tbb::detail::d1::graph_task* try_put_task( const T &t ) override {
      if ( my_multiset ) {
          (*my_multiset).emplace( t );
      } else {
          ++my_count;
      }
      return const_cast<tbb::detail::d1::graph_task*>(SUCCESSFULLY_ENQUEUED);
    }

    tbb::flow::graph& graph_reference() const override {
        return my_graph;
    }

    void validate() {
        if ( my_multiset ) {
            for ( size_t i = 0; i < (size_t)max_value; ++i ) {
                auto it = (*my_multiset).find((int)i);
                CHECK_MESSAGE( it != my_multiset->end(), "Expected element in the map." );
                size_t n = (*my_multiset).count(int(i));
                CHECK( n == num_copies );
            }
        } else {
            size_t n = my_count;
            CHECK( n == num_copies*max_value );
        }
    }

    void reset_receiver(tbb::flow::reset_flags /*f*/) {
        my_count = 0;
        if(my_multiset) delete my_multiset;
        my_multiset = new multiset_type;
    }

};

//! Counts the number of puts received
template< typename T >
struct harness_counting_sender : public tbb::flow::sender<T> {
    harness_counting_sender(const harness_counting_sender&) = delete;
    harness_counting_sender& operator=(const harness_counting_sender&) = delete;

    typedef typename tbb::flow::sender<T>::successor_type successor_type;
    std::atomic< successor_type * > my_receiver;
    std::atomic< size_t > my_count;
    std::atomic< size_t > my_received;
    size_t my_limit;

    harness_counting_sender( ) : my_limit(~size_t(0)) {
       my_receiver = nullptr;
       my_count = 0;
       my_received = 0;
    }

    harness_counting_sender( size_t limit ) : my_limit(limit) {
       my_receiver = nullptr;
       my_count = 0;
       my_received = 0;
    }

    bool register_successor( successor_type &r ) override {
        my_receiver = &r;
        return true;
    }

    bool remove_successor( successor_type &r ) override {
        successor_type *s = my_receiver.exchange( nullptr );
        CHECK( s == &r );
        return true;
    }

    bool try_get( T & v ) override {
        size_t i = my_count++;
        if ( i < my_limit ) {
           v = T( i );
           ++my_received;
           return true;
        } else {
           return false;
        }
    }

    bool try_put_once() {
        successor_type *s = my_receiver;
        size_t i = my_count++;
        if ( s->try_put( T(i) ) ) {
            ++my_received;
            return true;
        } else {
            return false;
        }
    }

    void try_put_until_false() {
        successor_type *s = my_receiver;
        size_t i = my_count++;

        while ( s->try_put( T(i) ) ) {
            ++my_received;
            i = my_count++;
        }
    }

    void try_put_until_limit() {
        successor_type *s = my_receiver;

        for ( int i = 0; i < (int)my_limit; ++i ) {
            CHECK( s->try_put( T(i) ) );
            ++my_received;
        }
        CHECK( my_received == my_limit );
    }

};

template< typename InputType >
struct parallel_put_until_limit {
    parallel_put_until_limit& operator=(const parallel_put_until_limit&) = delete;

    typedef std::vector< std::shared_ptr<harness_counting_sender<InputType>> > senders_t;

    senders_t& my_senders;

    parallel_put_until_limit( senders_t& senders ) : my_senders(senders) {}

    void operator()( int i ) const  { my_senders[i]->try_put_until_limit(); }
};

// test for resets of buffer-type nodes.
std::atomic<int> serial_fn_state0;
std::atomic<int> serial_fn_state1;
std::atomic<int> serial_continue_state0;

template<typename T>
struct serial_fn_body {
    std::atomic<int>& my_flag;
    serial_fn_body(std::atomic<int>& flag) : my_flag(flag) { }
    T operator()(const T& in) {
        if (my_flag == 0) {
            my_flag = 1;

            // wait until we are released
            utils::SpinWaitWhileEq(my_flag, 1);
        }
        return in;
    }
};

template<typename T>
struct serial_continue_body {
    std::atomic<int>& my_flag;
    serial_continue_body(std::atomic<int> &flag) : my_flag(flag) {}
    T operator()(const tbb::flow::continue_msg& /*in*/) {
        // signal we have received a value
        my_flag = 1;
        // wait until we are released
        utils::SpinWaitWhileEq(my_flag, 1);
        return (T)1;
    }
};

template<typename T, typename BufferType>
void test_resets() {
    const int NN = 3;
    bool nFound[NN];
    tbb::task_arena arena{4};
    arena.execute(
        [&] {
            tbb::task_group_context   tgc;
            tbb::flow::graph          g(tgc);
            BufferType                b0(g);
            tbb::flow::queue_node<T>  q0(g);
            T j{};

            // reset empties buffer
            for(T i = 0; i < NN; ++i) {
                b0.try_put(i);
                nFound[(int)i] = false;
            }
            g.wait_for_all();
            g.reset();
            CHECK_MESSAGE(!b0.try_get(j), "reset did not empty buffer");

            // reset doesn't delete edge

            tbb::flow::make_edge(b0,q0);
            g.wait_for_all(); // TODO: invesigate why make_edge to buffer_node always creates a forwarding task
            g.reset();
            for(T i = 0; i < NN; ++i) {
                b0.try_put(i);
            }

            g.wait_for_all();
            for( T i = 0; i < NN; ++i) {
                CHECK_MESSAGE(q0.try_get(j), "Missing value from buffer");
                CHECK_MESSAGE(!nFound[(int)j], "Duplicate value found");
                nFound[(int)j] = true;
            }

            for(int ii = 0; ii < NN; ++ii) {
                CHECK_MESSAGE(nFound[ii], "missing value");
            }
            CHECK_MESSAGE(!q0.try_get(j), "Extra values in output");

            // reset reverses a reversed edge.
            // we will use a serial rejecting node to get the edge to reverse.
            tbb::flow::function_node<T, T, tbb::flow::rejecting> sfn(g, tbb::flow::serial, serial_fn_body<T>(serial_fn_state0));
            tbb::flow::queue_node<T> outq(g);
            tbb::flow::remove_edge(b0,q0);
            tbb::flow::make_edge(b0, sfn);
            tbb::flow::make_edge(sfn,outq);
            g.wait_for_all();  // wait for all the tasks started by building the graph are done.
            serial_fn_state0 = 0;

            // b0 ------> sfn ------> outq
            for(int icnt = 0; icnt < 2; ++icnt) {
                g.wait_for_all();
                serial_fn_state0 = 0;
                std::thread t([&] {
                    b0.try_put((T)0);  // will start sfn
                    g.wait_for_all();  // wait for all the tasks to complete.
                });
                // wait until function_node starts
                utils::SpinWaitWhileEq(serial_fn_state0, 0);
                // now the function_node is executing.
                // this will start a task to forward the second item
                // to the serial function node
                b0.try_put((T)1);  // first item will be consumed by task completing the execution
                b0.try_put((T)2);  // second item will remain after cancellation
                // now wait for the task that attempts to forward the buffer item to
                // complete.
                // now cancel the graph.
                CHECK_MESSAGE(tgc.cancel_group_execution(), "task group already cancelled");
                serial_fn_state0 = 0;  // release the function_node.
                t.join();
                // check that at most one output reached the queue_node
                T outt;
                T outt2;
                bool got_item1 = outq.try_get(outt);
                bool got_item2 = outq.try_get(outt2);
                // either the output queue was empty (if the function_node tested for cancellation before putting the
                // result to the queue) or there was one element in the queue (the 0).
                bool is_successful_operation = got_item1 && (int)outt == 0 && !got_item2;
                CHECK_MESSAGE( is_successful_operation, "incorrect output from function_node");
                // the edge between the buffer and the function_node should be reversed, and the last
                // message we put in the buffer should still be there.  We can't directly test for the
                // edge reversal.
                got_item1 = b0.try_get(outt);
                CHECK_MESSAGE(got_item1, " buffer lost a message");
                is_successful_operation = (2 == (int)outt || 1 == (int)outt);
                CHECK_MESSAGE(is_successful_operation, " buffer had incorrect message");  // the one not consumed by the node.
                CHECK_MESSAGE(g.is_cancelled(), "Graph was not cancelled");
                g.reset();
            }  // icnt

            // reset with remove_edge removes edge.  (icnt ==0 => forward edge, 1 => reversed edge
            for(int icnt = 0; icnt < 2; ++icnt) {
                if(icnt == 1) {
                    // set up reversed edge
                    tbb::flow::make_edge(b0, sfn);
                    tbb::flow::make_edge(sfn,outq);
                    serial_fn_state0 = 0;
                    std::thread t([&] {
                        b0.try_put((T)0);  // starts up the function node
                        b0.try_put((T)1);  // should reverse the edge
                        g.wait_for_all();  // wait for all the tasks to complete.
                    });
                    utils::SpinWaitWhileEq(serial_fn_state0, 0); // waiting for edge reversal
                    CHECK_MESSAGE(tgc.cancel_group_execution(), "task group already cancelled");
                    serial_fn_state0 = 0;  // release the function_node.
                    t.join();
                }
                g.reset(tbb::flow::rf_clear_edges);
                // test that no one is a successor to the buffer now.
                serial_fn_state0 = 1;  // let the function_node go if it gets an input message
                b0.try_put((T)23);
                g.wait_for_all();
                CHECK_MESSAGE((int)serial_fn_state0 == 1, "function_node executed when it shouldn't");
                T outt;
                bool is_successful_operation = b0.try_get(outt) && (T)23 == outt && !outq.try_get(outt);
                CHECK_MESSAGE(is_successful_operation, "node lost its input");
            }
        }
    );                          // arena.execute()
}

template<typename NodeType>
void test_input_ports_return_ref(NodeType& mip_node) {
    typename NodeType::input_ports_type& input_ports1 = mip_node.input_ports();
    typename NodeType::input_ports_type& input_ports2 = mip_node.input_ports();
    CHECK_MESSAGE(&input_ports1 == &input_ports2, "input_ports() should return reference");
}

template<typename NodeType>
void test_output_ports_return_ref(NodeType& mop_node) {
    typename NodeType::output_ports_type& output_ports1 = mop_node.output_ports();
    typename NodeType::output_ports_type& output_ports2 = mop_node.output_ports();
    CHECK_MESSAGE(&output_ports1 == &output_ports2, "output_ports() should return reference");
}

template< template <typename> class ReservingNodeType, typename DataType, bool DoClear >
class harness_reserving_body {
    harness_reserving_body& operator=(const harness_reserving_body&) = delete;
    ReservingNodeType<DataType> &my_reserving_node;
    tbb::flow::buffer_node<DataType> &my_buffer_node;
public:
    harness_reserving_body(ReservingNodeType<DataType> &reserving_node, tbb::flow::buffer_node<DataType> &bn) : my_reserving_node(reserving_node), my_buffer_node(bn) {}
    void operator()(DataType i) const {
        my_reserving_node.try_put(i);
#if _MSC_VER && !__INTEL_COMPILER
#pragma warning (push)
#pragma warning (disable: 4127)  /* suppress conditional expression is constant */
#endif
        if (DoClear) {
#if _MSC_VER && !__INTEL_COMPILER
#pragma warning (pop)
#endif
            my_reserving_node.clear();
        }
        my_buffer_node.try_put(i);
        my_reserving_node.try_put(i);
    }
};

template< template <typename> class ReservingNodeType, typename DataType >
void test_reserving_nodes() {
#if TBB_TEST_LOW_WORKLOAD
    const int N = 30;
#else
    const int N = 300;
#endif

    tbb::flow::graph g;

    ReservingNodeType<DataType> reserving_n(g);

    tbb::flow::buffer_node<DataType> buffering_n(g);
    tbb::flow::join_node< std::tuple<DataType, DataType>, tbb::flow::reserving > join_n(g);
    harness_counting_receiver< std::tuple<DataType, DataType> > end_receiver(g);

    tbb::flow::make_edge(reserving_n, tbb::flow::input_port<0>(join_n));
    tbb::flow::make_edge(buffering_n, tbb::flow::input_port<1>(join_n));
    tbb::flow::make_edge(join_n, end_receiver);

    utils::NativeParallelFor(N, harness_reserving_body<ReservingNodeType, DataType, false>(reserving_n, buffering_n));
    g.wait_for_all();

    CHECK(end_receiver.my_count == N);

    // Should not hang
    utils::NativeParallelFor(N, harness_reserving_body<ReservingNodeType, DataType, true>(reserving_n, buffering_n));
    g.wait_for_all();

    CHECK(end_receiver.my_count == 2 * N);
}

namespace lightweight_testing {

typedef std::tuple<int, int> output_tuple_type;

template<typename NodeType>
class native_loop_body {
    native_loop_body& operator=(const native_loop_body&) = delete;
    NodeType& my_node;
public:
    native_loop_body(NodeType& node) : my_node(node) {}

    void operator()(int) const noexcept {
        std::thread::id this_id = std::this_thread::get_id();
        my_node.try_put(this_id);
    }
};

std::atomic<unsigned> g_body_count;

class concurrency_checker_body {
public:
    concurrency_checker_body() { g_body_count = 0; }

    template<typename gateway_type>
    void operator()(const std::thread::id& input, gateway_type&) noexcept { increase_and_check(input); }

    output_tuple_type operator()(const std::thread::id& input) noexcept {
        increase_and_check(input);
        return output_tuple_type();
    }

private:
    void increase_and_check(const std::thread::id& input) {
        ++g_body_count;
        std::thread::id body_thread_id = std::this_thread::get_id();
        CHECK_MESSAGE(input == body_thread_id, "Body executed as not lightweight");
    }
};

template<typename NodeType>
void test_unlimited_lightweight_execution(unsigned N) {
    tbb::flow::graph g;
    NodeType node(g, tbb::flow::unlimited, concurrency_checker_body());

    utils::NativeParallelFor(N, native_loop_body<NodeType>(node));
    g.wait_for_all();

    CHECK_MESSAGE(g_body_count == N, "Body needs to be executed N times");
}

std::mutex m;
std::condition_variable lightweight_condition;
std::atomic<bool> work_submitted;
std::atomic<bool> lightweight_work_processed;

template<typename NodeType>
class native_loop_limited_body {
    native_loop_limited_body& operator=(const native_loop_limited_body&) = delete;
    NodeType& my_node;
    utils::SpinBarrier& my_barrier;
public:
    native_loop_limited_body(NodeType& node, utils::SpinBarrier& barrier):
        my_node(node), my_barrier(barrier) {}
    void operator()(int) const noexcept {
        std::thread::id this_id = std::this_thread::get_id();
        my_node.try_put(this_id);
        if(!lightweight_work_processed) {
            my_barrier.wait();
            work_submitted = true;
            lightweight_condition.notify_all();
        }
    }
};

struct condition_predicate {
    bool operator()() {
        return work_submitted;
    }
};

std::atomic<unsigned> g_lightweight_count;
std::atomic<unsigned> g_task_count;

template <bool NoExcept>
class limited_lightweight_checker_body {
public:
    limited_lightweight_checker_body() {
        g_body_count = 0;
        g_lightweight_count = 0;
        g_task_count = 0;
    }
private:
    void increase_and_check(const std::thread::id& /*input*/) {
        ++g_body_count;

        bool is_inside_task = oneapi::tbb::task::current_context() != nullptr;

        if(is_inside_task) {
            ++g_task_count;
        } else {
            std::unique_lock<std::mutex> lock(m);
            lightweight_condition.wait(lock, condition_predicate());
            ++g_lightweight_count;
            lightweight_work_processed = true;
        }
    }
public:
    template<typename gateway_type>
    void operator()(const std::thread::id& input, gateway_type&) noexcept(NoExcept) {
        increase_and_check(input);
    }
    output_tuple_type operator()(const std::thread::id& input) noexcept(NoExcept) {
        increase_and_check(input);
        return output_tuple_type();
    }
};

template<typename NodeType>
void test_limited_lightweight_execution(unsigned N, unsigned concurrency) {
    CHECK_MESSAGE(concurrency != tbb::flow::unlimited,
                  "Test for limited concurrency cannot be called with unlimited concurrency argument");
    tbb::flow::graph g;
    NodeType node(g, concurrency, limited_lightweight_checker_body</*NoExcept*/true>());
    // Execute first body as lightweight, then wait for all other threads to fill internal buffer.
    // Then unblock the lightweight thread and check if other body executions are inside oneTBB task.
    utils::SpinBarrier barrier(N - concurrency);
    utils::NativeParallelFor(N, native_loop_limited_body<NodeType>(node, barrier));
    g.wait_for_all();
    CHECK_MESSAGE(g_body_count == N, "Body needs to be executed N times");
    CHECK_MESSAGE(g_lightweight_count == concurrency, "Body needs to be executed as lightweight once");
    CHECK_MESSAGE(g_task_count == N - concurrency, "Body needs to be executed as not lightweight N - 1 times");
    work_submitted = false;
    lightweight_work_processed = false;
}

template<typename NodeType>
void test_limited_lightweight_execution_with_throwing_body(unsigned N, unsigned concurrency) {
    CHECK_MESSAGE(concurrency != tbb::flow::unlimited,
                  "Test for limited concurrency cannot be called with unlimited concurrency argument");
    tbb::flow::graph g;
    NodeType node(g, concurrency, limited_lightweight_checker_body</*NoExcept*/false>());
    // Body is no noexcept, in this case it must be executed as tasks, instead of lightweight execution
    utils::SpinBarrier barrier(N);
    utils::NativeParallelFor(N, native_loop_limited_body<NodeType>(node, barrier));
    g.wait_for_all();
    CHECK_MESSAGE(g_body_count == N, "Body needs to be executed N times");
    CHECK_MESSAGE(g_lightweight_count == 0, "Body needs to be executed with queueing policy");
    CHECK_MESSAGE(g_task_count == N, "Body needs to be executed as task N times");
    work_submitted = false;
    lightweight_work_processed = false;
}

template <int Threshold>
struct throwing_body{
    std::atomic<int>& my_counter;

    throwing_body(std::atomic<int>& counter) : my_counter(counter) {}

    template<typename input_type, typename gateway_type>
    void operator()(const input_type&, gateway_type&) {
        ++my_counter;
        if(my_counter == Threshold)
            throw Threshold;
    }
    
    template<typename input_type>
    output_tuple_type operator()(const input_type&) {
        ++my_counter;
        if(my_counter == Threshold)
            throw Threshold;
        return output_tuple_type();
    }
};

#if TBB_USE_EXCEPTIONS
//! Test excesption thrown in node with lightweight policy was rethrown by graph
template<template<typename, typename, typename> class NodeType>
void test_exception_ligthweight_policy(){
    std::atomic<int> counter {0};
    constexpr int threshold = 10;

    using IndexerNodeType = oneapi::tbb::flow::indexer_node<int, int>;
    using FuncNodeType = NodeType<IndexerNodeType::output_type, output_tuple_type, tbb::flow::lightweight>;
    oneapi::tbb::flow::graph g;

    IndexerNodeType indexer(g);
    FuncNodeType tested_node(g, oneapi::tbb::flow::serial, throwing_body<threshold>(counter));
    oneapi::tbb::flow::make_edge(indexer, tested_node);

    utils::NativeParallelFor( threshold * 2, [&](int i){
        if(i % 2)
            std::get<1>(indexer.input_ports()).try_put(1);
        else
            std::get<0>(indexer.input_ports()).try_put(0);
    });

    bool catchException = false;
    try
    {
        g.wait_for_all();
    }
    catch (const int& exc)
    {
        catchException = true;
        CHECK_MESSAGE( exc == threshold, "graph.wait_for_all() rethrow current exception" );
    }
    CHECK_MESSAGE( catchException, "The exception must be thrown from graph.wait_for_all()" );
    CHECK_MESSAGE( counter == threshold, "Graph must cancel all tasks after exception" );
}
#endif /* TBB_USE_EXCEPTIONS */

template<typename NodeType>
void test_lightweight(unsigned N) {
    test_unlimited_lightweight_execution<NodeType>(N);
    test_limited_lightweight_execution<NodeType>(N, tbb::flow::serial);
    test_limited_lightweight_execution<NodeType>(N, (std::min)(std::thread::hardware_concurrency() / 2, N/2));

    test_limited_lightweight_execution_with_throwing_body<NodeType>(N, tbb::flow::serial);
}

template<template<typename, typename, typename> class NodeType>
void test(unsigned N) {
    typedef std::thread::id input_type;
    typedef NodeType<input_type, output_tuple_type, tbb::flow::queueing_lightweight> node_type;
    test_lightweight<node_type>(N);

#if TBB_USE_EXCEPTIONS
    test_exception_ligthweight_policy<NodeType>();
#endif /* TBB_USE_EXCEPTIONS */
}

} // namespace lightweight_testing

#endif  // __TBB_harness_graph_H
