/*
    Copyright (c) 2018-2021 Intel Corporation

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
#include "tbb/parallel_for.h"
#include "tbb/global_control.h"
#include "tbb/task_arena.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_concurrency_limit.h"
#include "common/spin_barrier.h"

#include <vector>
#include <cstdlib>
#include <random>
#include <algorithm>
#include <memory>


//! \file test_flow_graph_priorities.cpp
//! \brief Test for [flow_graph.copy_body flow_graph.function_node flow_graph.multifunction_node flow_graph.continue_node flow_graph.async_node] specification


using namespace tbb::flow;

struct TaskInfo {
    TaskInfo() : my_priority(-1), my_task_index(-1) {}
    TaskInfo( int priority, int task_index )
        : my_priority(priority), my_task_index(task_index) {}
    int my_priority;
    int my_task_index;
};

std::vector<TaskInfo> g_task_info;

std::atomic<unsigned> g_task_num;

void spin_for( double delta ) {
    tbb::tick_count start = tbb::tick_count::now();
    while( (tbb::tick_count::now() - start).seconds() < delta ) ;
}

namespace PriorityNodesTakePrecedence {

std::atomic<bool> g_work_submitted;

const unsigned node_num = 100;
const unsigned start_index = node_num / 3;
const unsigned end_index = node_num * 2 / 3;
std::atomic<unsigned> g_priority_task_index;

void body_func( int priority, utils::SpinBarrier& my_barrier ) {
    while( !g_work_submitted.load(std::memory_order_acquire) )
        tbb::detail::d0::yield();
    int current_task_index = g_task_num++;
    if( priority != no_priority )
        g_task_info[g_priority_task_index++] = TaskInfo( priority, current_task_index );
    const bool all_threads_will_come =
        unsigned(current_task_index) < node_num - (node_num % tbb::this_task_arena::max_concurrency());
    if( all_threads_will_come )
        my_barrier.wait();
}

typedef multifunction_node< int, std::tuple<int> > multi_node;

template <typename T>
struct Body {
    Body( int priority, utils::SpinBarrier& barrier )
        : my_priority( priority ), my_barrier( barrier ) {}
    T operator()( const T& msg ) const {
        body_func( my_priority, my_barrier );
        return msg;
    }
    void operator()( int msg, multi_node::output_ports_type& op ) const {
        body_func( my_priority, my_barrier );
        std::get<0>(op).try_put( msg );
    }
private:
    int my_priority;
    utils::SpinBarrier& my_barrier;
};

template<typename NodeType, typename BodyType>
struct node_creator_t {
    NodeType* operator()( graph& g, unsigned index, utils::SpinBarrier& barrier ) {
        if( start_index <= index && index < end_index )
            return new NodeType( g, unlimited, BodyType(index, barrier), node_priority_t(index) );
        else
            return new NodeType( g, unlimited, BodyType(no_priority, barrier) );
    }
};

template<typename BodyType>
struct node_creator_t< continue_node<continue_msg>, BodyType > {
    continue_node<continue_msg>* operator()( graph& g, unsigned index, utils::SpinBarrier& barrier ) {
        if( start_index <= index && index < end_index )
            return new continue_node<continue_msg>( g, BodyType(index, barrier), node_priority_t(index) );
        else
            return new continue_node<continue_msg>( g, BodyType(no_priority, barrier) );
    }
};


struct passthru_body {
    template<typename T>
    continue_msg operator()( T ) const { return continue_msg(); }
};

template<typename NodeType, typename NodeTypeCreator>
void test_node( NodeTypeCreator node_creator ) {
    const int num_threads = tbb::this_task_arena::max_concurrency();
    utils::SpinBarrier barrier( num_threads );
    graph g;
    broadcast_node<typename NodeType::input_type> bn(g);
    function_node<typename NodeType::input_type> tn(g, unlimited, passthru_body());
    // Using pointers to nodes to avoid errors on compilers, which try to generate assignment
    // operator for the nodes
    std::vector< std::unique_ptr<NodeType> > nodes;
    for( unsigned i = 0; i < node_num; ++i ) {
        nodes.push_back(std::unique_ptr<NodeType>( node_creator(g, i, barrier) ));
        make_edge( bn, *nodes.back() );
        make_edge( *nodes.back(), tn );
    }

    const size_t repeats = 10;
    const size_t priority_nodes_num = end_index - start_index;
    size_t global_order_failures = 0;
    for( size_t repeat = 0; repeat < repeats; ++repeat ) {
        g_work_submitted.store( false, std::memory_order_release );
        g_task_num = g_priority_task_index = 0;
        g_task_info.clear(); g_task_info.resize( priority_nodes_num );

        bn.try_put( typename NodeType::input_type{} );
        // Setting of the flag is based on the knowledge that the calling thread broadcasts the
        // message to successor nodes. Thus, once the calling thread returns from try_put() call all
        // necessary tasks are spawned. Thus, this makes this test to be a whitebox test to some
        // extent.
        g_work_submitted.store( true, std::memory_order_release );

        g.wait_for_all();

        CHECK_MESSAGE( (g_priority_task_index == g_task_info.size()), "Incorrect number of tasks with priority." );
        CHECK_MESSAGE( (priority_nodes_num == g_task_info.size()), "Incorrect number of tasks with priority executed." );

        for( unsigned i = 0; i < g_priority_task_index; i += num_threads ) {
            bool found = false;
            unsigned highest_priority_within_group = end_index - i - 1;
            for( unsigned j = i; j < i+num_threads; ++j ) {
                if( g_task_info[j].my_priority == int(highest_priority_within_group) ) {
                    found = true;
                    break;
                }
            }
            CHECK_MESSAGE( found, "Highest priority task within a group was not found" );
        }
        for( unsigned i = 0; i < g_priority_task_index; ++i ) {
            // This check might fail because priorities do not guarantee ordering, i.e. assumption
            // that all priority nodes should increment the task counter before any subsequent
            // no-priority node is not correct. In the worst case, a thread that took a priority
            // node might be preempted and become the last to increment the counter. That's why the
            // test passing is based on statistics, which could be affected by machine overload
            // unfortunately.
            // TODO revamp: reconsider the following check for this test
            if( g_task_info[i].my_task_index > int(priority_nodes_num + num_threads) )
                ++global_order_failures;
        }
    }
    float failure_ratio = float(global_order_failures) / float(repeats*priority_nodes_num);
    CHECK_MESSAGE(
        failure_ratio <= 0.1f,
        "Nodes with priorities executed in wrong order too frequently over non-prioritized nodes."
    );
}

template<typename NodeType, typename NodeBody>
void call_within_arena( tbb::task_arena& arena ) {
    arena.execute(
        [&]() {
            test_node<NodeType>( node_creator_t<NodeType, NodeBody>() );
        }
    );
}

void test( int num_threads ) {
    INFO( "Testing execution of nodes with priority takes precedence (num_threads=" << num_threads << ") - " );
    tbb::task_arena arena(num_threads);
    call_within_arena< function_node<int,int>, Body<int> >( arena );
    call_within_arena< multi_node, Body<int> >( arena );
    call_within_arena< continue_node<continue_msg>, Body<continue_msg> >( arena );
}

} /* namespace PriorityNodesTakePrecedence */

namespace ThreadsEagerReaction {

// TODO revamp: combine with similar queue from test_async_node
template <typename T>
class concurrent_queue {
public:
    bool try_pop(T& item) {
        std::lock_guard<queue_mutex> lock(mutex);
        if ( q.empty() )
            return false;
        item = q.front();
        q.pop();
        return true;
    }

    void push(const T& item) {
        std::lock_guard<queue_mutex> lock(mutex);
        q.push(item);
    }
private:
    std::queue<T> q;
    using queue_mutex = std::mutex;
    std::mutex mutex;
};

using utils::SpinBarrier;

enum task_type_t { no_task, regular_task, async_task };

struct profile_t {
    task_type_t task_type;
    unsigned global_task_id;
    double elapsed;
};

std::vector<unsigned> g_async_task_ids;

typedef unsigned data_type;
typedef async_node<data_type, data_type> async_node_type;
typedef multifunction_node<
    data_type, std::tuple<data_type, data_type> > decider_node_type;
struct AsyncActivity {
    typedef async_node_type::gateway_type gateway_type;

    struct work_type { data_type input; gateway_type* gateway; };
    std::atomic<bool> done;
    concurrent_queue<work_type> my_queue;
    std::thread my_service_thread;

    struct ServiceThreadFunc {
        SpinBarrier& my_barrier;
        ServiceThreadFunc(SpinBarrier& barrier) : my_barrier(barrier) {}
        void operator()(AsyncActivity* activity) {
            while (!activity->done) {
                work_type work;
                while (activity->my_queue.try_pop(work)) {
                    g_async_task_ids.push_back( ++g_task_num );
                    work.gateway->try_put(work.input);
                    work.gateway->release_wait();
                    my_barrier.wait();
                }
            }
        }
    };
    void stop_and_wait() { done = true; my_service_thread.join(); }

    void submit(data_type input, gateway_type* gateway) {
        work_type work = { input, gateway };
        gateway->reserve_wait();
        my_queue.push(work);
    }
    AsyncActivity(SpinBarrier& barrier)
        : done(false), my_service_thread(ServiceThreadFunc(barrier), this) {}
};

struct StartBody {
    bool has_run;
    data_type operator()(tbb::flow_control& fc) {
        if (has_run){
            fc.stop();
            return data_type();
        }
        has_run = true;
        return 1;
    }
    StartBody() : has_run(false) {}
};

struct ParallelForBody {
    SpinBarrier& my_barrier;
    const data_type& my_input;
    ParallelForBody(SpinBarrier& barrier, const data_type& input)
        : my_barrier(barrier), my_input(input) {}
    void operator()(const data_type&) const {
        my_barrier.wait();
        ++g_task_num;
    }
};

struct CpuWorkBody {
    SpinBarrier& my_barrier;
    const int my_tasks_count;
    data_type operator()(const data_type& input) {
        tbb::parallel_for(0, my_tasks_count, ParallelForBody(my_barrier, input), tbb::simple_partitioner());
        return input;
    }
    CpuWorkBody(SpinBarrier& barrier, int tasks_count)
        : my_barrier(barrier), my_tasks_count(tasks_count) {}
};

struct DeciderBody {
    const data_type my_limit;
    DeciderBody( const data_type& limit ) : my_limit( limit ) {}
    void operator()(data_type input, decider_node_type::output_ports_type& ports) {
        if (input < my_limit)
            std::get<0>(ports).try_put(input + 1);
    }
};

struct AsyncSubmissionBody {
    AsyncActivity* my_activity;
    // It is important that async_node in the test executes without spawning a TBB task, because
    // it passes the work to asynchronous thread, which unlocks the barrier that is waited
    // by every execution thread (asynchronous thread and any TBB worker or main thread).
    // This is why async_node's body marked noexcept.
    void operator()(data_type input, async_node_type::gateway_type& gateway) noexcept {
        my_activity->submit(input, &gateway);
    }
    AsyncSubmissionBody(AsyncActivity* activity) : my_activity(activity) {}
};

void test( unsigned num_threads ) {
    INFO( "Testing threads react eagerly on asynchronous tasks (num_threads=" << num_threads << ") - " );
    if( num_threads == std::thread::hardware_concurrency() ) {
        // one thread is required for asynchronous compute resource
        INFO("skipping test since it is designed to work on less number of threads than "
             "hardware concurrency allows\n");
        return;
    }
    const unsigned cpu_threads = unsigned(num_threads);
    const unsigned cpu_tasks_per_thread = 4;
    const unsigned nested_cpu_tasks = cpu_tasks_per_thread * cpu_threads;
    const unsigned async_subgraph_reruns = 8;
    const unsigned cpu_subgraph_reruns = 2;

    SpinBarrier barrier(cpu_threads + /*async thread=*/1);
    g_task_num = 0;
    g_async_task_ids.clear();
    g_async_task_ids.reserve(async_subgraph_reruns);

    tbb::task_arena arena(cpu_threads);
	arena.execute(
        [&]() {
            AsyncActivity activity(barrier);
            graph g;

            input_node<data_type> starter_node(g, StartBody());
            function_node<data_type, data_type> cpu_work_node(
                g, unlimited, CpuWorkBody(barrier, nested_cpu_tasks));
            decider_node_type cpu_restarter_node(g, unlimited, DeciderBody(cpu_subgraph_reruns));
            async_node_type async_node(g, unlimited, AsyncSubmissionBody(&activity));
            decider_node_type async_restarter_node(
                g, unlimited, DeciderBody(async_subgraph_reruns), node_priority_t(1)
            );

            make_edge(starter_node, cpu_work_node);
            make_edge(cpu_work_node, cpu_restarter_node);
            make_edge(output_port<0>(cpu_restarter_node), cpu_work_node);

            make_edge(starter_node, async_node);
            make_edge(async_node, async_restarter_node);
            make_edge(output_port<0>(async_restarter_node), async_node);

            starter_node.activate();
            g.wait_for_all();
            activity.stop_and_wait();

            const size_t async_task_num = size_t(async_subgraph_reruns);
            CHECK_MESSAGE( ( g_async_task_ids.size() == async_task_num), "Incorrect number of async tasks." );
            unsigned max_span = unsigned(2 * cpu_threads + 1);
            for( size_t idx = 1; idx < async_task_num; ++idx ) {
                CHECK_MESSAGE( (g_async_task_ids[idx] - g_async_task_ids[idx-1] <= max_span),
                               "Async tasks were not able to interfere with CPU tasks." );

            }
        }
    );
    INFO("done\n");
}
} /* ThreadsEagerReaction */

namespace LimitingExecutionToPriorityTask {

enum work_type_t { NONPRIORITIZED_WORK, PRIORITIZED_WORK };

struct execution_tracker_t {
    execution_tracker_t() { reset(); }
    void reset() {
        prioritized_work_submitter = std::thread::id();
        prioritized_work_started = false;
        prioritized_work_finished = false;
        prioritized_work_interrupted = false;
    }
    std::thread::id prioritized_work_submitter;
    std::atomic<bool> prioritized_work_started;
    bool prioritized_work_finished;
    bool prioritized_work_interrupted;
} exec_tracker;

template<work_type_t work_type>
void do_node_work( int work_size );

template<work_type_t>
void do_nested_work( const std::thread::id& tid, const tbb::blocked_range<int>& subrange );

template<work_type_t work_type>
struct CommonBody {
    CommonBody() : my_body_size( 0 ) { }
    CommonBody( int body_size ) : my_body_size( body_size ) { }
    continue_msg operator()( const continue_msg& msg ) const {
        do_node_work<work_type>(my_body_size);
        return msg;
    }
    void operator()( const tbb::blocked_range<int>& subrange ) const {
        do_nested_work<work_type>( /*tid=*/std::this_thread::get_id(), subrange );
    }
    int my_body_size;
};

template<work_type_t work_type>
void do_node_work(int work_size) {
    tbb::parallel_for( tbb::blocked_range<int>(0, work_size), CommonBody<work_type>(),
                       tbb::simple_partitioner() );
}

template<work_type_t>
void do_nested_work( const std::thread::id& tid, const tbb::blocked_range<int>& /*subrange*/ ) {
    // This is non-prioritized work...
    if( !exec_tracker.prioritized_work_started || exec_tracker.prioritized_work_submitter != tid )
        return;
    // ...being executed by the thread that initially started prioritized one...
    CHECK_MESSAGE( exec_tracker.prioritized_work_started,
                   "Prioritized work should have been started by that time." );
    // ...prioritized work has been started already...
    if( exec_tracker.prioritized_work_finished )
        return;
    // ...but has not been finished yet
    exec_tracker.prioritized_work_interrupted = true;
}

struct IsolationFunctor {
    int work_size;
    IsolationFunctor(int ws) : work_size(ws) {}
    void operator()() const {
        tbb::parallel_for( tbb::blocked_range<int>(0, work_size), CommonBody<PRIORITIZED_WORK>(),
                           tbb::simple_partitioner() );
    }
};

template<>
void do_node_work<PRIORITIZED_WORK>(int work_size) {
    exec_tracker.prioritized_work_submitter = std::this_thread::get_id();
    exec_tracker.prioritized_work_started = true;
    tbb::this_task_arena::isolate( IsolationFunctor(work_size) );
    exec_tracker.prioritized_work_finished = true;
}

template<>
void do_nested_work<PRIORITIZED_WORK>( const std::thread::id& tid,
                                       const tbb::blocked_range<int>& /*subrange*/ ) {
    if( exec_tracker.prioritized_work_started && exec_tracker.prioritized_work_submitter == tid ) {
        CHECK_MESSAGE( !exec_tracker.prioritized_work_interrupted,
                       "Thread was not fully devoted to processing of prioritized task." );
    } else {
        // prolong processing of prioritized work so that the thread that started
        // prioritized work has higher probability to help with non-prioritized one.
        spin_for(0.1);
    }
}

// Using pointers to nodes to avoid errors on compilers, which try to generate assignment operator
// for the nodes
typedef std::vector< std::unique_ptr<continue_node<continue_msg>> > nodes_container_t;

void create_nodes( nodes_container_t& nodes, graph& g, int num, int body_size ) {
    for( int i = 0; i < num; ++i )
        nodes.push_back(
            std::unique_ptr<continue_node<continue_msg>>(
                new continue_node<continue_msg>( g, CommonBody<NONPRIORITIZED_WORK>( body_size ) )
            )
        );
}

void test( int num_threads ) {
    INFO( "Testing limit execution to priority tasks (num_threads=" << num_threads << ") - " );

    tbb::task_arena arena( num_threads );
	arena.execute(
        [&]() {
            const int nodes_num = 100;
            const int priority_node_position_part = 10;
            const int pivot = nodes_num / priority_node_position_part;
            const int nodes_in_lane = 3 * num_threads;
            const int small_problem_size = 100;
            const int large_problem_size = 1000;

            graph g;
            nodes_container_t nodes;
            create_nodes( nodes, g, pivot, large_problem_size );
            nodes.push_back(
                std::unique_ptr<continue_node<continue_msg>>(
                    new continue_node<continue_msg>(
                        g, CommonBody<PRIORITIZED_WORK>(small_problem_size), node_priority_t(1)
                    )
                )
            );
            create_nodes( nodes, g, nodes_num - pivot - 1, large_problem_size );

            broadcast_node<continue_msg> bn(g);
            for( int i = 0; i < nodes_num; ++i )
                if( i % nodes_in_lane == 0 )
                    make_edge( bn, *nodes[i] );
                else
                    make_edge( *nodes[i-1], *nodes[i] );
            exec_tracker.reset();
            bn.try_put( continue_msg() );
            g.wait_for_all();
        }
	);

    INFO( "done\n" );
}

} /* namespace LimitingExecutionToPriorityTask */

namespace NestedCase {

using tbb::task_arena;

struct InnerBody {
    continue_msg operator()( const continue_msg& ) const {
        return continue_msg();
    }
};

struct OuterBody {
    int my_max_threads;
    task_arena** my_inner_arena;
    OuterBody( int max_threads, task_arena** inner_arena )
        : my_max_threads(max_threads), my_inner_arena(inner_arena) {}
    // copy constructor to please some old compilers
    OuterBody( const OuterBody& rhs )
        : my_max_threads(rhs.my_max_threads), my_inner_arena(rhs.my_inner_arena) {}
    int operator()( const int& ) {
        graph inner_graph;
        continue_node<continue_msg> start_node(inner_graph, InnerBody());
        continue_node<continue_msg> mid_node1(inner_graph, InnerBody(), node_priority_t(5));
        continue_node<continue_msg> mid_node2(inner_graph, InnerBody());
        continue_node<continue_msg> end_node(inner_graph, InnerBody(), node_priority_t(15));
        make_edge( start_node, mid_node1 );
        make_edge( mid_node1, end_node );
        make_edge( start_node, mid_node2 );
        make_edge( mid_node2, end_node );
        (*my_inner_arena)->execute( [&inner_graph]{ inner_graph.reset(); } );
        start_node.try_put( continue_msg() );
        inner_graph.wait_for_all();
        return 13;
    }
};

void execute_outer_graph( bool same_arena, task_arena& inner_arena, int max_threads,
                          graph& outer_graph, function_node<int,int>& start_node ) {
    if( same_arena ) {
        start_node.try_put( 42 );
        outer_graph.wait_for_all();
        return;
    }

    auto threads_range = utils::concurrency_range(max_threads);
    for( auto num_threads : threads_range ) {
        inner_arena.initialize( static_cast<int>(num_threads) );
        start_node.try_put( 42 );
        outer_graph.wait_for_all();
        inner_arena.terminate();
    }
}

void test_in_arena( int max_threads, task_arena& outer_arena, task_arena& inner_arena,
                    graph& outer_graph, function_node<int, int>& start_node ) {
    bool same_arena = &outer_arena == &inner_arena;
    auto threads_range = utils::concurrency_range(max_threads);
    for( auto num_threads : threads_range ) {
        INFO( "Testing nested nodes with specified priority in " << (same_arena? "same" : "different")
              << " arenas, num_threads=" << num_threads << ") - " );
        outer_arena.initialize( static_cast<int>(num_threads) );
        outer_arena.execute( [&outer_graph]{ outer_graph.reset(); } );
        execute_outer_graph( same_arena, inner_arena, max_threads, outer_graph, start_node );
        outer_arena.terminate();
        INFO( "done\n" );
    }
}

void test( int max_threads ) {
    task_arena outer_arena; task_arena inner_arena;
    task_arena* inner_arena_pointer = &outer_arena; // make it same as outer arena in the beginning

    graph outer_graph;
    const unsigned num_outer_nodes = 10;
    const size_t concurrency = unlimited;
    std::vector< std::unique_ptr<function_node<int,int>> > outer_nodes;
    for( unsigned node_index = 0; node_index < num_outer_nodes; ++node_index ) {
        node_priority_t priority = no_priority;
        if( node_index == num_outer_nodes / 2 )
            priority = 10;

        outer_nodes.push_back(
            std::unique_ptr< function_node<int, int> >(
                new function_node<int,int>(
                    outer_graph, concurrency, OuterBody(max_threads, &inner_arena_pointer), priority
                )
            )
        );
    }

    for( unsigned node_index1 = 0; node_index1 < num_outer_nodes; ++node_index1 )
        for( unsigned node_index2 = node_index1+1; node_index2 < num_outer_nodes; ++node_index2 )
            make_edge( *outer_nodes[node_index1], *outer_nodes[node_index2] );

    test_in_arena( max_threads, outer_arena, outer_arena, outer_graph, *outer_nodes[0] );

    inner_arena_pointer = &inner_arena;

    test_in_arena( max_threads, outer_arena, inner_arena, outer_graph, *outer_nodes[0] );
}
} // namespace NestedCase


namespace BypassPrioritizedTask {

void common_body( int priority ) {
    int current_task_index = g_task_num++;
    g_task_info.push_back( TaskInfo( priority, current_task_index ) );
}

struct Body {
    Body( int priority ) : my_priority( priority ) {}
    continue_msg operator()(const continue_msg&) {
        common_body( my_priority );
        return continue_msg();
    }
    int my_priority;
};

struct InputNodeBody {
    continue_msg operator()( tbb::flow_control& fc ){
        static bool is_source_executed = false;

        if( is_source_executed ) {
            fc.stop();
            return continue_msg();
        }

        common_body( 0 );
        is_source_executed = true;

        return continue_msg();
    }
};

template<typename StarterNodeType>
StarterNodeType create_starter_node(graph& g) {
    return continue_node<continue_msg>( g, Body(0) );
}

template<>
input_node<continue_msg> create_starter_node<input_node<continue_msg>>(graph& g) {
    return input_node<continue_msg>( g, InputNodeBody() );
}

template<typename StarterNodeType>
void start_graph( StarterNodeType& starter ) {
    starter.try_put( continue_msg() );
}

template<>
void start_graph<input_node<continue_msg>>( input_node<continue_msg>& starter ) {
    starter.activate();
}

template<typename StarterNodeType>
void test_use_case() {
    g_task_info.clear();
    g_task_num = 0;
    graph g;
    StarterNodeType starter = create_starter_node<StarterNodeType>(g);
    continue_node<continue_msg> spawn_successor( g, Body(1), node_priority_t(1) );
    continue_node<continue_msg> bypass_successor( g, Body(2), node_priority_t(2) );

    make_edge( starter, spawn_successor );
    make_edge( starter, bypass_successor );

    start_graph<StarterNodeType>( starter );
    g.wait_for_all();

    CHECK_MESSAGE( g_task_info.size() == 3, "" );
    CHECK_MESSAGE( g_task_info[0].my_task_index == 0, "" );
    CHECK_MESSAGE( g_task_info[1].my_task_index == 1, "" );
    CHECK_MESSAGE( g_task_info[2].my_task_index == 2, "" );

    CHECK_MESSAGE( g_task_info[0].my_priority == 0, "" );
    CHECK_MESSAGE( g_task_info[1].my_priority == 2, "Bypassed task with higher priority executed in wrong order." );
    CHECK_MESSAGE( g_task_info[2].my_priority == 1, "" );
}

//! The test checks that the task from the node with higher priority, which task gets bypassed, is
//! executed first than the one spawned with lower priority.
void test() {
    test_use_case<continue_node<continue_msg>>();
    test_use_case<input_node<continue_msg>>();
}

} // namespace BypassPrioritizedTask

namespace ManySuccessors {

struct no_priority_node_body {
    void operator()(continue_msg) {
        CHECK_MESSAGE(
            barrier == 0, "Non-priority successor has to be executed after all priority successors"
        );
    }
    std::atomic<int>& barrier;
};

struct priority_node_body {
    void operator()(continue_msg) {
        --barrier;
        while (barrier)
            tbb::detail::d0::yield();
    }
    std::atomic<int>& barrier;
};

void test(int num_threads) {
    tbb::task_arena arena( num_threads );
    arena.execute(
        [&]() {
            graph g;
            broadcast_node<continue_msg> bn(g);
            std::vector< std::unique_ptr<continue_node<continue_msg>> > nodes;
            std::atomic<int> barrier;
            for (int i = 0; i < 2 * num_threads; ++i)
                nodes.push_back(
                    std::unique_ptr<continue_node<continue_msg>>(
                        new continue_node<continue_msg>(g, no_priority_node_body{ barrier })
                    )
                );
            for (int i = 0; i < num_threads; ++i)
                nodes.push_back(
                    std::unique_ptr<continue_node<continue_msg>>(
                        new continue_node<continue_msg>(g, priority_node_body{ barrier }, /*priority*/1)
                    )
                );

            std::random_device rd;
            std::mt19937 gen(rd());

            for (int trial = 0; trial < 10; ++trial) {
                barrier = num_threads;
                std::shuffle(nodes.begin(), nodes.end(), gen);
                for (auto& n : nodes)
                    make_edge(bn, *n);
                bn.try_put(continue_msg());
                g.wait_for_all();
                for (auto& n : nodes)
                    remove_edge(bn, *n);
            }
        }
    );
}

} // namespace ManySuccessors

#if TBB_USE_EXCEPTIONS
namespace Exceptions {
    void test() {
        using namespace tbb::flow;
        graph g;
        std::srand(42);
        const unsigned num_messages = 50;
        std::vector<unsigned> throwing_msgs;
        std::atomic<unsigned> msg_count(0);
        continue_node<unsigned> c(g, [&msg_count](continue_msg) {
            return ++msg_count;
        }, 2);
        function_node<unsigned> f(g, unlimited, [&throwing_msgs](unsigned v) {
            for( auto i : throwing_msgs ) {
                if( i == v )
                    throw std::runtime_error("Exception::test");
            }
        }, 1);
        make_edge(c, f);
        for (int i = 0; i < 10; ++i) {
            msg_count = 0;
            g.reset();
            throwing_msgs.push_back(std::rand() % num_messages);
            try {
                for (unsigned j = 0; j < num_messages; ++j) {
                    c.try_put(continue_msg());
                }
                g.wait_for_all();
                FAIL("Unreachable code. The exception is expected");
            } catch (std::runtime_error&) {
                CHECK(g.is_cancelled());
                CHECK(g.exception_thrown());
            } catch (...) {
                FAIL("Unexpected exception");
            }
        }
    }
} // namespace Exceptions
#endif

//! Test node prioritization
//! \brief \ref requirement
TEST_CASE("Priority nodes take precedence"){
    for( auto p : utils::concurrency_range() ) {
        PriorityNodesTakePrecedence::test( static_cast<int>(p) );
    }
}

//! Test thread eager reaction
//! \brief \ref error_guessing
TEST_CASE("Thread eager reaction"){
    for( auto p : utils::concurrency_range() ) {
        ThreadsEagerReaction::test( static_cast<int>(p) );
    }
}

//! Test prioritization under concurrency limits
//! \brief \ref error_guessing
TEST_CASE("Limiting execution to prioritized work") {
    for( auto p : utils::concurrency_range() ) {
        LimitingExecutionToPriorityTask::test( static_cast<int>(p) );
    }
}

//! Test nested graphs
//! \brief \ref error_guessing
TEST_CASE("Nested test case") {
    std::size_t max_threads = utils::get_platform_max_threads();
    // The stepping for the threads is done inside.
    NestedCase::test( static_cast<int>(max_threads) );
}

//! Test bypassed task with higher priority
//! \brief \ref error_guessing
TEST_CASE("Bypass prioritized task"){
    tbb::global_control gc( tbb::global_control::max_allowed_parallelism, 1 );
    BypassPrioritizedTask::test();
}

//! Test mixing prioritized and ordinary successors
//! \brief \ref error_guessing
TEST_CASE("Many successors") {
    for( auto p : utils::concurrency_range() ) {
        ManySuccessors::test( static_cast<int>(p) );
    }
}

#if TBB_USE_EXCEPTIONS
//! Test for exceptions
//! \brief \ref error_guessing
TEST_CASE("Exceptions") {
    Exceptions::test();
}
#endif
