.. _Iterating_Over_a_Concurrent_Queue_for_Debugging:

Iterating Over a Concurrent Queue for Debugging
===============================================


The template classes ``concurrent_queue`` and
``concurrent_bounded_queue`` support STL-style iteration. This support
is intended only for debugging, when you need to dump a queue. The
iterators go forwards only, and are too slow to be very useful in
production code. If a queue is modified, all iterators pointing to it
become invalid and unsafe to use. The following snippet dumps a queue.
The ``operator<<`` is defined for a ``Foo``.


::


   concurrent_queue<Foo> q;
   ...
   typedef concurrent_queue<Foo>::const_iterator iter;
   for(iter i(q.unsafe_begin()); i!=q.unsafe_end(); ++i ) {
       cout << *i;
   }


The prefix ``unsafe_`` on the methods is a reminder that they are not
concurrency safe.

