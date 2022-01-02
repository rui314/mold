.. _Concurrent_Queue_Classes:

Concurrent Queue Classes
========================


Template class ``concurrent_queue<T,Alloc>`` implements a concurrent
queue with values of type ``T``. Multiple threads may simultaneously
push and pop elements from the queue. The queue is unbounded and has no
blocking operations. The fundamental operations on it are ``push`` and
``try_pop``. The ``push`` operation works just like ``push`` for a
std::queue. The operation ``try_pop`` pops an item if it is available.
The check and popping have to be done in a single operation for sake of
thread safety.


For example, consider the following serial code:


::


           extern std::queue<T> MySerialQueue;
           T item;
           if( !MySerialQueue.empty() ) {
               item = MySerialQueue.front(); 
               MySerialQueue.pop_front();
               ... process item...
           }


Even if each std::queue method were implemented in a thread-safe manner,
the composition of those methods as shown in the example would not be
thread safe if there were other threads also popping from the same
queue. For example, ``MySerialQueue.empty()`` might return true just
before another thread snatches the last item from ``MySerialQueue``.


The equivalent thread-safe |full_name| code is:


::


           extern concurrent_queue<T> MyQueue;
           T item;
           if( MyQueue.try_pop(item) ) {
               ...process item...
           }            


In a single-threaded program, a queue is a first-in first-out structure.
But if multiple threads are pushing and popping concurrently, the
definition of "first" is uncertain. Use of ``concurrent_queue``
guarantees that if a thread pushes two values, and another thread pops
those two values, they will be popped in the same order that they were
pushed.


Template class ``concurrent_queue`` is unbounded and has no methods that
wait. It is up to the user to provide synchronization to avoid overflow,
or to wait for the queue to become non-empty. Typically this is
appropriate when the synchronization has to be done at a higher level.


Template class ``concurrent_bounded_queue<T,Alloc>`` is a variant that
adds blocking operations and the ability to specify a capacity. The
methods of particular interest on it are:


-  ``pop(item)`` waits until it can succeed.


-  ``push(item)`` waits until it can succeed without exceeding the
   queue's capacity.


-  ``try_push(item)`` pushes ``item`` only if it would not exceed the
   queue's capacity.


-  size() returns a *signed* integer.


The value of concurrent_queue::size() is defined as the number of push
operations started minus the number of pop operations started. If pops
outnumber pushes, ``size()`` becomes negative. For example, if a
``concurrent_queue`` is empty, and there are ``n`` pending pop
operations, ``size()`` returns -\ ``n``. This provides an easy way for
producers to know how many consumers are waiting on the queue. Method
``empty()`` is defined to be true if and only if ``size()`` is not
positive.


By default, a ``concurrent_bounded_queue`` is unbounded. It may hold any
number of values, until memory runs out. It can be bounded by setting
the queue capacity with method ``set_capacity``.Setting the capacity
causes ``push`` to block until there is room in the queue. Bounded
queues are slower than unbounded queues, so if there is a constraint
elsewhere in your program that prevents the queue from becoming too
large, it is better not to set the capacity. If you do not need the
bounds or the blocking pop, consider using ``concurrent_queue`` instead.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Iterating_Over_a_Concurrent_Queue_for_Debugging
   ../tbb_userguide/When_Not_to_Use_Queues