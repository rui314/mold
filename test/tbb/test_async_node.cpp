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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "tbb/task.h"
#include "tbb/global_control.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/graph_utils.h"
#include "common/spin_barrier.h"
#include "common/test_follows_and_precedes_api.h"
#include "common/concepts_common.h"

#include <string>
#include <thread>
#include <mutex>


//! \file test_async_node.cpp
//! \brief Test for [flow_graph.async_node] specification


class minimal_type {
    template<typename T>
    friend struct place_wrapper;

    int value;

public:
    minimal_type() : value(-1) {}
    minimal_type(int v) : value(v) {}
    minimal_type(const minimal_type &m) : value(m.value) { }
    minimal_type &operator=(const minimal_type &m) { value = m.value; return *this; }
};

template <typename T>
struct place_wrapper {
    typedef T wrapped_type;
    T value;
    std::thread::id thread_id;

    place_wrapper( int v = 0 ) : value(v), thread_id(std::this_thread::get_id()) {}

    template <typename Q>
    place_wrapper(const place_wrapper<Q>& v)
        : value(v.value), thread_id(v.thread_id)
    {}

    template <typename Q>
    place_wrapper<Q>& operator=(const place_wrapper<Q>& v) {
        if (this != &v) {
            value = v.value;
            thread_id = v.thread_id;
        }
        return *this;
    }

};

template<typename T1, typename T2>
struct wrapper_helper {
    static void check(const T1 &, const T2 &) { }
    static void copy_value(const T1 &in, T2 &out) { out = in; }
};

template<typename T1, typename T2>
struct wrapper_helper< place_wrapper<T1>, place_wrapper<T2> > {
    static void check(const place_wrapper<T1> &a, const place_wrapper<T2> &b) {
       CHECK_MESSAGE( ( (a.thread_id != b.thread_id)), "same thread used to execute adjacent nodes");
       return;
    }
    static void copy_value(const place_wrapper<T1> &in, place_wrapper<T2> &out) {
        out.value = in.value;
    }
};

const int NUMBER_OF_MSGS = 10;
const int UNKNOWN_NUMBER_OF_ITEMS = -1;
std::atomic<int> async_body_exec_count;
std::atomic<int> async_activity_processed_msg_count;
std::atomic<int> end_body_exec_count;

// queueing required in test_reset for testing of cancellation
typedef tbb::flow::async_node< int, int, tbb::flow::queueing > counting_async_node_type;
typedef counting_async_node_type::gateway_type counting_gateway_type;

struct counting_async_unlimited_body {

    counting_async_unlimited_body(tbb::task_group_context& graph_tgc) : my_tgc( graph_tgc ) {}

    void operator()( const int &input, counting_gateway_type& gateway) {
        // TODO revamp: reconsider logging for the tests. It is known that frequent calls to
        // doctest's INFO cause issues.

        // INFO( "Body execution with input == " << input << "\n");
        ++async_body_exec_count;
        if ( input == -1 ) {
            bool result = my_tgc.cancel_group_execution();
            // INFO( "Canceling graph execution\n" );
            CHECK_MESSAGE( ( result == true), "attempted to cancel graph twice" );
            utils::Sleep(50);
        }
        gateway.try_put(input);
    }
private:
    tbb::task_group_context& my_tgc;
};

struct counting_async_serial_body : counting_async_unlimited_body {
    typedef counting_async_unlimited_body base_type;
    int my_async_body_exec_count;

    counting_async_serial_body(tbb::task_group_context& tgc)
        : base_type(tgc), my_async_body_exec_count( 0 ) { }

    void operator()( const int &input, counting_gateway_type& gateway ) {
        ++my_async_body_exec_count;
        base_type::operator()( input, gateway );
    }
};

void test_reset() {
    const int N = NUMBER_OF_MSGS;
    async_body_exec_count = 0;

    tbb::task_group_context graph_ctx;
    tbb::flow::graph g(graph_ctx);
    counting_async_node_type a(g, tbb::flow::serial, counting_async_serial_body(graph_ctx) );

    const int R = 3;
    std::vector< std::shared_ptr<harness_counting_receiver<int>> > r;
    for (size_t i = 0; i < R; ++i) {
        r.push_back( std::make_shared<harness_counting_receiver<int>>(g) );
    }

    for (int i = 0; i < R; ++i) {
        tbb::flow::make_edge(a, *r[i]);
    }

    INFO( "One body execution\n" );
    a.try_put(-1);
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    // should be canceled with only 1 item reaching the async_body and the counting receivers
    // and N items left in the node's queue
    CHECK_MESSAGE( ( g.is_cancelled() == true), "task group not canceled" );

    counting_async_serial_body b1 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( ( int(async_body_exec_count) == int(b1.my_async_body_exec_count)), "body and global body counts are different" );
    CHECK_MESSAGE( ( int(async_body_exec_count) == 1), "global body execution count not 1"  );
    for (int i = 0; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == 1), "counting receiver count not 1" );
    }

    // should clear the async_node queue, but retain its local count at 1 and keep all edges
    g.reset(tbb::flow::rf_reset_protocol);

    INFO( "N body executions\n" );
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( ( g.is_cancelled() == false), "task group not canceled" );

    // a total of N+1 items should have passed through the node body
    // the local body count should also be N+1
    // and the counting receivers should all have a count of N+1
    counting_async_serial_body b2 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( int(async_body_exec_count) == int(b2.my_async_body_exec_count),
                   "local and global body execution counts are different" );
    INFO( "async_body_exec_count==" << int(async_body_exec_count) << "\n" );
    CHECK_MESSAGE( ( int(async_body_exec_count) == N+1), "global body execution count not N+1"  );
    for (int i = 0; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == N+1), "counting receiver has not received N+1 items" );
    }

    INFO( "N body executions with new bodies\n" );
    // should clear the async_node queue and reset its local count to 0, but keep all edges
    g.reset(tbb::flow::rf_reset_bodies);
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( ( g.is_cancelled() == false), "task group not canceled" );

    // a total of 2N+1 items should have passed through the node body
    // the local body count should be N
    // and the counting receivers should all have a count of 2N+1
    counting_async_serial_body b3 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( ( int(async_body_exec_count) == 2*N+1), "global body execution count not 2N+1"  );
    CHECK_MESSAGE( ( int(b3.my_async_body_exec_count) == N), "local body execution count not N"  );
    for (int i = 0; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == 2*N+1), "counting receiver has not received 2N+1 items" );
    }

    // should clear the async_node queue and keep its local count at N and remove all edges
    INFO( "N body executions with no edges\n" );
    g.reset(tbb::flow::rf_clear_edges);
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( ( g.is_cancelled() == false), "task group not canceled" );

    // a total of 3N+1 items should have passed through the node body
    // the local body count should now be 2*N
    // and the counting receivers should remain at a count of 2N+1
    counting_async_serial_body b4 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( ( int(async_body_exec_count) == 3*N+1), "global body execution count not 3N+1"  );
    CHECK_MESSAGE( ( int(b4.my_async_body_exec_count) == 2*N), "local body execution count not 2N"  );
    for (int i = 0; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == 2*N+1), "counting receiver has not received 2N+1 items" );
    }

    // put back 1 edge to receiver 0
    INFO( "N body executions with 1 edge\n" );
    tbb::flow::make_edge(a, *r[0]);
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( ( g.is_cancelled() == false), "task group not canceled" );

    // a total of 4N+1 items should have passed through the node body
    // the local body count should now be 3*N
    // and all of the counting receivers should remain at a count of 2N+1, except r[0] which should be 3N+1
    counting_async_serial_body b5 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( ( int(async_body_exec_count) == 4*N+1), "global body execution count not 4N+1"  );
    CHECK_MESSAGE( ( int(b5.my_async_body_exec_count) == 3*N), "local body execution count not 3N"  );
    CHECK_MESSAGE( ( int(r[0]->my_count) == 3*N+1), "counting receiver has not received 3N+1 items" );
    for (int i = 1; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == 2*N+1), "counting receiver has not received 2N+1 items" );
    }

    // should clear the async_node queue and keep its local count at N and remove all edges
    INFO( "N body executions with no edges and new body\n" );
    g.reset(static_cast<tbb::flow::reset_flags>(tbb::flow::rf_reset_bodies|tbb::flow::rf_clear_edges));
    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();
    CHECK_MESSAGE( ( g.is_cancelled() == false), "task group not canceled" );

    // a total of 4N+1 items should have passed through the node body
    // the local body count should now be 3*N
    // and all of the counting receivers should remain at a count of 2N+1, except r[0] which should be 3N+1
    counting_async_serial_body b6 = tbb::flow::copy_body<counting_async_serial_body>(a);
    CHECK_MESSAGE( ( int(async_body_exec_count) == 5*N+1), "global body execution count not 5N+1"  );
    CHECK_MESSAGE( ( int(b6.my_async_body_exec_count) == N), "local body execution count not N"  );
    CHECK_MESSAGE( ( int(r[0]->my_count) == 3*N+1), "counting receiver has not received 3N+1 items" );
    for (int i = 1; i < R; ++i) {
        CHECK_MESSAGE( ( int(r[i]->my_count) == 2*N+1), "counting receiver has not received 2N+1 items" );
    }
}


#include <mutex>

template <typename T>
class async_activity_queue {
public:
    void push( const T& item ) {
        std::lock_guard<mutex_t> lock( m_mutex );
        m_queue.push( item );
    }

    bool try_pop( T& item ) {
        std::lock_guard<mutex_t> lock( m_mutex );
        if( m_queue.empty() )
            return false;
        item = m_queue.front();
        m_queue.pop();
        return true;
    }

    bool empty() {
        std::lock_guard<mutex_t> lock( m_mutex );
        return m_queue.empty();
    }

private:
    typedef std::mutex mutex_t;
    mutex_t m_mutex;
    std::queue<T> m_queue;
};

template< typename Input, typename Output >
class async_activity : utils::NoAssign {
public:
    typedef Input input_type;
    typedef Output output_type;
    typedef tbb::flow::async_node< input_type, output_type > async_node_type;
    typedef typename async_node_type::gateway_type gateway_type;

    struct work_type {
        input_type input;
        gateway_type* gateway;
    };

    class ServiceThreadBody {
    public:
        ServiceThreadBody( async_activity* activity ) : my_activity( activity ) {}
        void operator()() { my_activity->process(); }
    private:
        async_activity* my_activity;
    };

    async_activity(int expected_items, bool deferred = false, int sleep_time = 50)
        : my_expected_items(expected_items), my_sleep_time(sleep_time)
    {
        is_active = !deferred;
        my_quit = false;
        std::thread( ServiceThreadBody( this ) ).swap( my_service_thread );
    }

private:

    async_activity( const async_activity& )
        : my_expected_items(UNKNOWN_NUMBER_OF_ITEMS), my_sleep_time(0)
    {
        is_active = true;
    }

public:
    ~async_activity() {
        stop();
        my_service_thread.join();
    }

    void submit( const input_type &input, gateway_type& gateway ) {
        work_type work = {input, &gateway};
        my_work_queue.push( work );
    }

    void process() {
        do {
            work_type work;
            if( is_active && my_work_queue.try_pop( work ) ) {
                utils::Sleep(my_sleep_time);
                ++async_activity_processed_msg_count;
                output_type output;
                wrapper_helper<output_type, output_type>::copy_value(work.input, output);
                wrapper_helper<output_type, output_type>::check(work.input, output);
                work.gateway->try_put(output);
                if ( my_expected_items == UNKNOWN_NUMBER_OF_ITEMS ||
                     int(async_activity_processed_msg_count) == my_expected_items ) {
                    work.gateway->release_wait();
                }
            }
        } while( my_quit == false || !my_work_queue.empty());
    }

    void stop() {
        my_quit = true;
    }

    void activate() {
        is_active = true;
    }

    bool should_reserve_each_time() {
        if ( my_expected_items == UNKNOWN_NUMBER_OF_ITEMS )
            return true;
        else
            return false;
    }

private:

    const int my_expected_items;
    const int my_sleep_time;
    std::atomic< bool > is_active;

    async_activity_queue<work_type> my_work_queue;

    std::atomic< bool > my_quit;

    std::thread my_service_thread;
};

template<typename Input, typename Output>
struct basic_test {
    typedef Input input_type;
    typedef Output output_type;
    typedef tbb::flow::async_node< input_type, output_type > async_node_type;
    typedef typename async_node_type::gateway_type gateway_type;

    basic_test() {}

    static int run(int async_expected_items = UNKNOWN_NUMBER_OF_ITEMS) {
        async_activity<input_type, output_type> my_async_activity(async_expected_items);

        tbb::flow::graph g;

        tbb::flow::function_node< int, input_type > start_node(
            g, tbb::flow::unlimited, [](int input) { return input_type(input); }
        );
        async_node_type offload_node(
            g, tbb::flow::unlimited,
            [&] (const input_type &input, gateway_type& gateway) {
                ++async_body_exec_count;
                if(my_async_activity.should_reserve_each_time())
                    gateway.reserve_wait();
                my_async_activity.submit(input, gateway);
            }
        );
        tbb::flow::function_node< output_type > end_node(
            g, tbb::flow::unlimited,
            [&](const output_type& input) {
                ++end_body_exec_count;
                output_type output;
                wrapper_helper<output_type, output_type>::check(input, output);
            }
        );

        tbb::flow::make_edge( start_node, offload_node );
        tbb::flow::make_edge( offload_node, end_node );

        async_body_exec_count = 0;
        async_activity_processed_msg_count = 0;
        end_body_exec_count = 0;

        if (async_expected_items != UNKNOWN_NUMBER_OF_ITEMS) {
            offload_node.gateway().reserve_wait();
        }
        for (int i = 0; i < NUMBER_OF_MSGS; ++i) {
            start_node.try_put(i);
        }
        g.wait_for_all();
        CHECK_MESSAGE( ( async_body_exec_count == NUMBER_OF_MSGS), "AsyncBody processed wrong number of signals" );
        CHECK_MESSAGE( ( async_activity_processed_msg_count == NUMBER_OF_MSGS), "AsyncActivity processed wrong number of signals" );
        CHECK_MESSAGE( ( end_body_exec_count == NUMBER_OF_MSGS), "EndBody processed wrong number of signals");
        INFO( "async_body_exec_count == " << int(async_body_exec_count) <<
              " == async_activity_processed_msg_count == " << int(async_activity_processed_msg_count) <<
              " == end_body_exec_count == " << int(end_body_exec_count) << "\n"
        );
        return 0;
    }

};

int test_copy_ctor() {
    const int N = NUMBER_OF_MSGS;
    async_body_exec_count = 0;

    tbb::flow::graph g;

    harness_counting_receiver<int> r1(g);
    harness_counting_receiver<int> r2(g);

    tbb::task_group_context graph_ctx;
    counting_async_node_type a(g, tbb::flow::unlimited, counting_async_unlimited_body(graph_ctx) );
    counting_async_node_type b(a);

    tbb::flow::make_edge(a, r1);                             // C++11-style of making edges
    tbb::flow::make_edge(tbb::flow::output_port<0>(b), r2);  // usual way of making edges

    for (int i = 0; i < N; ++i) {
       a.try_put(i);
    }
    g.wait_for_all();

    INFO("async_body_exec_count = " << int(async_body_exec_count) << "\n" );
    INFO("r1.my_count == " << int(r1.my_count) << " and r2.my_count = " << int(r2.my_count) << "\n" );
    CHECK_MESSAGE( ( int(async_body_exec_count) == NUMBER_OF_MSGS), "AsyncBody processed wrong number of signals" );
    CHECK_MESSAGE( ( int(r1.my_count) == N), "counting receiver r1 has not received N items" );
    CHECK_MESSAGE( ( int(r2.my_count) == 0), "counting receiver r2 has not received 0 items" );

    for (int i = 0; i < N; ++i) {
       b.try_put(i);
    }
    g.wait_for_all();

    INFO("async_body_exec_count = " << int(async_body_exec_count) << "\n" );
    INFO("r1.my_count == " << int(r1.my_count) << " and r2.my_count = " << int(r2.my_count) << "\n" );
    CHECK_MESSAGE( ( int(async_body_exec_count) == 2*NUMBER_OF_MSGS), "AsyncBody processed wrong number of signals" );
    CHECK_MESSAGE( ( int(r1.my_count) == N), "counting receiver r1 has not received N items" );
    CHECK_MESSAGE( ( int(r2.my_count) == N), "counting receiver r2 has not received N items" );
    return 0;
}

std::atomic<int> main_tid_count;

template<typename Input, typename Output>
struct spin_test {
    typedef Input input_type;
    typedef Output output_type;
    typedef tbb::flow::async_node< input_type, output_type > async_node_type;
    typedef typename async_node_type::gateway_type gateway_type;

    class end_body_type {
        typedef Output output_type;
        std::thread::id my_main_tid;
        utils::SpinBarrier *my_barrier;
    public:
        end_body_type(std::thread::id t, utils::SpinBarrier &b) : my_main_tid(t), my_barrier(&b) { }

        void operator()( const output_type & ) {
            ++end_body_exec_count;
            if (std::this_thread::get_id() == my_main_tid) {
               ++main_tid_count;
            }
            my_barrier->wait();
        }
    };

    spin_test() {}

    static int run(int nthreads, int async_expected_items = UNKNOWN_NUMBER_OF_ITEMS) {
        async_activity<input_type, output_type> my_async_activity(async_expected_items, false, 0);
        const int overall_message_count = nthreads * NUMBER_OF_MSGS;
        utils::SpinBarrier spin_barrier(nthreads);

        tbb::flow::graph g;
        tbb::flow::function_node<int, input_type> start_node(
            g, tbb::flow::unlimited, [](int input) { return input_type(input); }
        );
        async_node_type offload_node(
            g, tbb::flow::unlimited,
            [&](const input_type &input, gateway_type& gateway) {
                ++async_body_exec_count;
                if(my_async_activity.should_reserve_each_time())
                    gateway.reserve_wait();
                my_async_activity.submit(input, gateway);
            }
        );
        tbb::flow::function_node<output_type> end_node(
            g, tbb::flow::unlimited, end_body_type(std::this_thread::get_id(), spin_barrier)
        );

        tbb::flow::make_edge( start_node, offload_node );
        tbb::flow::make_edge( offload_node, end_node );

        async_body_exec_count = 0;
        async_activity_processed_msg_count = 0;
        end_body_exec_count = 0;
        main_tid_count = 0;

        if (async_expected_items != UNKNOWN_NUMBER_OF_ITEMS ) {
            offload_node.gateway().reserve_wait();
        }
        for (int i = 0; i < overall_message_count; ++i) {
            start_node.try_put(i);
        }
        g.wait_for_all();
        CHECK_MESSAGE( (async_body_exec_count == overall_message_count),
                       "AsyncBody processed wrong number of signals" );
        CHECK_MESSAGE( (async_activity_processed_msg_count == overall_message_count),
                       "AsyncActivity processed wrong number of signals" );
        CHECK_MESSAGE( (end_body_exec_count == overall_message_count),
                       "EndBody processed wrong number of signals");

        INFO( "Main thread participated in " << main_tid_count << " end_body tasks\n");

        INFO("async_body_exec_count == " << int(async_body_exec_count) <<
             " == async_activity_processed_msg_count == " << int(async_activity_processed_msg_count) <<
             " == end_body_exec_count == " << int(end_body_exec_count) << "\n"
        );
        return 0;
    }

};

void test_for_spin_avoidance() {
    const int nthreads = 4;
    tbb::global_control gc(tbb::global_control::max_allowed_parallelism, nthreads);
    tbb::task_arena a(nthreads);
    a.execute([&] {
        spin_test<int, int>::run(nthreads);
    });
}

template< typename Input, typename Output >
int run_tests() {
    basic_test<Input, Output>::run();
    basic_test<Input, Output>::run(NUMBER_OF_MSGS);
    basic_test<place_wrapper<Input>, place_wrapper<Output> >::run();
    basic_test<place_wrapper<Input>, place_wrapper<Output> >::run(NUMBER_OF_MSGS);
    return 0;
}

#include "tbb/parallel_for.h"
template<typename Input, typename Output>
class enqueueing_on_inner_level {
    typedef Input input_type;
    typedef Output output_type;
    typedef async_activity<input_type, output_type> async_activity_type;
    typedef tbb::flow::async_node<Input, Output> async_node_type;
    typedef typename async_node_type::gateway_type gateway_type;

    class body_graph_with_async {
    public:
        body_graph_with_async( utils::SpinBarrier& barrier, async_activity_type& activity )
            : spin_barrier(&barrier), my_async_activity(&activity) {}

        void operator()(int) const {
            tbb::flow::graph g;
            tbb::flow::function_node< int, input_type > start_node(
                g, tbb::flow::unlimited, [](int input) { return input_type(input); }
            );
            async_node_type offload_node(
                g, tbb::flow::unlimited,
                [&](const input_type &input, gateway_type& gateway) {
                    gateway.reserve_wait();
                    my_async_activity->submit( input, gateway );
                }
            );
            tbb::flow::function_node< output_type > end_node( g, tbb::flow::unlimited, [](output_type){} );

            tbb::flow::make_edge( start_node, offload_node );
            tbb::flow::make_edge( offload_node, end_node );

            start_node.try_put(1);

            spin_barrier->wait();

            my_async_activity->activate();

            g.wait_for_all();
        }

    private:
        utils::SpinBarrier* spin_barrier;
        async_activity_type* my_async_activity;
    };

public:
    static int run ()
    {
        const int nthreads = tbb::this_task_arena::max_concurrency();
        utils::SpinBarrier spin_barrier( nthreads );

        async_activity_type my_async_activity( UNKNOWN_NUMBER_OF_ITEMS, true );

        tbb::parallel_for( 0, nthreads, body_graph_with_async( spin_barrier, my_async_activity ) );
        return 0;
    }
};

int run_test_enqueueing_on_inner_level() {
    enqueueing_on_inner_level<int, int>::run();
    return 0;
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>

template<typename NodeType>
class AsyncActivity {
public:
    using gateway_t = typename NodeType::gateway_type;

    struct work_type {
        int input;
        gateway_t* gateway;
    };

    AsyncActivity(size_t limit) : stop_limit(limit), c(0), thr([this]() {
        while(!end_of_work()) {
            work_type w;
            while( my_q.try_pop(w) ) {
                int res = do_work(w.input);
                w.gateway->try_put(res);
                w.gateway->release_wait();
                ++c;
            }
        }
    }) {}

    void submit(int i, gateway_t* gateway) {
        work_type w = {i, gateway};
        gateway->reserve_wait();
        my_q.push(w);
    }

    void wait_for_all() { thr.join(); }

private:
    bool end_of_work() { return c >= stop_limit; }

    int do_work(int& i) { return i + i; }

    async_activity_queue<work_type> my_q;
    size_t stop_limit;
    size_t c;
    std::thread thr;
};

void test_follows() {
    using namespace tbb::flow;

    using input_t = int;
    using output_t = int;
    using node_t = async_node<input_t, output_t>;

    graph g;

    AsyncActivity<node_t> async_activity(3);

    std::array<broadcast_node<input_t>, 3> preds = {
      {
        broadcast_node<input_t>(g),
        broadcast_node<input_t>(g),
        broadcast_node<input_t>(g)
      }
    };

    node_t node(follows(preds[0], preds[1], preds[2]), unlimited, [&](int input, node_t::gateway_type& gtw) {
        async_activity.submit(input, &gtw);
    }, no_priority);

    buffer_node<output_t> buf(g);
    make_edge(node, buf);

    for(auto& pred: preds) {
        pred.try_put(1);
    }

    g.wait_for_all();
    async_activity.wait_for_all();

    output_t storage;
    CHECK_MESSAGE((buf.try_get(storage) && buf.try_get(storage) && buf.try_get(storage) && !buf.try_get(storage)),
                  "Not exact edge quantity was made");
}

void test_precedes() {
    using namespace tbb::flow;

    using input_t = int;
    using output_t = int;
    using node_t = async_node<input_t, output_t>;

    graph g;

    AsyncActivity<node_t> async_activity(1);

    std::array<buffer_node<input_t>, 1> successors = { {buffer_node<input_t>(g)} };

    broadcast_node<input_t> start(g);

    node_t node(precedes(successors[0]), unlimited, [&](int input, node_t::gateway_type& gtw) {
        async_activity.submit(input, &gtw);
    }, no_priority);

    make_edge(start, node);

    start.try_put(1);

    g.wait_for_all();
    async_activity.wait_for_all();

    for(auto& successor : successors) {
        output_t storage;
        CHECK_MESSAGE((successor.try_get(storage) && !successor.try_get(storage)),
                      "Not exact edge quantity was made");
    }
}

void test_follows_and_precedes_api() {
    test_follows();
    test_precedes();
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

//! Test async bodies processing
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Basic tests"){
    tbb::task_arena arena(utils::MaxThread);
    arena.execute(
        [&]() {
            run_tests<int, int>();
            run_tests<minimal_type, minimal_type>();
            run_tests<int, minimal_type>();
        }
    );
}

//! NativeParallelFor test with various concurrency settings
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Lightweight tests"){
    lightweight_testing::test<tbb::flow::async_node>(NUMBER_OF_MSGS);
}

//! Test reset and cancellation
//! \brief \ref error_guessing
TEST_CASE("Reset test"){
    test_reset();
}

//! Test
//! \brief \ref requirement \ref error_guessing
TEST_CASE("Copy constructor test"){
    test_copy_ctor();
}

//! Test if main thread spins
//! \brief \ref stress
TEST_CASE("Spin avoidance test"){
    test_for_spin_avoidance();
}

//! Test nested enqueuing
//! \brief \ref error_guessing
TEST_CASE("Inner enqueuing test"){
    run_test_enqueueing_on_inner_level();
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test deprecated follows and preceedes API
//! \brief \ref error_guessing
TEST_CASE("Test follows and preceedes API"){
    test_follows_and_precedes_api();
}
#endif

#if __TBB_CPP20_CONCEPTS_PRESENT
//! \brief \ref error_guessing
TEST_CASE("constraints for async_node input") {
    struct InputObject {
        InputObject() = default;
        InputObject( const InputObject& ) = default;
    };

    static_assert(utils::well_formed_instantiation<tbb::flow::async_node, InputObject, int>);
    static_assert(utils::well_formed_instantiation<tbb::flow::async_node, int, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::async_node, test_concepts::NonCopyable, int>);
    static_assert(!utils::well_formed_instantiation<tbb::flow::async_node, test_concepts::NonDefaultInitializable, int>);
}

template <typename Input, typename Output, typename Body>
concept can_call_async_node_ctor = requires( tbb::flow::graph& graph, std::size_t concurrency,
                                             Body body, tbb::flow::node_priority_t priority, tbb::flow::buffer_node<int>& f ) {
    tbb::flow::async_node<Input, Output>(graph, concurrency, body);
    tbb::flow::async_node<Input, Output>(graph, concurrency, body, priority);
#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    tbb::flow::async_node<Input, Output>(tbb::flow::follows(f), concurrency, body);
    tbb::flow::async_node<Input, Output>(tbb::flow::follows(f), concurrency, body, priority);
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
};

//! \brief \ref error_guessing
TEST_CASE("constraints for async_node body") {
    using input_type = int;
    using output_type = input_type;
    using namespace test_concepts::async_node_body;

    static_assert(can_call_async_node_ctor<input_type, output_type, Correct<input_type, output_type>>);
    static_assert(!can_call_async_node_ctor<input_type, output_type, NonCopyable<input_type, output_type>>);
    static_assert(!can_call_async_node_ctor<input_type, output_type, NonDestructible<input_type, output_type>>);
    static_assert(!can_call_async_node_ctor<input_type, output_type, NoOperatorRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_async_node_ctor<input_type, output_type, WrongFirstInputOperatorRoundBrackets<input_type, output_type>>);
    static_assert(!can_call_async_node_ctor<input_type, output_type, WrongSecondInputOperatorRoundBrackets<input_type, output_type>>);
}

#endif // __TBB_CPP20_CONCEPTS_PRESENT
