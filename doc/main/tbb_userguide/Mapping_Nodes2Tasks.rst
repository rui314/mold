.. _Mapping_Nodes2Tasks:

Flow Graph Basics: Mapping Nodes to Tasks
=========================================


The following figure shows the timeline for one possible execution of
the two node graph example in the previous section. The bodies of n and
m will be referred to as λ\ :sub:`n` and λ\ :sub:`m`, respectively. The
three calls to try_put spawn three tasks; each one applies the lambda
expression, λ\ :sub:`n`, on one of the three input messages. Because n
has unlimited concurrency, these tasks can execute concurrently if there
are enough threads available. The call to ``g.wait_for_all()`` blocks until
there are no tasks executing in the graph. As with other ``wait_for_all``
functions in oneTBB, the thread that calls ``wait_for_all`` is not spinning
idly during this time, but instead can join in executing other tasks
from the work pool.


.. container:: fignone


   **Execution Timeline of a Two Node Graph**


   .. container:: imagecenter


      |image0|


As each task from n finishes, it puts its output to m, since m is a
successor of n. Unlike node n, m has been constructed with a concurrency
limit of 1 and therefore does not spawn all tasks immediately. Instead,
it sequentially spawns tasks to execute its body, λ\ :sub:`m`, on the
messages in the order that they arrive. When all tasks are complete, the
call to ``wait_for_all`` returns.


.. note:: 
   All execution in the flow graph happens asynchronously. The calls to
   try_put return control to the calling thread quickly, after either
   immediately spawning a task or buffering the message being passed.
   Likewise, the body tasks execute the lambda expressions and then put
   the result to any successor nodes. Only the call to ``wait_for_all``
   blocks, as it should, and even in this case the calling thread may be
   used to execute tasks from the oneTBB work pool while it is waiting.


The above timeline shows the sequence when there are enough threads to
execute all of the tasks that can be executed in parallel. If there are
fewer threads, some spawned tasks will need to wait until a thread is
available to execute them.


.. |image0| image:: Images/execution_timeline2node.jpg

