.. _Containers:

Containers
==========


|full_name| provides highly concurrent
container classes. These containers can be used with raw Windows\* OS or
Linux\* OS threads, or in conjunction with task-based programming.


A concurrent container allows multiple threads to concurrently access
and update items in the container. Typical C++ STL containers do not
permit concurrent update; attempts to modify them concurrently often
result in corrupting the container. STL containers can be wrapped in a
mutex to make them safe for concurrent access, by letting only one
thread operate on the container at a time, but that approach eliminates
concurrency, thus restricting parallel speedup.


Containers provided by oneTBB offer a much higher level of concurrency,
via one or both of the following methods:


-  **Fine-grained locking:** Multiple threads operate on the container
   by locking only those portions they really need to lock. As long as
   different threads access different portions, they can proceed
   concurrently.


-  **Lock-free techniques:** Different threads account and correct for
   the effects of other interfering threads.


Notice that highly-concurrent containers come at a cost. They typically
have higher overheads than regular STL containers. Operations on
highly-concurrent containers may take longer than for STL containers.
Therefore, use highly-concurrent containers when the speedup from the
additional concurrency that they enable outweighs their slower
sequential performance.


.. CAUTION:: 
   As with most objects in C++, the constructor or destructor of a
   container object must not be invoked concurrently with another
   operation on the same object. Otherwise the resulting race may cause
   the operation to be executed on an undefined object.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/concurrent_hash_map
   ../tbb_userguide/concurrent_vector_ug
   ../tbb_userguide/Concurrent_Queue_Classes
   ../tbb_userguide/Summary_of_Containers
