.. _creating_tasks_with_parallel_invoke:

Creating Tasks with parallel_invoke
===================================

Suppose you want to search a binary tree for the node that contains a specific value.
Here is sequential code to do this:

Nodes are represented by ``struct TreeNode``:

.. literalinclude:: ./examples/task_examples.cpp
    :language: c++
    :start-after: /*begin_treenode*/
    :end-before: /*end_treenode*/

The function ``serial_tree_search`` is a recursive algorithm that checks the current node
and, if the value is not found, calls itself on the left and right subtree. 

.. literalinclude:: ./examples/task_examples.cpp
    :language: c++
    :start-after: /*begin_search_serial*/
    :end-before: /*end_search_serial*/


To improve performance, you can use ``oneapi::tbb::parallel_invoke`` to search the tree 
in parallel:

A recursive base case is used after a minimum size threshold is reached to avoid parallel overheads.
Since more than one thread can call the base case concurrently as part of the same tree, ``result``
is held in an atomic variable.

.. literalinclude:: ./examples/task_examples.cpp
    :language: c++
    :start-after: /*begin_sequential_tree_search*/
    :end-before: /*end_sequential_tree_search*/

The function ``oneapi::tbb::parallel_invoke`` runs multiple independent tasks in parallel.
Here is a function ``parallel_invoke_search``, where two lambdas are passed that define tasks
that search the left and right subtrees of the current node in parallel:

.. literalinclude:: ./examples/task_examples.cpp
    :language: c++
    :start-after: /*begin_parallel_invoke_search*/
    :end-before: /*end_parallel_invoke_search*/

If the value is found, the pointer to the node that contains the value is stored
in the ``std::atomic<TreeNode*> result``. This example uses recursion to create many tasks, instead of
just two. The depth of the parallel recursion is limited by the ``depth_threshold`` parameter. After this depth is
reached, the search falls back to a sequential approach. The value of ``result`` is periodically checked
to see if the value has been found by other concurrent tasks, and if so, the search in the current task is
terminated. Because multiple threads may access ``result`` concurrently, an atomic variable is used, even
from the sequential base case.

Because ``oneapi::tbb::parallel_invoke`` is a fork-join algorithm, each level of the recursion does not
complete until both the left and right subtrees have completed.