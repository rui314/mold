.. _Predefined_Node_Types:

Predefined Node Types
=====================


You can define your own node types by inheriting from class graph_node,
class sender and class receiver but it is likely that you can create
your graph with the predefined node types already available in
flow_graph.h. Below is a table that lists all of the predefined types
with a basic description. See the Developer Reference for a more
detailed description of each node.


.. container:: tablenoborder


   .. list-table:: 
      :header-rows: 1
      :widths: 25 25

      * - Predefined Node Type 
        - Description 
      * - ``input_node``
        - A single-output node, with a generic output type.
          When activated, it executes a user body to generate its output. Its body is invoked if downstream nodes have accepted the previous generated output.
          Otherwise, the previous output is temporarily buffered until it is accepted downstream and then the body is again invoked.
      * - ``function_node``
        - A single-input single-output node that broadcasts its output to all successors. Has generic input and output types. Executes a user body and has controllable concurrency level and buffering policy. For each input exactly one output is returned.
      * - ``continue_node`` 
        - A single-input, single-output node that broadcasts its output to all successors. It has a single input that requires 1 or more inputs   of type ``continue_msg`` and has a generic output type. It executes a   user body when it receives N ``continue_msg objects`` at its input. N is   equal to the number of predecessors plus any additional offset   assigned at construction time.
      * - ``multifunction_node``
        - A single-input multi-output node. It has a generic input type and    several generic output types. It executes a user body, and has   controllable concurrency level and buffering policy. The body can   output zero or more messages on each output port.
      * - ``broadcast_node`` 
        - A single-input, single-output node that broadcasts each message    received to all successors. Its input and output are of the same   generic type. It does not buffer messages.
      * - ``buffer_node``, ``queue_node``, ``priority_queue_node``, and ``sequencer_node``. 
        - Single-input, single-output nodes that buffer messages and send    their output to one successor. The order in which the messages are   sent are node specific (see the Developer Reference). These nodes are   unique in that they send to only a single successor and not all   successors.
      * - ``join_node``
        - A multi-input, single-output node. There are several generic    input types and the output type is a tuple of these generic types.   The node combines one message from each input port to create a tuple   that is broadcast to all successors. The policy used to combine   messages is selectable as queueing, reserving or tag-matching.
      * - ``split_node`` 
        - A single-input, multi-output node. The input type is a tuple of    generic types and there is one output port for each of the types in   the tuple. The node receives a tuple of values and outputs each   element of the tuple on a corresponding output port.
      * - ``write_once_node``, ``overwrite_node`` 
        - Single-input, single-output nodes that buffer a single message    and broadcast their outputs to all successors. After broadcast, the   nodes retain the last message received, so it is available to any   future successor. A ``write_once_node`` will only accept the first   message it receives, while the ``overwrite_node`` will accept all   messages, broadcasting them to all successors, and replacing the old   value with the new.
      * - ``limiter_node`` 
        - A multi-input, single output node that broadcasts its output to    all successors. The main input type and output type are of the same   generic type. The node increments an internal counter when it   broadcasts a message. If the increment causes it to reach its   user-assigned threshold, it will broadcast no more messages. A   special input port can be used to adjust the internal count, allowing   further messages to be broadcast. The node does not buffer messages.
      * - ``indexer_node`` 
        - A multi-input, single-output node that broadcasts its output    message to all of its successors. The input type is a list of generic   types and the output type is a ``tagged_msg``. The message is of one of   the types listed in the input and the tag identifies the port on   which the message was received. Messages are broadcast individually   as they arrive at the input ports.
      * - ``composite_node`` 
        - A node that might have 0, 1 or multiple ports for both input and    output. The ``composite_node`` packages a group of other nodes together   and maintains a tuple of references to ports that border it. This   allows for the corresponding ports of the ``composite_node`` to be used   to make edges which hitherto would have been made from the actual   nodes in the ``composite_node``.
      * - async_node (preview feature) 
        - A node that allows a flow graph to communicate with an external    activity managed by the user or another runtime. This node receives   messages of generic type, invokes the user-provided body to submit a   message to an external activity. The external activity can use a   special interface to return a generic type and put it to all   successors of ``async_node``.



