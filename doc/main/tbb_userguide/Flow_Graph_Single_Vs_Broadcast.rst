.. _Flow_Graph_Single_Vs_Broadcast:

Flow Graph Basics: Single-push vs. Broadcast-push
=================================================


Nodes in the |full_name| flow graph
communicate by pushing and pulling messages. Two policies for pushing
messages are used, depending on the type of the node:


-  **single-push**: No matter how many successors to the node exist and
   are able to accept a message, each message will be only sent to one
   successor.
-  **broadcast-push**: A message will be pushed to every successor which
   is connected to the node by an edge in push mode, and which accepts
   the message.


The following code demonstrates this difference:


::


   using namespace oneapi::tbb::flow;


   std::atomic<size_t> g_cnt;


   struct fn_body1 {
       std::atomic<size_t> &body_cnt;
       fn_body1(std::atomic<size_t> &b_cnt) : body_cnt(b_cnt) {}
       continue_msg operator()( continue_msg /*dont_care*/) {
           ++g_cnt;
           ++body_cnt;
           return continue_msg();
       }
   };


   void run_example1() {  // example for Flow_Graph_Single_Vs_Broadcast.xml
       graph g;
       std::atomic<size_t> b1;  // local counts
       std::atomic<size_t> b2;  // for each function _node body
       std::atomic<size_t> b3;  //
       function_node<continue_msg> f1(g,serial,fn_body1(b1));
       function_node<continue_msg> f2(g,serial,fn_body1(b2));
       function_node<continue_msg> f3(g,serial,fn_body1(b3));
       buffer_node<continue_msg> buf1(g);
       //
       // single-push policy
       //
       g_cnt = b1 = b2 = b3 = 0;
       make_edge(buf1,f1);
       make_edge(buf1,f2);
       make_edge(buf1,f3);
       buf1.try_put(continue_msg());
       buf1.try_put(continue_msg());
       buf1.try_put(continue_msg());
       g.wait_for_all();
       printf( "after single-push test, g_cnt == %d, b1==%d, b2==%d, b3==%d\n", (int)g_cnt, (int)b1, (int)b2, (int)b3);
       remove_edge(buf1,f1);
       remove_edge(buf1,f2);
       remove_edge(buf1,f3);
       //
       // broadcast-push policy
       //
       broadcast_node<continue_msg> bn(g);
       g_cnt = b1 = b2 = b3 = 0;
       make_edge(bn,f1);
       make_edge(bn,f2);
       make_edge(bn,f3);
       bn.try_put(continue_msg());
       bn.try_put(continue_msg());
       bn.try_put(continue_msg());
       g.wait_for_all();
       printf( "after broadcast-push test, g_cnt == %d, b1==%d, b2==%d, b3==%d\n", (int)g_cnt, (int)b1, (int)b2, (int)b3);
   }


The output of this code is


::


   after single-push test, g_cnt == 3, b1==3, b2==0, b3==0
   after broadcast-push test, g_cnt == 9, b1==3, b2==3, b3==3


The single-push test uses a ``buffer_node``, which has a "single-push"
policy for forwarding messages. Putting three messages to the
``buffer_node`` results in three messages being pushed. Notice also only
the first ``function_node`` is sent to; in general there is no policy
for which node is pushed to if more than one successor can accept.


The broadcast-push test uses a ``broadcast_node``, which will push any
message it receives to all accepting successors. Putting three messages
to the ``broadcast_node`` results in a total of nine messages pushed to
the ``function_nodes``.


Only nodes designed to buffer (hold and forward received messages) have
a "single-push" policy; all other nodes have a "broadcast-push" policy.

Please see the :ref:`broadcast_or_send` section of
:ref:`Flow_Graph_Tips`, and :ref:`Flow_Graph_Buffering_in_Nodes` for more
information.

