.. _cancelling_nested_parallelism:

Canceling Nested Parallelism
============================


Nested parallelism is canceled if the inner context is bound to the
outer context; otherwise it is not.


If the execution of a flow graph is canceled, either explicitly or due
to an exception, any tasks started by parallel algorithms or flow graphs
nested within the nodes of the canceled flow graph may or may not be
canceled.


As with all of the library's nested parallelism, you can control
cancellation relationships by use of explicit task_group_context
objects. If you do not provide an explicit task_group_context to a flow
graph, it is created with an isolated context by default.

