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

#include "common/config.h"

#if _MSC_VER
    #pragma warning (disable: 4503) // Suppress "decorated name length exceeded, name was truncated" warning
    #if _MSC_VER==1700 && !defined(__INTEL_COMPILER)
        // Suppress "unreachable code" warning by VC++ 17.0 (VS 2012)
        #pragma warning (disable: 4702)
    #endif
#endif

// need these to get proper external names for private methods in library.
#include "tbb/spin_mutex.h"
#include "tbb/spin_rw_mutex.h"
#include "tbb/task_arena.h"
#include "tbb/task_group.h"

#define private public
#define protected public
#include "tbb/flow_graph.h"
#undef protected
#undef private

#include "common/test.h"
#include "common/utils.h"
#include "common/spin_barrier.h"
#include "common/graph_utils.h"

#include <string> // merely prevents LNK2001 error to happen (on ICL+VC9 configurations)

//! \file test_flow_graph_whitebox.cpp
//! \brief Test for [flow_graph.broadcast_node flow_graph.priority_queue_node flow_graph.indexer_node flow_graph.sequencer_node flow_graph.remove_edge flow_graph.join_node flow_graph.split_node flow_graph.limiter_node flow_graph.write_once_node flow_graph.overwrite_node flow_graph.make_edge flow_graph.graph flow_graph.buffer_node flow_graph.function_node flow_graph.multifunction_node flow_graph.continue_node flow_graph.input_node] specification

template<typename T>
struct receiverBody {
    tbb::flow::continue_msg operator()(const T &/*in*/) {
        return tbb::flow::continue_msg();
    }
};

// split_nodes cannot have predecessors
// they do not reject messages and always forward.
// they reject edge reversals from successors.
void TestSplitNode() {
    typedef tbb::flow::split_node<std::tuple<int> > snode_type;
    tbb::flow::graph g;
    snode_type snode(g);
    tbb::flow::function_node<int> rcvr(g,tbb::flow::unlimited, receiverBody<int>());
    INFO("Testing split_node\n");
    CHECK_MESSAGE( (tbb::flow::output_port<0>(snode).my_successors.empty()), "Constructed split_node has successors");
    // tbb::flow::output_port<0>(snode)
    tbb::flow::make_edge(tbb::flow::output_port<0>(snode), rcvr);
    CHECK_MESSAGE( (!(tbb::flow::output_port<0>(snode).my_successors.empty())), "after make_edge, split_node has no successor.");
    snode.try_put(std::tuple<int>(1));
    g.wait_for_all();
    g.reset();
    CHECK_MESSAGE( (!(tbb::flow::output_port<0>(snode).my_successors.empty())), "after reset(), split_node has no successor.");
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (tbb::flow::output_port<0>(snode).my_successors.empty()), "after reset(rf_clear_edges), split_node has a successor.");
}

// buffering nodes cannot have predecessors
// they do not reject messages and always save or forward
// they allow edge reversals from successors
template< typename B >
void TestBufferingNode(const char * name) {
    tbb::flow::graph g;
    B bnode(g);
    tbb::flow::function_node<int,int,tbb::flow::rejecting> fnode(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));
    INFO("Testing " << name << ":");
    for(int icnt = 0; icnt < 2; icnt++) {
        bool reverse_edge = (icnt & 0x2) != 0;
        serial_fn_state0 = 0;  // reset to waiting state.
        INFO(" make_edge");
        tbb::flow::make_edge(bnode, fnode);
        CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after make_edge");
        std::thread t([&] {
            INFO(" try_put");
            bnode.try_put(1);  // will forward to the fnode
            g.wait_for_all();
        });
        utils::SpinWaitWhileEq(serial_fn_state0, 0);
        if(reverse_edge) {
            INFO(" try_put2");
            bnode.try_put(2);  // should reverse the edge
            // waiting for the edge to reverse
            utils::SpinWaitWhile([&] { return !bnode.my_successors.empty(); });
        }
        else {
            CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after forwarding message");
        }
        serial_fn_state0 = 0;  // release the function_node.
        if(reverse_edge) {
            // have to do a second release because the function_node will get the 2nd item
            utils::SpinWaitWhileEq(serial_fn_state0, 0);
            serial_fn_state0 = 0;  // release the function_node.
        }
        t.join();
        INFO(" remove_edge");
        tbb::flow::remove_edge(bnode, fnode);
        CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after remove_edge");
    }
    tbb::flow::join_node<std::tuple<int,int>,tbb::flow::reserving> jnode(g);
    tbb::flow::make_edge(bnode, tbb::flow::input_port<0>(jnode));  // will spawn a task
    g.wait_for_all();
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after attaching to join");
    INFO(" reverse");
    bnode.try_put(1);  // the edge should reverse
    g.wait_for_all();
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after reserving");
    INFO(" reset()");
    g.wait_for_all();
    g.reset();  // should be in forward direction again
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after reset()");
    INFO(" remove_edge");
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after reset(rf_clear_edges)");
    tbb::flow::make_edge(bnode, tbb::flow::input_port<0>(jnode));  // add edge again
    // reverse edge by adding to buffer.
    bnode.try_put(1);  // the edge should reverse
    g.wait_for_all();
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after reserving");
    INFO(" remove_edge(reversed)");
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has no successor after reset()");
    CHECK_MESSAGE( (tbb::flow::input_port<0>(jnode).my_predecessors.empty()), "predecessor not reset");
    INFO("  done\n");
    g.wait_for_all();
}

// continue_node has only predecessor count
// they do not have predecessors, only the counts
// successor edges cannot be reversed
void TestContinueNode() {
    tbb::flow::graph g;
    tbb::flow::function_node<int> fnode0(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));
    tbb::flow::continue_node<int> cnode(g, /*number_of_predecessors*/ 1,
                                        serial_continue_body<int>(serial_continue_state0));
    tbb::flow::function_node<int> fnode1(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state1));
    tbb::flow::make_edge(fnode0, cnode);
    tbb::flow::make_edge(cnode, fnode1);
    INFO("Testing continue_node:");
    for( int icnt = 0; icnt < 2; ++icnt ) {
        INFO( " initial" << icnt);
        CHECK_MESSAGE( (cnode.my_predecessor_count == 2), "predecessor addition didn't increment count");
        CHECK_MESSAGE( (!cnode.successors().empty()), "successors empty though we added one");
        CHECK_MESSAGE( (cnode.my_current_count == 0), "state of continue_receiver incorrect");
        serial_continue_state0 = 0;
        serial_fn_state0 = 0;
        serial_fn_state1 = 0;

        std::thread t([&] {
            fnode0.try_put(1);  // start the first function node.
            if(icnt == 0) {  // first time through, let the continue_node fire
                INFO(" firing");
                fnode0.try_put(1);  // second message
                g.wait_for_all();

                // try a try_get()
                {
                    int i;
                    CHECK_MESSAGE( (!cnode.try_get(i)), "try_get not rejected");
                }

                INFO(" reset");
                CHECK_MESSAGE( (!cnode.my_successors.empty()), "Empty successors in built graph (before reset)");
                CHECK_MESSAGE( (cnode.my_predecessor_count == 2), "predecessor_count reset (before reset)");
                g.reset();  // should still be the same
                CHECK_MESSAGE( (!cnode.my_successors.empty()), "Empty successors in built graph (after reset)" );
                CHECK_MESSAGE( (cnode.my_predecessor_count == 2), "predecessor_count reset (after reset)");
            }
            else {  // we're going to see if the rf_clear_edges resets things.
                g.wait_for_all();
                INFO(" reset(rf_clear_edges)");
                CHECK_MESSAGE( (!cnode.my_successors.empty()), "Empty successors in built graph (before reset)" );
                CHECK_MESSAGE( (cnode.my_predecessor_count == 2), "predecessor_count reset (before reset)" );
                g.reset(tbb::flow::rf_clear_edges);  // should be in forward direction again
                CHECK_MESSAGE( (cnode.my_current_count == 0), "state of continue_receiver incorrect after reset(rf_clear_edges)" );
                CHECK_MESSAGE( (cnode.my_successors.empty()), "buffering node has a successor after reset(rf_clear_edges)" );
                CHECK_MESSAGE( (cnode.my_predecessor_count == cnode.my_initial_predecessor_count), "predecessor count not reset" );
            }
        });

        utils::SpinWaitWhileEq(serial_fn_state0, 0); // waiting for the first message to arrive in function_node
        // Now the body of function_node 0 is executing.
        serial_fn_state0 = 0;  // release the node
        if (icnt == 0) {
            // wait for node to count the message (or for the node body to execute, which would be wrong)
            utils::SpinWaitWhile([&] {
                tbb::spin_mutex::scoped_lock l(cnode.my_mutex);
                return serial_continue_state0 == 0 && cnode.my_current_count == 0;
            });
            CHECK_MESSAGE( (serial_continue_state0 == 0), "Improperly released continue_node");
            CHECK_MESSAGE( (cnode.my_current_count == 1), "state of continue_receiver incorrect");

            utils::SpinWaitWhileEq(serial_fn_state0, 0); // waiting for the second message to arrive in function_node
            // Now the body of function_node 0 is executing.
            serial_fn_state0 = 0;  // release the node

            utils::SpinWaitWhileEq(serial_continue_state0, 0); // waiting for continue_node to start
            CHECK_MESSAGE( (cnode.my_current_count == 0), " my_current_count not reset before body of continue_node started");
            serial_continue_state0 = 0;  // release the continue_node

            utils::SpinWaitWhileEq(serial_fn_state1, 0); // wait for the successor function_node to enter body
            serial_fn_state1 = 0;  // release successor function_node.
        }

        t.join();
    }

    INFO(" done\n");

}

// function_node has predecessors and successors
// try_get() rejects
// successor edges cannot be reversed
// predecessors will reverse (only rejecting will reverse)
void TestFunctionNode() {
    tbb::flow::graph g;
    tbb::flow::queue_node<int> qnode0(g);
    tbb::flow::function_node<int,int,   tbb::flow::rejecting > fnode0(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));
    tbb::flow::function_node<int,int/*, tbb::flow::queueing*/> fnode1(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));

    tbb::flow::queue_node<int> qnode1(g);

    tbb::flow::make_edge(fnode0, qnode1);
    tbb::flow::make_edge(qnode0, fnode0);

    serial_fn_state0 = 2;  // just let it go
    qnode0.try_put(1);
    g.wait_for_all();
    int ii;
    CHECK_MESSAGE( (qnode1.try_get(ii) && ii == 1), "output not passed" );
    tbb::flow::remove_edge(qnode0, fnode0);
    tbb::flow::remove_edge(fnode0, qnode1);

    tbb::flow::make_edge(fnode1, qnode1);
    tbb::flow::make_edge(qnode0, fnode1);

    serial_fn_state0 = 2;  // just let it go
    qnode0.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE( (qnode1.try_get(ii) && ii == 1), "output not passed" );
    tbb::flow::remove_edge(qnode0, fnode1);
    tbb::flow::remove_edge(fnode1, qnode1);

    // rejecting
    serial_fn_state0 = 0;
    std::atomic<bool> rejected{ false };
    std::thread t([&] {
        g.reset(); // attach to the current arena
        tbb::flow::make_edge(fnode0, qnode1);
        tbb::flow::make_edge(qnode0, fnode0); // TODO: invesigate why it always creates a forwarding task
        INFO("Testing rejecting function_node:");
        CHECK_MESSAGE( (!fnode0.my_queue), "node should have no queue");
        CHECK_MESSAGE( (!fnode0.my_successors.empty()), "successor edge not added");
        qnode0.try_put(1);
        qnode0.try_put(2);   // rejecting node should reject, reverse.
        rejected = true;
        g.wait_for_all();
    });
    utils::SpinWaitWhileEq(serial_fn_state0, 0); // waiting rejecting node to start
    utils::SpinWaitWhileEq(rejected, false);
    // TODO: the assest below is not stable due to the logical race between try_put(1)
    // try_put(2) and wait_for_all.
    // Additionally, empty() cannot be called concurrently due to null_mutex used in implementation
    // CHECK(fnode0.my_predecessors.empty() == false);
    serial_fn_state0 = 2;   // release function_node body.
    t.join();
    INFO(" reset");
    g.reset();  // should reverse the edge from the input to the function node.
    CHECK_MESSAGE( (!qnode0.my_successors.empty()), "empty successors after reset()");
    CHECK_MESSAGE( (fnode0.my_predecessors.empty()), "predecessor not reversed");
    tbb::flow::remove_edge(qnode0, fnode0);
    tbb::flow::remove_edge(fnode0, qnode1);
    INFO("\n");

    // queueing
    tbb::flow::make_edge(fnode1, qnode1);
    INFO("Testing queueing function_node:");
    CHECK_MESSAGE( (fnode1.my_queue), "node should have no queue");
    CHECK_MESSAGE( (!fnode1.my_successors.empty()), "successor edge not added");
    INFO(" add_pred");
    CHECK_MESSAGE( (fnode1.register_predecessor(qnode0)), "Cannot register as predecessor");
    CHECK_MESSAGE( (!fnode1.my_predecessors.empty()), "Missing predecessor");
    INFO(" reset");
    g.wait_for_all();
    g.reset();  // should reverse the edge from the input to the function node.
    CHECK_MESSAGE( (!qnode0.my_successors.empty()), "empty successors after reset()");
    CHECK_MESSAGE( (fnode1.my_predecessors.empty()), "predecessor not reversed");
    tbb::flow::remove_edge(qnode0, fnode1);
    tbb::flow::remove_edge(fnode1, qnode1);
    INFO("\n");

    serial_fn_state0 = 0;  // make the function_node wait
    rejected = false;
    std::thread t2([&] {
        g.reset(); // attach to the current arena

        tbb::flow::make_edge(qnode0, fnode0); // TODO: invesigate why it always creates a forwarding task

        INFO(" start_func");
        qnode0.try_put(1);
        // now if we put an item to the queues the edges to the function_node will reverse.
        INFO(" put_node(2)");
        qnode0.try_put(2);   // start queue node.
        rejected = true;
        g.wait_for_all();
    });
    utils::SpinWaitWhileEq(serial_fn_state0, 0); // waiting rejecting node to start
    // wait for the edges to reverse
    utils::SpinWaitWhileEq(rejected, false);
    // TODO: the assest below is not stable due to the logical race between try_put(1)
    // try_put(2) and wait_for_all.
    // Additionally, empty() cannot be called concurrently due to null_mutex used in implementation
    // CHECK(fnode0.my_predecessors.empty() == false);
    g.my_context->cancel_group_execution();
    // release the function_node
    serial_fn_state0 = 2;
    t2.join();
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (fnode0.my_predecessors.empty() && qnode0.my_successors.empty()), "function_node edge not removed");
    CHECK_MESSAGE( (fnode0.my_successors.empty()), "successor to fnode not removed");
    INFO(" done\n");
}

template<typename TT>
class tag_func {
    TT my_mult;
public:
    tag_func(TT multiplier) : my_mult(multiplier) { }
    // operator() will return [0 .. Count)
    tbb::flow::tag_value operator()( TT v) {
        tbb::flow::tag_value t = tbb::flow::tag_value(v / my_mult);
        return t;
    }
};

template<typename JNODE_TYPE>
void
TestSimpleSuccessorArc(const char *name) {
    tbb::flow::graph g;
    {
        INFO("Join<" << name << "> successor test ");
        tbb::flow::join_node<std::tuple<int>, JNODE_TYPE> qj(g);
        tbb::flow::broadcast_node<std::tuple<int> > bnode(g);
        tbb::flow::make_edge(qj, bnode);
        CHECK_MESSAGE( (!qj.my_successors.empty()),"successor missing after linking");
        g.reset();
        CHECK_MESSAGE( (!qj.my_successors.empty()),"successor missing after reset()");
        g.reset(tbb::flow::rf_clear_edges);
        CHECK_MESSAGE( (qj.my_successors.empty()), "successors not removed after reset(rf_clear_edges)");
    }
}

template<>
void
TestSimpleSuccessorArc<tbb::flow::tag_matching>(const char *name) {
    tbb::flow::graph g;
    {
        INFO("Join<" << name << "> successor test ");
        typedef std::tuple<int,int> my_tuple;
        tbb::flow::join_node<my_tuple, tbb::flow::tag_matching> qj(g,
                                                                   tag_func<int>(1),
                                                                   tag_func<int>(1)
        );
        tbb::flow::broadcast_node<my_tuple > bnode(g);
        tbb::flow::make_edge(qj, bnode);
        CHECK_MESSAGE( (!qj.my_successors.empty()),"successor missing after linking");
        g.reset();
        CHECK_MESSAGE( (!qj.my_successors.empty()),"successor missing after reset()");
        g.reset(tbb::flow::rf_clear_edges);
        CHECK_MESSAGE( (qj.my_successors.empty()), "successors not removed after reset(rf_clear_edges)");
    }
}

void
TestJoinNode() {
    tbb::flow::graph g;

    TestSimpleSuccessorArc<tbb::flow::queueing>("queueing");
    TestSimpleSuccessorArc<tbb::flow::reserving>("reserving");
    TestSimpleSuccessorArc<tbb::flow::tag_matching>("tag_matching");

    // queueing and tagging join nodes have input queues, so the input ports do not reverse.
    INFO(" reserving preds");
    {
        tbb::flow::join_node<std::tuple<int,int>, tbb::flow::reserving> rj(g);
        tbb::flow::queue_node<int> q0(g);
        tbb::flow::queue_node<int> q1(g);
        tbb::flow::make_edge(q0,tbb::flow::input_port<0>(rj));
        tbb::flow::make_edge(q1,tbb::flow::input_port<1>(rj));
        q0.try_put(1);
        g.wait_for_all();  // quiesce
        CHECK_MESSAGE( (!(tbb::flow::input_port<0>(rj).my_predecessors.empty())),"reversed port missing predecessor");
        CHECK_MESSAGE( ((tbb::flow::input_port<1>(rj).my_predecessors.empty())),"non-reversed port has pred");
        g.reset();
        CHECK_MESSAGE( ((tbb::flow::input_port<0>(rj).my_predecessors.empty())),"reversed port has pred after reset()");
        CHECK_MESSAGE( ((tbb::flow::input_port<1>(rj).my_predecessors.empty())),"non-reversed port has pred after reset()");
        q1.try_put(2);
        g.wait_for_all();  // quiesce
        CHECK_MESSAGE( (!(tbb::flow::input_port<1>(rj).my_predecessors.empty())),"reversed port missing predecessor");
        CHECK_MESSAGE( ((tbb::flow::input_port<0>(rj).my_predecessors.empty())),"non-reversed port has pred");
        g.reset();
        CHECK_MESSAGE( ((tbb::flow::input_port<1>(rj).my_predecessors.empty())),"reversed port has pred after reset()");
        CHECK_MESSAGE( ((tbb::flow::input_port<0>(rj).my_predecessors.empty())),"non-reversed port has pred after reset()");
        // should reset predecessors just as regular reset.
        q1.try_put(3);
        g.wait_for_all();  // quiesce
        CHECK_MESSAGE( (!(tbb::flow::input_port<1>(rj).my_predecessors.empty())),"reversed port missing predecessor");
        CHECK_MESSAGE( ((tbb::flow::input_port<0>(rj).my_predecessors.empty())),"non-reversed port has pred");
        g.reset(tbb::flow::rf_clear_edges);
        CHECK_MESSAGE( ((tbb::flow::input_port<1>(rj).my_predecessors.empty())),"reversed port has pred after reset()");
        CHECK_MESSAGE( ((tbb::flow::input_port<0>(rj).my_predecessors.empty())),"non-reversed port has pred after reset()");
        CHECK_MESSAGE( (q0.my_successors.empty()), "edge not removed by reset(rf_clear_edges)");
        CHECK_MESSAGE( (q1.my_successors.empty()), "edge not removed by reset(rf_clear_edges)");
    }
    INFO(" done\n");
}

template <typename DecrementerType>
struct limiter_node_type {
    using type = tbb::flow::limiter_node<int, DecrementerType>;
    using dtype = DecrementerType;
};

template <>
struct limiter_node_type<void> {
    using type = tbb::flow::limiter_node<int>;
    using dtype = tbb::flow::continue_msg;
};

template <typename DType>
struct DecrementerHelper {
    template <typename Decrementer>
    static void check(Decrementer&) {}
    static DType makeDType() {
        return DType(1);
    }
};

template <>
struct DecrementerHelper<tbb::flow::continue_msg> {
    template <typename Decrementer>
    static void check(Decrementer& decrementer) {
        auto& d = static_cast<tbb::detail::d1::continue_receiver&>(decrementer);
        CHECK_MESSAGE(d.my_predecessor_count == 0, "error in pred count");
        CHECK_MESSAGE(d.my_initial_predecessor_count == 0, "error in initial pred count");
        CHECK_MESSAGE(d.my_current_count == 0, "error in current count");
    }
    static tbb::flow::continue_msg makeDType() {
        return tbb::flow::continue_msg();
    }
};

template <typename DecrementerType>
void TestLimiterNode() {
    int out_int{};
    tbb::flow::graph g;
    using dtype = typename limiter_node_type<DecrementerType>::dtype;
    typename limiter_node_type<DecrementerType>::type ln(g,1);
    INFO("Testing limiter_node: preds and succs");
    DecrementerHelper<dtype>::check(ln.decrementer());
    CHECK_MESSAGE( (ln.my_threshold == 1), "error in my_threshold");
    tbb::flow::queue_node<int> inq(g);
    tbb::flow::queue_node<int> outq(g);
    tbb::flow::broadcast_node<dtype> bn(g);

    tbb::flow::make_edge(inq,ln);
    tbb::flow::make_edge(ln,outq);
    tbb::flow::make_edge(bn,ln.decrementer());

    g.wait_for_all();
    CHECK_MESSAGE( (!(ln.my_successors.empty())),"successors empty after make_edge");
    CHECK_MESSAGE( (ln.my_predecessors.empty()), "input edge reversed");
    inq.try_put(1);
    g.wait_for_all();
    CHECK_MESSAGE( (outq.try_get(out_int) && out_int == 1), "limiter_node didn't pass first value");
    CHECK_MESSAGE( (ln.my_predecessors.empty()), "input edge reversed");
    inq.try_put(2);
    g.wait_for_all();
    CHECK_MESSAGE( (!outq.try_get(out_int)), "limiter_node incorrectly passed second input");
    CHECK_MESSAGE( (!ln.my_predecessors.empty()), "input edge to limiter_node not reversed");
    bn.try_put(DecrementerHelper<dtype>::makeDType());
    g.wait_for_all();
    CHECK_MESSAGE( (outq.try_get(out_int) && out_int == 2), "limiter_node didn't pass second value");
    g.wait_for_all();
    CHECK_MESSAGE( (!ln.my_predecessors.empty()), "input edge was reversed(after try_get())");
    g.reset();
    CHECK_MESSAGE( (ln.my_predecessors.empty()), "input edge not reset");
    inq.try_put(3);
    g.wait_for_all();
    CHECK_MESSAGE( (outq.try_get(out_int) && out_int == 3), "limiter_node didn't pass third value");

    INFO(" rf_clear_edges");
    // currently the limiter_node will not pass another message
    g.reset(tbb::flow::rf_clear_edges);
    DecrementerHelper<dtype>::check(ln.decrementer());
    CHECK_MESSAGE( (ln.my_threshold == 1), "error in my_threshold");
    CHECK_MESSAGE( (ln.my_predecessors.empty()), "preds not reset(rf_clear_edges)");
    CHECK_MESSAGE( (ln.my_successors.empty()), "preds not reset(rf_clear_edges)");
    CHECK_MESSAGE( (inq.my_successors.empty()), "Arc not removed on reset(rf_clear_edges)");
    CHECK_MESSAGE( (inq.my_successors.empty()), "Arc not removed on reset(rf_clear_edges)");
    CHECK_MESSAGE( (bn.my_successors.empty()), "control edge not removed on reset(rf_clear_edges)");
    tbb::flow::make_edge(inq,ln);
    tbb::flow::make_edge(ln,outq);
    inq.try_put(4);
    inq.try_put(5);
    g.wait_for_all();
    CHECK_MESSAGE( (outq.try_get(out_int)),"missing output after reset(rf_clear_edges)");
    CHECK_MESSAGE( (out_int == 4), "input incorrect (4)");
    bn.try_put(DecrementerHelper<dtype>::makeDType());
    g.wait_for_all();
    CHECK_MESSAGE( (!outq.try_get(out_int)),"second output incorrectly passed (rf_clear_edges)");
    INFO(" done\n");
}

template<typename MF_TYPE>
struct mf_body {
    std::atomic<int>& my_flag;
    mf_body(std::atomic<int>& flag) : my_flag(flag) { }
    void operator()(const int& in, typename MF_TYPE::output_ports_type& outports) {
        if(my_flag == 0) {
            my_flag = 1;

            utils::SpinWaitWhileEq(my_flag, 1);
        }

        if (in & 0x1)
            std::get<1>(outports).try_put(in);
        else
            std::get<0>(outports).try_put(in);
    }
};

template<typename P, typename T>
struct test_reversal;
template<typename T>
struct test_reversal<tbb::flow::queueing, T> {
    test_reversal() { INFO("<queueing>"); }
    // queueing node will not reverse.
    bool operator()(T& node) const { return node.my_predecessors.empty(); }
};

template<typename T>
struct test_reversal<tbb::flow::rejecting, T> {
    test_reversal() { INFO("<rejecting>"); }
    bool operator()(T& node) const { return !node.my_predecessors.empty(); }
};

template<typename P>
void TestMultifunctionNode() {
    typedef tbb::flow::multifunction_node<int, std::tuple<int, int>, P> multinode_type;
    INFO("Testing multifunction_node");
    test_reversal<P,multinode_type> my_test;
    INFO(":");
    tbb::flow::graph g;
    multinode_type mf(g, tbb::flow::serial, mf_body<multinode_type>(serial_fn_state0));
    tbb::flow::queue_node<int> qin(g);
    tbb::flow::queue_node<int> qodd_out(g);
    tbb::flow::queue_node<int> qeven_out(g);
    tbb::flow::make_edge(qin,mf);
    tbb::flow::make_edge(tbb::flow::output_port<0>(mf), qeven_out);
    tbb::flow::make_edge(tbb::flow::output_port<1>(mf), qodd_out);
    g.wait_for_all();
    for (int ii = 0; ii < 2 ; ++ii) {
        std::atomic<bool> submitted{ false };
        serial_fn_state0 = 0;
        /* if(ii == 0) REMARK(" reset preds"); else REMARK(" 2nd");*/
        std::thread t([&] {
            g.reset(); // attach to the current arena
            qin.try_put(0);
            qin.try_put(1);
            submitted = true;
            g.wait_for_all();
        });
        // wait for node to be active
        utils::SpinWaitWhileEq(serial_fn_state0, 0);
        utils::SpinWaitWhileEq(submitted, false);
        g.my_context->cancel_group_execution();
        // release node
        serial_fn_state0 = 2;
        t.join();
        // The rejection test cannot guarantee the state of predecessors cache.
        if (!std::is_same<P, tbb::flow::rejecting>::value) {
            CHECK_MESSAGE((my_test(mf)), "fail cancel group test");
        }
        if( ii == 1) {
            INFO(" rf_clear_edges");
            g.reset(tbb::flow::rf_clear_edges);
            CHECK_MESSAGE( (tbb::flow::output_port<0>(mf).my_successors.empty()), "output_port<0> not reset (rf_clear_edges)");
            CHECK_MESSAGE( (tbb::flow::output_port<1>(mf).my_successors.empty()), "output_port<1> not reset (rf_clear_edges)");
        }
        else
        {
            g.reset();
        }
        CHECK_MESSAGE( (mf.my_predecessors.empty()), "edge didn't reset");
        CHECK_MESSAGE( ((ii == 0 && !qin.my_successors.empty()) || (ii == 1 && qin.my_successors.empty())), "edge didn't reset");
    }
    INFO(" done\n");
}

// indexer_node is like a broadcast_node, in that none of its inputs reverse, and it
// never allows a successor to reverse its edge, so we only need test the successors.
void
TestIndexerNode() {
    tbb::flow::graph g;
    typedef tbb::flow::indexer_node< int, int > indexernode_type;
    indexernode_type inode(g);
    INFO("Testing indexer_node:");
    tbb::flow::queue_node<indexernode_type::output_type> qout(g);
    tbb::flow::make_edge(inode,qout);
    g.wait_for_all();
    CHECK_MESSAGE( (!inode.my_successors.empty()), "successor of indexer_node missing");
    g.reset();
    CHECK_MESSAGE( (!inode.my_successors.empty()), "successor of indexer_node missing after reset");
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (inode.my_successors.empty()), "successor of indexer_node not removed by reset(rf_clear_edges)");
    INFO(" done\n");
}

template<typename Node>
void
TestScalarNode(const char *name) {
    tbb::flow::graph g;
    Node on(g);
    tbb::flow::queue_node<int> qout(g);
    INFO("Testing " << name << ":");
    tbb::flow::make_edge(on,qout);
    g.wait_for_all();
    CHECK_MESSAGE( (!on.my_successors.empty()), "edge not added");
    g.reset();
    CHECK_MESSAGE( (!on.my_successors.empty()), "edge improperly removed");
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (on.my_successors.empty()), "edge not removed by reset(rf_clear_edges)");
    INFO(" done\n");
}

struct seq_body {
    size_t operator()(const int &in) {
        return size_t(in / 3);
    }
};

// sequencer_node behaves like a queueing node, but requires a different constructor.
void TestSequencerNode() {
    tbb::flow::graph g;
    tbb::flow::sequencer_node<int> bnode(g, seq_body());
    INFO("Testing sequencer_node:");
    tbb::flow::function_node<int> fnode(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));
    INFO("Testing sequencer_node:");
    serial_fn_state0 = 0;  // reset to waiting state.
    INFO(" make_edge");
    tbb::flow::make_edge(bnode, fnode);
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after make_edge" );
    INFO(" try_put");
    std::thread t([&]{
        bnode.try_put(0);  // will forward to the fnode
        g.wait_for_all();
    });
    // wait for the function_node to fire up
    utils::SpinWaitWhileEq(serial_fn_state0, 0);
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after forwarding message" );
    serial_fn_state0 = 0;       // release the function node
    t.join();

    INFO(" remove_edge");
    tbb::flow::remove_edge(bnode, fnode);
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after remove_edge");
    tbb::flow::join_node<std::tuple<int,int>,tbb::flow::reserving> jnode(g);
    tbb::flow::make_edge(bnode, tbb::flow::input_port<0>(jnode));  // will spawn a task
    g.wait_for_all();
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after attaching to join");
    INFO(" reverse");
    bnode.try_put(3);  // the edge should reverse
    g.wait_for_all();
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after reserving");
    INFO(" reset()");
    g.wait_for_all();
    g.reset();  // should be in forward direction again
    CHECK_MESSAGE( (!bnode.my_successors.empty()), "buffering node has no successor after reset()");
    INFO(" remove_edge");
    g.reset(tbb::flow::rf_clear_edges);  // should be in forward direction again
    CHECK_MESSAGE( (bnode.my_successors.empty()), "buffering node has a successor after reset(rf_clear_edges)");
    CHECK_MESSAGE( (fnode.my_predecessors.empty()), "buffering node reversed after reset(rf_clear_edges)");
    INFO("  done\n");
    g.wait_for_all();
}

struct snode_body {
    int max_cnt;
    int my_cnt;
    snode_body(const int& in) : max_cnt(in) { my_cnt = 0; }
    int operator()(tbb::flow_control& fc) {
        if (max_cnt <= my_cnt++) {
            fc.stop();
            return int();
        }
        return my_cnt;
    }
};

void TestInputNode() {
    tbb::flow::graph g;
    tbb::flow::input_node<int> in(g, snode_body(4));
    INFO("Testing input_node:");
    tbb::flow::queue_node<int> qin(g);
    tbb::flow::join_node<std::tuple<int,int>, tbb::flow::reserving> jn(g);
    tbb::flow::queue_node<std::tuple<int,int> > qout(g);

    INFO(" make_edges");
    tbb::flow::make_edge(in, tbb::flow::input_port<0>(jn));
    tbb::flow::make_edge(qin, tbb::flow::input_port<1>(jn));
    tbb::flow::make_edge(jn,qout);
    CHECK_MESSAGE( (!in.my_successors.empty()), "input node has no successor after make_edge");
    g.wait_for_all();
    g.reset();
    CHECK_MESSAGE( (!in.my_successors.empty()), "input node has no successor after reset");
    g.wait_for_all();
    g.reset(tbb::flow::rf_clear_edges);
    CHECK_MESSAGE( (in.my_successors.empty()), "input node has successor after reset(rf_clear_edges)");
    tbb::flow::make_edge(in, tbb::flow::input_port<0>(jn));
    tbb::flow::make_edge(qin, tbb::flow::input_port<1>(jn));
    tbb::flow::make_edge(jn,qout);
    g.wait_for_all();
    INFO(" activate");
    in.activate();  // will forward to the fnode
    INFO(" wait1");
    g.wait_for_all();
    CHECK_MESSAGE( (in.my_successors.empty()), "input node has no successor after forwarding message");
    g.reset();
    CHECK_MESSAGE( (!in.my_successors.empty()), "input_node has no successors after reset");
    CHECK_MESSAGE( (tbb::flow::input_port<0>(jn).my_predecessors.empty()), "successor of input_node has pred after reset.");
    INFO(" done\n");
}

//! Test buffering nodes
//! \brief \ref error_guessing
TEST_CASE("Test buffering nodes"){
    unsigned int MinThread = utils::MinThread;
    if(MinThread < 3) MinThread = 3;
    tbb::task_arena arena(MinThread);
	arena.execute(
        [&]() {
            // tests presume at least three threads
            TestBufferingNode< tbb::flow::buffer_node<int> >("buffer_node");
            TestBufferingNode< tbb::flow::priority_queue_node<int> >("priority_queue_node");
            TestBufferingNode< tbb::flow::queue_node<int> >("queue_node");
        }
	);
}

//! Test sequencer_node
//! \brief \ref error_guessing
TEST_CASE("Test sequencer node"){
    TestSequencerNode();
}

TEST_SUITE("Test multifunction node") {
    //! Test multifunction_node with rejecting policy
    //! \brief \ref error_guessing
    TEST_CASE("with rejecting policy"){
        TestMultifunctionNode<tbb::flow::rejecting>();
    }

    //! Test multifunction_node with queueing policy
    //! \brief \ref error_guessing
    TEST_CASE("with queueing policy") {
        TestMultifunctionNode<tbb::flow::queueing>();
    }
}

//! Test input_node
//! \brief \ref error_guessing
TEST_CASE("Test input node"){
    TestInputNode();
}

//! Test continue_node
//! \brief \ref error_guessing
TEST_CASE("Test continue node"){
    TestContinueNode();
}

//! Test function_node
//! \brief \ref error_guessing
TEST_CASE("Test function node" * doctest::may_fail()){
    TestFunctionNode();
}

//! Test join_node
//! \brief \ref error_guessing
TEST_CASE("Test join node"){
    TestJoinNode();
}

//! Test limiter_node
//! \brief \ref error_guessing
TEST_CASE("Test limiter node"){
    TestLimiterNode<void>();
    TestLimiterNode<int>();
    TestLimiterNode<tbb::flow::continue_msg>();
}

//! Test indexer_node
//! \brief \ref error_guessing
TEST_CASE("Test indexer node"){
    TestIndexerNode();
}

//! Test split_node
//! \brief \ref error_guessing
TEST_CASE("Test split node"){
    TestSplitNode();
}

//! Test broadcast, overwrite, write_once nodes
//! \brief \ref error_guessing
TEST_CASE("Test scalar node"){
    TestScalarNode<tbb::flow::broadcast_node<int> >("broadcast_node");
    TestScalarNode<tbb::flow::overwrite_node<int> >("overwrite_node");
    TestScalarNode<tbb::flow::write_once_node<int> >("write_once_node");
}

//! try_get in inactive graph
//! \brief \ref error_guessing
TEST_CASE("try_get in inactive graph"){
    tbb::flow::graph g;

    tbb::flow::input_node<int> src(g, [&](tbb::flow_control& fc) { fc.stop(); return 0;});
    deactivate_graph(g);

    int tmp = -1;
    CHECK_MESSAGE((src.try_get(tmp) == false), "try_get can not succeed");

    src.activate();
    tmp = -1;
    CHECK_MESSAGE((src.try_get(tmp) == false), "try_get can not succeed");
}

//! Test make_edge in inactive graph
//! \brief \ref error_guessing
TEST_CASE("Test make_edge in inactive graph"){
    tbb::flow::graph g;

    tbb::flow::continue_node<int> c(g, [](const tbb::flow::continue_msg&){ return 1; });

    tbb::flow::function_node<int, int> f(g, tbb::flow::serial, serial_fn_body<int>(serial_fn_state0));

    c.try_put(tbb::flow::continue_msg());
    g.wait_for_all();

    deactivate_graph(g);

    make_edge(c, f);
}

//! Test make_edge from overwrite_node in inactive graph
//! \brief \ref error_guessing
TEST_CASE("Test make_edge from overwrite_node in inactive graph"){
    tbb::flow::graph g;

    tbb::flow::queue_node<int> q(g);

    tbb::flow::overwrite_node<int> on(g);

    on.try_put(1);
    g.wait_for_all();

    deactivate_graph(g);

    make_edge(on, q);

    int tmp = -1;
    CHECK_MESSAGE((q.try_get(tmp) == false), "Message should not be passed on");
}

//! Test iterators directly
//! \brief \ref error_guessing
TEST_CASE("graph_iterator details"){
    tbb::flow::graph g;
    const tbb::flow::graph cg;

    tbb::flow::graph::iterator b = g.begin();
    tbb::flow::graph::iterator b2 = g.begin();
    ++b2;
    // Cast to a volatile pointer to workaround self assignment warnings from some compilers.
    tbb::flow::graph::iterator* volatile b2_ptr = &b2;
    b2 = *b2_ptr;
    b = b2;
    CHECK_MESSAGE((b == b2), "Assignment should make iterators equal");
}

//! const graph
//! \brief \ref error_guessing
TEST_CASE("const graph"){
    using namespace tbb::flow;

    const graph g;
    CHECK_MESSAGE((g.cbegin() == g.cend()), "Starting graph is empty");
    CHECK_MESSAGE((g.begin() == g.end()), "Starting graph is empty");

    graph g2;
    CHECK_MESSAGE((g2.begin() == g2.end()), "Starting graph is empty");
}

//! Send message to continue_node while graph is inactive
//! \brief \ref error_guessing
TEST_CASE("Send message to continue_node while graph is inactive") {
    using namespace tbb::flow;

    graph g;

    continue_node<int> c(g, [](const continue_msg&){ return 1; });
    buffer_node<int> b(g);

    make_edge(c, b);

    deactivate_graph(g);

    c.try_put(continue_msg());
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((b.try_get(tmp) == false), "Message should not arrive");
    CHECK_MESSAGE((tmp == -1), "Value should not be altered");
}


//! Bypass of a successor's message in a node with lightweight policy
//! \brief \ref error_guessing
TEST_CASE("Bypass of a successor's message in a node with lightweight policy") {
    using namespace tbb::flow;

    graph g;

    auto body = [](const int&v)->int { return v * 2; };
    function_node<int, int, lightweight> f1(g, unlimited, body);

    auto body2 = [](const int&v)->int {return v / 2;};
    function_node<int, int> f2(g, unlimited, body2);

    buffer_node<int> b(g);

    make_edge(f1, f2);
    make_edge(f2, b);

    f1.try_put(1);
    g.wait_for_all();

    int tmp = -1;
    CHECK_MESSAGE((b.try_get(tmp) == true), "Functional nodes can work in succession");
    CHECK_MESSAGE((tmp == 1), "Value should not be altered");
}
