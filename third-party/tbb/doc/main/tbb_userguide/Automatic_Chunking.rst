.. _Automatic_Chunking:

Automatic Chunking
==================


A parallel loop construct incurs overhead cost for every chunk of work
that it schedules. |full_name|
chooses chunk sizes automatically, depending upon load balancing
needs. The heuristic attempts to limit overheads while
still providing ample opportunities for load balancing.


.. CAUTION::
   Typically a loop needs to take at least a million clock cycles to
   make it worth using ``parallel_for``. For example, a loop that takes
   at least 500 microseconds on a 2 GHz processor might benefit from
   ``parallel_for``.


The default automatic chunking is recommended for most uses. As with
most heuristics, however, there are situations where controlling the
chunk size more precisely might yield better performance.
