/*
    Copyright (c) 2020-2023 Intel Corporation

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

#ifndef __TBB_test_conformance_conformance_flowgraph_H
#define __TBB_test_conformance_conformance_flowgraph_H

#include "common/test.h"
#include "common/utils.h"
#include "common/graph_utils.h"
#include "common/concurrency_tracker.h"

#include "oneapi/tbb/flow_graph.h"
#include "oneapi/tbb/task_arena.h"
#include "oneapi/tbb/global_control.h"

namespace conformance {

constexpr int expected = 5;

template<typename V>
using test_push_receiver = oneapi::tbb::flow::queue_node<V>;

template<typename Input, typename Output = Input>
using multifunc_ports_t =
      typename oneapi::tbb::flow::multifunction_node<Input, std::tuple<Output>>::output_ports_type;

template<typename Input, typename Output = Input>
using async_ports_t =
      typename oneapi::tbb::flow::async_node<Input, Output>::gateway_type;

template<bool DefaultConstructible, bool CopyConstructible, bool CopyAssignable>
struct message {
    int data;

    message(int _data) : data(_data) {};

    template<bool T = DefaultConstructible, typename = typename std::enable_if<T>::type>
    message(){};

    template<bool T = CopyConstructible, typename = typename std::enable_if<T>::type>
    message(const message& msg) : data(msg.data) {};

    template<bool T = CopyAssignable, typename = typename std::enable_if<T>::type>
    message& operator=(const message& msg) {
        this->data = msg.data;
        return *this;
    };

    bool operator==(const int expected_data) const {
        return data == expected_data;
    }

    bool operator==(const message& msg) const {
        return data == msg.data;
    }

    operator std::size_t() const {
        return static_cast<std::size_t>(data);
    }

    operator int() const {
        return data;
    }
};

template<typename V>
typename std::enable_if<!std::is_default_constructible<V>::value, std::vector<V>>::type get_values( test_push_receiver<V>& rr ) {
    std::vector<V> messages;
    V tmp(0);

    while (rr.try_get(tmp)) {
        messages.push_back(tmp);
    }
    return messages;
}

template<typename V>
typename std::enable_if<std::is_default_constructible<V>::value, std::vector<V>>::type get_values( test_push_receiver<V>& rr ) {
    std::vector<V> messages;
    V tmp;

    while (rr.try_get(tmp)) {
        messages.push_back(tmp);
    }
    return messages;
}

template<typename Node, typename InputType = void>
bool produce_messages(Node& node, int arg = 1) {
    utils::suppress_unused_warning(arg);
#if defined CONFORMANCE_INPUT_NODE
    node.activate();
    return true;
#elif defined CONFORMANCE_CONTINUE_NODE
    return node.try_put(InputType());
#else
    return node.try_put(InputType(arg));
#endif
}

template<typename T, typename U>
typename std::enable_if<std::is_same<T, U>::value, bool>::type check_output_type(){
    return true;
}

template<typename T, typename U>
typename std::enable_if<!std::is_same<T, U>::value, bool>::type check_output_type(){
    return false;
}

template<typename T>
struct sequencer_functor {
    struct seq_message {
        std::size_t id;
        T data;
    };

    using input_type = T;

    std::size_t operator()(T v) {
        return v;
    }

    std::size_t operator()(seq_message msg) {
        return msg.id;
    }
};

template<typename OutputType>
struct track_first_id_functor {
    int my_id;
    static std::atomic<int> first_id;

    track_first_id_functor(int id) : my_id(id) {}

    OutputType operator()( OutputType argument ) {
        int old_value = -1;
        while(first_id == -1 &&
              !first_id.compare_exchange_strong(old_value, my_id));
        return argument;
    }

    template<typename InputType>
    OutputType operator()( InputType& ) {
        return operator()(OutputType(0));
    }

    template<typename InputType>
    void operator()( InputType, async_ports_t<InputType, OutputType>& g ) {
        g.try_put(operator()(OutputType(0)));
    }

    template<typename InputType>
    void operator()( InputType, multifunc_ports_t<InputType, OutputType>& op ) {
        std::get<0>(op).try_put(operator()(OutputType(0)));
    }
};

template<typename OutputType>
std::atomic<int> track_first_id_functor<OutputType>::first_id = {-1};

template<typename OutputType>
struct counting_functor {
    OutputType return_value;

    static std::atomic<std::size_t> execute_count;

    counting_functor( OutputType value = OutputType(0) ) : return_value(value) {
        execute_count = 0;
    }

    counting_functor( const counting_functor & c ) : return_value(static_cast<int>(c.return_value)) {
        execute_count = 0;
    }

    template<typename InputType>
    OutputType operator()( InputType ) {
        ++execute_count;
        return return_value;
    }

    template<typename InputType>
    void operator()( InputType, multifunc_ports_t<InputType, OutputType>& op ) {
        ++execute_count;
        std::get<0>(op).try_put(return_value);
    }

    OutputType operator()( oneapi::tbb::flow_control& fc ) {
        ++execute_count;
        if(execute_count > std::size_t(return_value)) {
            fc.stop();
            return return_value;
        }
        return return_value;
    }

    template<typename InputType>
    void operator()( InputType, async_ports_t<InputType, OutputType>& g ) {
        ++execute_count;
        g.try_put(return_value);
    }
};

template<typename OutputType>
std::atomic<std::size_t> counting_functor<OutputType>::execute_count = {0};

template<typename OutputType>
struct dummy_functor {
    template<typename InputType>
    OutputType operator()( InputType ) {
#ifdef CONFORMANCE_CONTINUE_NODE
        return OutputType();
#else
        return OutputType(0);
#endif
    }

    template<typename InputType>
    void operator()( InputType, multifunc_ports_t<InputType, OutputType>& op ) {
        std::get<0>(op).try_put(OutputType(0));
    }

    template<typename InputType>
    void operator()( InputType, async_ports_t<InputType, OutputType>& g ) {
        g.try_put(OutputType(0));
    }

    template<typename InputType, typename T>
    void operator()( InputType, std::tuple<T, T>& ) {}

    OutputType operator()( oneapi::tbb::flow_control & fc ) {
        static bool check = false;
        if(check) {
            check = false;
            fc.stop();
            return OutputType(1);
        }
        check = true;
        return OutputType(1);
    }
};

struct wait_flag_body {
    static std::atomic<bool> flag;

    wait_flag_body() {
        flag.store(false);
    }

    template<typename InputType>
    InputType operator()( InputType ) {
        while(!flag.load()) { utils::yield(); };
#ifdef CONFORMANCE_CONTINUE_NODE
        return InputType();
#else
        return InputType(0);
#endif
    }

    template<typename InputType>
    void operator()( InputType argument, multifunc_ports_t<InputType>& op ) {
        while(!flag.load()) { };
        std::get<0>(op).try_put(argument);
    }

    template<typename InputType>
    void operator()( InputType argument, async_ports_t<InputType>& g ) {
        while(!flag.load()) { };
        g.try_put(argument);
    }
};

std::atomic<bool> wait_flag_body::flag{false};

struct concurrency_peak_checker_body {
    std::size_t required_max_concurrency = 0;

    concurrency_peak_checker_body( std::size_t req_max_concurrency = 0 ) :
                                    required_max_concurrency(req_max_concurrency) {}

    concurrency_peak_checker_body( const concurrency_peak_checker_body & ) = default;

    int operator()( oneapi::tbb::flow_control & fc ) {
        static int counter = 0;
        utils::ConcurrencyTracker ct;
        if(++counter > 500) {
            counter = 0;
            fc.stop();
            return 1;
        }
        utils::doDummyWork(1000);
        CHECK_MESSAGE((int)utils::ConcurrencyTracker::PeakParallelism() <= required_max_concurrency,
        "Input node is serial and its body never invoked concurrently");
        return 1;
    }

    int operator()( int ) {
        utils::ConcurrencyTracker ct;
        utils::doDummyWork(1000);
        CHECK_MESSAGE((int)utils::ConcurrencyTracker::PeakParallelism() <= required_max_concurrency,
        "Measured parallelism is not expected");
        return 1;
    }

    void operator()( const int& argument, multifunc_ports_t<int>& op ) {
        utils::ConcurrencyTracker ct;
        utils::doDummyWork(1000);
        CHECK_MESSAGE((int)utils::ConcurrencyTracker::PeakParallelism() <= required_max_concurrency,
        "Measured parallelism is not expected");
        std::get<0>(op).try_put(argument);
    }

    void operator()( const int& argument , async_ports_t<int>& g ) {
        utils::ConcurrencyTracker ct;
        utils::doDummyWork(1000);
        CHECK_MESSAGE((int)utils::ConcurrencyTracker::PeakParallelism() <= required_max_concurrency,
        "Measured parallelism is not expected");
        g.try_put(argument);
    }
};

template<typename OutputType, typename InputType = int>
struct copy_counting_object {
    std::size_t copy_count;/*increases on every new copied object*/
    mutable std::size_t copies_count;/*count number of objects copied from this object*/
    std::size_t assign_count;
    bool is_copy;

    copy_counting_object():
        copy_count(0), copies_count(0), assign_count(0), is_copy(false) {}

    copy_counting_object(int):
        copy_count(0), copies_count(0), assign_count(0), is_copy(false) {}

    copy_counting_object( const copy_counting_object<OutputType, InputType>& other ):
        copy_count(other.copy_count + 1), is_copy(true) {
            ++other.copies_count;
        }

    copy_counting_object& operator=( const copy_counting_object<OutputType, InputType>& other ) {
        assign_count = other.assign_count + 1;
        is_copy = true;
        return *this;
    }

    OutputType operator()( InputType ) {
        return OutputType(1);
    }

    void operator()( InputType, multifunc_ports_t<InputType,OutputType>& op ) {
        std::get<0>(op).try_put(OutputType(1));
    }

    void operator()( InputType , async_ports_t<InputType, OutputType>& g) {
        g.try_put(OutputType(1));
    }

    OutputType operator()( oneapi::tbb::flow_control & fc ) {
        static bool check = false;
        if(check) {
            check = false;
            fc.stop();
            return OutputType(1);
        }
        check = true;
        return OutputType(1);
    }
};

template <typename OutputType = int>
struct passthru_body {
    OutputType operator()( const oneapi::tbb::flow::continue_msg& ) {
        return OutputType(0);
    }

    OutputType operator()( const OutputType& i ) {
        return i;
    }

    OutputType operator()( oneapi::tbb::flow_control & fc ) {
        static bool check = false;
        if(check) {
            check = false;
            fc.stop();
            return OutputType(0);
        }
        check = true;
        return OutputType(0);
    }

    void operator()( OutputType argument, multifunc_ports_t<OutputType>& op ) {
        std::get<0>(op).try_put(argument);
    }

    void operator()( OutputType argument, async_ports_t<OutputType>& g ) {
        g.try_put(argument);
    }
};

template<typename Node, typename InputType, typename OutputType, typename ...Args>
void test_body_exec(Args... node_args) {
    oneapi::tbb::flow::graph g;
    counting_functor<OutputType> counting_body;
    counting_body.execute_count = 0;

    Node testing_node(g, node_args..., counting_body);

    constexpr std::size_t n = 10;
    for(std::size_t i = 0; i < n; ++i) {
        CHECK_MESSAGE((produce_messages<Node, InputType>(testing_node) == true),
                "try_put of first node should return true");
    }
    g.wait_for_all();

    CHECK_MESSAGE((counting_body.execute_count == n), "Body of the first node needs to be executed N times");
}

template<typename Node, typename Body, typename ...Args>
void test_copy_body_function(Args... node_args) {
    using namespace oneapi::tbb::flow;

    Body base_body;

    graph g;

    Node testing_node(g, node_args..., base_body);

    Body b2 = copy_body<Body, Node>(testing_node);

    CHECK_MESSAGE((base_body.copy_count + 1 < b2.copy_count), "copy_body and constructor should copy bodies");
}

template<typename Node, typename InputType, typename ...Args>
void test_buffering(Args... node_args) {
    oneapi::tbb::flow::graph g;

    Node testing_node(g, node_args...);
    oneapi::tbb::flow::limiter_node<int> rejecter(g, 0);

    oneapi::tbb::flow::make_edge(testing_node, rejecter);

    int tmp = -1;
    produce_messages<Node, InputType>(testing_node);
    g.wait_for_all();


#if defined CONFORMANCE_BUFFERING_NODES || defined CONFORMANCE_INPUT_NODE
    CHECK_MESSAGE((testing_node.try_get(tmp) == true), "try_get after rejection should succeed");
    CHECK_MESSAGE((tmp == 1), "try_get after rejection should set value");
#else
#ifdef CONFORMANCE_MULTIFUNCTION_NODE
    CHECK_MESSAGE((std::get<0>(testing_node.output_ports()).try_get(tmp) == false), "try_get after rejection should not succeed");
#else
    CHECK_MESSAGE((testing_node.try_get(tmp) == false), "try_get after rejection should not succeed");
#endif
    CHECK_MESSAGE((tmp == -1), "try_get after rejection should not alter passed value");
#endif
}


template<typename Node, typename InputType, typename OutputType = InputType, typename ...Args>
void test_forwarding(std::size_t messages_received, Args... node_args) {
    oneapi::tbb::flow::graph g;

    Node testing_node(g, node_args...);
    std::vector<std::unique_ptr<test_push_receiver<OutputType>>> receiver_nodes;

    for(std::size_t i = 0; i < 10; ++i) {
        receiver_nodes.emplace_back(new test_push_receiver<OutputType>(g));
        oneapi::tbb::flow::make_edge(testing_node, *receiver_nodes.back());
    }

    produce_messages<Node, InputType>(testing_node, expected);

#ifdef CONFORMANCE_INPUT_NODE
    CHECK_MESSAGE(expected == messages_received, "For correct execution of test");
#endif

    g.wait_for_all();
    for(auto& receiver : receiver_nodes) {
        auto values = get_values(*receiver);
        CHECK_MESSAGE((values.size() == messages_received), std::string("Descendant of the node must receive " + std::to_string(messages_received) + " message."));
        CHECK_MESSAGE((values[0] == expected), "Value passed is the actual one received.");
    }
}

template<typename Node, typename ...Args>
void test_forwarding_single_push(Args... node_args) {
    oneapi::tbb::flow::graph g;

    Node testing_node(g, node_args...);
    test_push_receiver<int> suc_node1(g);
    test_push_receiver<int> suc_node2(g);

    oneapi::tbb::flow::make_edge(testing_node, suc_node1);
    oneapi::tbb::flow::make_edge(testing_node, suc_node2);

    testing_node.try_put(0);
    g.wait_for_all();

    auto values1 = get_values(suc_node1);
    auto values2 = get_values(suc_node2);
    CHECK_MESSAGE((values1.size() != values2.size()), "Only one descendant the node needs to receive");
    CHECK_MESSAGE((values1.size() + values2.size() == 1), "All messages need to be received");

    testing_node.try_put(1);
    g.wait_for_all();

    auto values3 = get_values(suc_node1);
    auto values4 = get_values(suc_node2);
    CHECK_MESSAGE((values3.size() != values4.size()), "Only one descendant the node needs to receive");
    CHECK_MESSAGE((values3.size() + values4.size() == 1), "All messages need to be received");

#ifdef CONFORMANCE_QUEUE_NODE
    CHECK_MESSAGE((values1[0] == 0), "Value passed is the actual one received");
    CHECK_MESSAGE((values3[0] == 1), "Value passed is the actual one received");
#else
    if(values1.size() == 1) {
        CHECK_MESSAGE((values1[0] == 0), "Value passed is the actual one received");
    }else{
        CHECK_MESSAGE((values2[0] == 0), "Value passed is the actual one received");
    }
#endif
}

template<typename Node, typename InputType, typename OutputType>
void test_inheritance() {
    using namespace oneapi::tbb::flow;

    CHECK_MESSAGE((std::is_base_of<graph_node, Node>::value), "Node should be derived from graph_node");
    CHECK_MESSAGE((std::is_base_of<receiver<InputType>, Node>::value), "Node should be derived from receiver<Input>");
    CHECK_MESSAGE((std::is_base_of<sender<OutputType>, Node>::value), "Node should be derived from sender<Output>");
}

template<typename Node>
void test_copy_ctor() {
    using namespace oneapi::tbb::flow;
    graph g;

    dummy_functor<int> fun1;
    conformance::copy_counting_object<int> fun2;

    Node node0(g, unlimited, fun1);
    Node node1(g, unlimited, fun2);
    test_push_receiver<int> suc_node1(g);
    test_push_receiver<int> suc_node2(g);

    oneapi::tbb::flow::make_edge(node0, node1);
    oneapi::tbb::flow::make_edge(node1, suc_node1);

    Node node_copy(node1);

    conformance::copy_counting_object<int> b2 = copy_body<conformance::copy_counting_object<int>, Node>(node_copy);

    CHECK_MESSAGE((fun2.copy_count + 1 < b2.copy_count), "constructor should copy bodies");

    oneapi::tbb::flow::make_edge(node_copy, suc_node2);

    node_copy.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((get_values(suc_node1).size() == 0 && get_values(suc_node2).size() == 1), "Copied node doesn`t copy successor");

    node0.try_put(1);
    g.wait_for_all();

    CHECK_MESSAGE((get_values(suc_node1).size() == 1 && get_values(suc_node2).size() == 0), "Copied node doesn`t copy predecessor");
}

template<typename Node, typename ...Args>
void test_copy_ctor_for_buffering_nodes(Args... node_args) {
    oneapi::tbb::flow::graph g;

    dummy_functor<int> fun;

    Node testing_node(g, node_args...);
    oneapi::tbb::flow::continue_node<int> pred_node(g, fun);
    test_push_receiver<int> suc_node1(g);
    test_push_receiver<int> suc_node2(g);

    oneapi::tbb::flow::make_edge(pred_node, testing_node);
    oneapi::tbb::flow::make_edge(testing_node, suc_node1);

#ifdef CONFORMANCE_OVERWRITE_NODE
    testing_node.try_put(1);
#endif

    Node node_copy(testing_node);

#ifdef CONFORMANCE_OVERWRITE_NODE
    int tmp;
    CHECK_MESSAGE((!node_copy.is_valid() && !node_copy.try_get(tmp)), "The buffered value is not copied from src");
    get_values(suc_node1);
#endif

    oneapi::tbb::flow::make_edge(node_copy, suc_node2);

    node_copy.try_put(0);
    g.wait_for_all();

    CHECK_MESSAGE((get_values(suc_node1).size() == 0 && get_values(suc_node2).size() == 1), "Copied node doesn`t copy successor");

#ifdef CONFORMANCE_OVERWRITE_NODE
    node_copy.clear();
    testing_node.clear();
#endif

    pred_node.try_put(oneapi::tbb::flow::continue_msg());
    g.wait_for_all();

    CHECK_MESSAGE((get_values(suc_node1).size() == 1 && get_values(suc_node2).size() == 0), "Copied node doesn`t copy predecessor");
}

template<typename Node, typename InputType, typename ...Args>
void test_priority(Args... node_args) {


    oneapi::tbb::flow::graph g;
    oneapi::tbb::flow::continue_node<InputType> source(g, dummy_functor<InputType>());

    track_first_id_functor<int>::first_id = -1;
    track_first_id_functor<int> low_functor(1);
    track_first_id_functor<int> high_functor(2);

    // Due to args... we cannot create the nodes inside the lambda with old compilers
    Node high(g, node_args..., high_functor, oneapi::tbb::flow::node_priority_t(1));
    Node low(g, node_args..., low_functor);

    tbb::task_arena a(1, 1);
    a.execute([&] {
        g.reset(); // attach to this arena

        make_edge(source, low);
        make_edge(source, high);
        source.try_put(oneapi::tbb::flow::continue_msg());

        g.wait_for_all();

        CHECK_MESSAGE((track_first_id_functor<int>::first_id == 2), "High priority node should execute first");
    });
}

template<typename Node>
void test_concurrency() {
    auto max_num_threads = oneapi::tbb::this_task_arena::max_concurrency();

    oneapi::tbb::global_control c(oneapi::tbb::global_control::max_allowed_parallelism,
                                  max_num_threads);

    std::vector<int> threads_count = {1, oneapi::tbb::flow::serial, max_num_threads, oneapi::tbb::flow::unlimited};

    if(max_num_threads > 2) {
        threads_count.push_back(max_num_threads / 2);
    }

    for(auto num_threads : threads_count) {
        utils::ConcurrencyTracker::Reset();
        int expected_threads = num_threads;
        if(num_threads == oneapi::tbb::flow::unlimited) {
            expected_threads = max_num_threads;
        }
        if(num_threads == oneapi::tbb::flow::serial) {
            expected_threads = 1;
        }
        oneapi::tbb::flow::graph g;
        concurrency_peak_checker_body counter(expected_threads);
        Node fnode(g, num_threads, counter);

        test_push_receiver<int> suc_node(g);

        make_edge(fnode, suc_node);

        for(int i = 0; i < 500; ++i) {
            fnode.try_put(i);
        }
        g.wait_for_all();
    }
}

template<typename Node>
void test_rejecting() {
    oneapi::tbb::flow::graph g;

    wait_flag_body body;
    Node fnode(g, oneapi::tbb::flow::serial, body);

    test_push_receiver<int> suc_node(g);

    make_edge(fnode, suc_node);

    fnode.try_put(0);

    CHECK_MESSAGE((!fnode.try_put(1)), "Messages should be rejected while the first is being processed");

    wait_flag_body::flag = true;

    g.wait_for_all();
    CHECK_MESSAGE((get_values(suc_node).size() == 1), "Messages should be rejected while the first is being processed");
}

template<typename Node, typename CountingBody>
void test_output_input_class() {
    using namespace oneapi::tbb::flow;

    passthru_body<CountingBody> fun;

    graph g;
    Node node1(g, unlimited, fun);
    test_push_receiver<CountingBody> suc_node(g);
    make_edge(node1, suc_node);
    CountingBody b1;
    CountingBody b2;
    node1.try_put(b1);
    g.wait_for_all();
    suc_node.try_get(b2);
    DOCTEST_WARN_MESSAGE((b1.copies_count > 0), "The type Input must meet the DefaultConstructible and CopyConstructible requirements");
    DOCTEST_WARN_MESSAGE((b2.is_copy), "The type Output must meet the CopyConstructible requirements");
}

template<typename Node, typename Output = copy_counting_object<int>>
void test_output_class() {
    using namespace oneapi::tbb::flow;

    passthru_body<Output> fun;

    graph g;
    Node node1(g, fun);
    test_push_receiver<Output> suc_node(g);
    make_edge(node1, suc_node);

#ifdef CONFORMANCE_INPUT_NODE
    node1.activate();
#else
    node1.try_put(oneapi::tbb::flow::continue_msg());
#endif

    g.wait_for_all();
    Output b;
    suc_node.try_get(b);
    DOCTEST_WARN_MESSAGE((b.is_copy), "The type Output must meet the CopyConstructible requirements");
}

template<typename Node>
void test_with_reserving_join_node_class() {
    using namespace oneapi::tbb::flow;

    graph g;

    function_node<int, int> static_result_computer_n(
        g, serial,
        [&](const int& msg) {
            // compute the result using incoming message and pass it further, e.g.:
            int result = int((msg >> 2) / 4);
            return result;
        });
    Node testing_node(g); // for buffering once computed value

    buffer_node<int> buffer_n(g);
    join_node<std::tuple<int, int>, reserving> join_n(g);

    std::atomic<int> number{2};
    std::atomic<int> counter{0};
    function_node<std::tuple<int, int>> consumer_n(
        g, unlimited,
        [&](const std::tuple<int, int>& arg) {
            // use the precomputed static result along with dynamic data
            ++counter;
#ifdef CONFORMANCE_OVERWRITE_NODE
            CHECK_MESSAGE((std::get<0>(arg) == int((number >> 2) / 4)), "A overwrite_node store a single item that can be overwritten");
#else
            CHECK_MESSAGE((std::get<0>(arg) == int((number >> 2) / 4)), "A write_once_node store a single item that cannot be overwritten");
#endif
        });

    make_edge(static_result_computer_n, testing_node);
    make_edge(testing_node, input_port<0>(join_n));
    make_edge(buffer_n, input_port<1>(join_n));
    make_edge(join_n, consumer_n);

    // do one-time calculation that will be reused many times further in the graph
    static_result_computer_n.try_put(number);

    constexpr int put_count = 50;
    for (int i = 0; i < put_count / 2; i++) {
        buffer_n.try_put(i);
    }
#ifdef CONFORMANCE_OVERWRITE_NODE
    number = 3;
#endif
    static_result_computer_n.try_put(number);
    for (int i = 0; i < put_count / 2; i++) {
        buffer_n.try_put(i);
    }

    g.wait_for_all();
    CHECK_MESSAGE((counter == put_count), "join_node with reserving policy \
        if at least one successor accepts the tuple must consume messages");
}
}
#endif // __TBB_test_conformance_conformance_flowgraph_H
