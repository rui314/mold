/*
    Copyright (c) 2019-2021 Intel Corporation

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

#ifndef __TBB_test_common_test_follows_and_precedes_api_H_
#define __TBB_test_common_test_follows_and_precedes_api_H_

#include "config.h"
#include "oneapi/tbb/flow_graph.h"

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#include <array>
#include <vector>
#include <type_traits>

namespace follows_and_precedes_testing{

template <typename NodeType>
struct testing_method_follows: std::integral_constant<int, 0> {};

template <typename... Args>
struct testing_method_follows<tbb::flow::join_node<Args...>> : std::integral_constant<int, 1> {};
template <typename... Args>
struct testing_method_follows<tbb::flow::continue_node<Args...>> : std::integral_constant<int, 1> {};

template <typename... Args>
struct testing_method_follows<tbb::flow::overwrite_node<Args...>> : std::integral_constant<int, 2> {};
template <typename... Args>
struct testing_method_follows<tbb::flow::write_once_node<Args...>> : std::integral_constant<int, 2> {};

template <typename... Args>
struct testing_method_follows<tbb::flow::multifunction_node<Args...>> : std::integral_constant<int, 3> {};

template <typename MessageType, typename NodeType, typename PredecessorType>
class edge_checker_follows {
public:
    static void check(tbb::flow::graph& g,
                      NodeType& node,
                      std::array<PredecessorType, 3>& preds,
                      std::array<MessageType, 3>& messages) {
        check_impl(g, node, preds, messages, typename testing_method_follows<NodeType>::type());
    }
private:
    // Test is applicable for: function_node, buffer_node, queue_node, priority_queue_node, limiter_node,
    //                         broadcast_node, sequencer_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<PredecessorType, 3>& preds,
                           std::array<MessageType, 3>& messages,
                           std::integral_constant<int, 0>) {
        tbb::flow::buffer_node<typename NodeType::output_type> buffer(g);
        tbb::flow::make_edge(node, buffer);

        for(size_t i = 0; i < 3; ++i) {
            preds[i].try_put(messages[i]);
            g.wait_for_all();

            typename NodeType::output_type storage;
            CHECK_MESSAGE((buffer.try_get(storage) && !buffer.try_get(storage)), "Not exact edge quantity was made");
        }
    }
    // Test is applicable for: continue_node, join_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<PredecessorType, 3>& preds,
                           std::array<MessageType, 3>& messages,
                           std::integral_constant<int, 1>) {
        tbb::flow::buffer_node<typename NodeType::output_type> buffer(g);
        tbb::flow::make_edge(node, buffer);

        for(size_t i = 0; i < 3; ++i) {
            preds[i].try_put(messages[i]);
            g.wait_for_all();
        }

        typename NodeType::output_type storage;
        CHECK_MESSAGE((buffer.try_get(storage) && !buffer.try_get(storage)), "Not exact edge quantity was made");
    }
    // Test is applicable for: overwrite_node, write_once_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<PredecessorType, 3>& preds,
                           std::array<MessageType, 3>& messages,
                           std::integral_constant<int, 2>) {
        for(size_t i = 0; i < 3; ++i) {
            node.clear();
            preds[i].try_put(messages[i]);
            g.wait_for_all();

            typename NodeType::output_type storage;
            CHECK_MESSAGE((node.try_get(storage)), "Not exact edge quantity was made");
        }
   }
    // Test is applicable for: multifunction_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<PredecessorType, 3>& preds,
                           std::array<MessageType, 3>& messages,
                           std::integral_constant<int, 3>) {
        typedef typename std::remove_reference<decltype(tbb::flow::output_port<0>(node))>::type multioutput;
        tbb::flow::buffer_node<typename multioutput::output_type> buf(tbb::flow::follows(tbb::flow::output_port<0>(node)));

        for(size_t i = 0; i < 3; ++i) {
            preds[i].try_put(messages[i]);
            g.wait_for_all();

            typename multioutput::output_type storage;
            CHECK_MESSAGE((buf.try_get(storage) && ! buf.try_get(storage)), "Not exact edge quantity was made");
        }
   }
};

template <typename NodeType>
struct testing_method_precedes: std::integral_constant<int, 0> {};

template <typename... Args>
struct testing_method_precedes<tbb::flow::buffer_node<Args...>> : std::integral_constant<int, 1> {};
template <typename... Args>
struct testing_method_precedes<tbb::flow::queue_node<Args...>> : std::integral_constant<int, 1> {};
template <typename... Args>
struct testing_method_precedes<tbb::flow::priority_queue_node<Args...>> : std::integral_constant<int, 1> {};
template <typename... Args>
struct testing_method_precedes<tbb::flow::sequencer_node<Args...>> : std::integral_constant<int, 1> {};

template <typename... Args>
struct testing_method_precedes<tbb::flow::join_node<Args...>> : std::integral_constant<int, 2> {};

template <typename MessageType, typename NodeType>
class edge_checker_precedes {
    using NodeOutputType = typename NodeType::output_type;
    using SuccessorOutputType = typename tbb::flow::buffer_node<NodeOutputType>::output_type;
public:
    static void check(tbb::flow::graph& g,
                      NodeType& node,
                      std::array<tbb::flow::buffer_node<NodeOutputType>, 3>& successors,
                      std::vector<MessageType>& messages) {
        check_impl(g, node, successors, messages, typename testing_method_precedes<NodeType>::type());
    }
private:
    // Testing is applicable for: continue_node, function_node, overwrite_node, buffer_node,
    //                            broadcast_node, write_once_node, limiter_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<tbb::flow::buffer_node<NodeOutputType>, 3>& successors,
                           std::vector<MessageType>& messages,
                           std::integral_constant<int, 0>) {
        CHECK_MESSAGE((messages.size() == 1),
               "messages.size() has to be 1 to test nodes what broadcast message to all the successors");

        node.try_put(messages[0]);
        g.wait_for_all();

        SuccessorOutputType storage;
        for(auto& successor: successors) {
            CHECK_MESSAGE((successor.try_get(storage) && !successor.try_get(storage)),
                    "Not exact edge quantity was made");
        }
    }
    // Testing is applicable for: buffer_node, queue_node, priority_queue_node, sequencer_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<tbb::flow::buffer_node<typename NodeType::output_type>, 3>& successors,
                           std::vector<MessageType>& messages,
                           std::integral_constant<int, 1>) {
        CHECK_MESSAGE((messages.size() == 3),
               "messages.size() has to be 3 to test nodes what send a message to the first available successor");

        tbb::flow::write_once_node<SuccessorOutputType>
            buffer(tbb::flow::follows(successors[0], successors[1], successors[2]));

        for(size_t i = 0; i < 3; ++i) {
            node.try_put(messages[i]);
            g.wait_for_all();

            SuccessorOutputType storage;
            CHECK_MESSAGE((buffer.try_get(storage)), "Not exact edge quantity was made");

            buffer.clear();
        }
    }
    // Testing is applicable for: join_node
    static void check_impl(tbb::flow::graph& g,
                           NodeType& node,
                           std::array<tbb::flow::buffer_node<typename NodeType::output_type>, 3>& successors,
                           std::vector<MessageType>& messages,
                           std::integral_constant<int, 2>) {
        CHECK_MESSAGE((messages.size() == 3),
               "messages.size() has to be 3 to test nodes what send a message to the first available successor");
        std::array<tbb::flow::buffer_node<MessageType>, 3> preds = {
	  {
	    tbb::flow::buffer_node<MessageType>(g), 
	    tbb::flow::buffer_node<MessageType>(g), 
	    tbb::flow::buffer_node<MessageType>(g)
	  }
	};

        make_edge(preds[0], tbb::flow::input_port<0>(node));
        make_edge(preds[1], tbb::flow::input_port<1>(node));
        make_edge(preds[2], tbb::flow::input_port<2>(node));

        for(size_t i = 0; i < 3; ++i) {
            preds[i].try_put(messages[i]);
            g.wait_for_all();
        }

        typename NodeType::output_type storage;
        for(auto& successor: successors) {
            CHECK_MESSAGE((successor.try_get(storage) && !successor.try_get(storage)),
            "Not exact edge quantity was made");
        }
    }
};

template<typename MessageType,
         typename NodeType,
         typename PredecessorType=tbb::flow::broadcast_node<MessageType>,
         typename... ConstructorArgs>
void test_follows(std::array<MessageType, 3>& messages, ConstructorArgs&&... args) {
    using namespace tbb::flow;

    graph g;
    std::array<PredecessorType, 3> preds = {
        {
            PredecessorType(g),
            PredecessorType(g),
            PredecessorType(g)
        }
    };

    NodeType node(follows(preds[0], preds[1], preds[2]), std::forward<ConstructorArgs>(args)...);

    edge_checker_follows<MessageType, NodeType, PredecessorType>::check(g, node, preds, messages);
}

template<typename MessageType,
         typename NodeType,
         typename... ConstructorArgs>
void test_precedes(std::vector<MessageType>& messages, ConstructorArgs&&... args) {
    using namespace tbb::flow;

    using SuccessorType = buffer_node<typename NodeType::output_type>;

    graph g;

    std::array<SuccessorType, 3> successors = { {SuccessorType(g), SuccessorType(g), SuccessorType(g)} };

    NodeType node(precedes(successors[0], successors[1], successors[2]), std::forward<ConstructorArgs>(args)...);

    edge_checker_precedes<MessageType, NodeType>::check(g, node, successors, messages);
}

} // follows_and_precedes_testing
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
#endif // __TBB_test_common_test_follows_and_precedes_api_H_
