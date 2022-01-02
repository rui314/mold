.. _concurrent_vector_ug:

concurrent_vector
=================


``A concurrent_vector<T>`` is a dynamically growable array of ``T``. It
is safe to grow a ``concurrent_vector`` while other threads are also
operating on elements of it, or even growing it themselves. For safe
concurrent growing, ``concurrent_vector`` has three methods that support
common uses of dynamic arrays: ``push_back``, ``grow_by``, and
``grow_to_at_least``.


Method ``push_back(x)`` safely appends x to the array. Method
``grow_by(n)`` safely appends ``n`` consecutive elements initialized
with ``T()``. Both methods return an iterator pointing to the first
appended element. Each element is initialized with ``T()``. So for
example, the following routine safely appends a C string to a shared
vector:


::


   void Append( concurrent_vector<char>& vector, const char* string ) {
       size_t n = strlen(string)+1;
       std::copy( string, string+n, vector.grow_by(n) );
   }


The related method ``grow_to_at_least(n)``\ grows a vector to size ``n``
if it is shorter. Concurrent calls to the growth methods do not
necessarily return in the order that elements are appended to the
vector.


Method ``size()`` returns the number of elements in the vector, which
may include elements that are still undergoing concurrent construction
by methods ``push_back``, ``grow_by,`` or ``grow_to_at_least``. The
example uses std::copy and iterators, not ``strcpy and pointers``,
because elements in a ``concurrent_vector`` might not be at consecutive
addresses. It is safe to use the iterators while the
``concurrent_vector`` is being grown, as long as the iterators never go
past the current value of ``end()``. However, the iterator may reference
an element undergoing concurrent construction. You must synchronize
construction and access.


A ``concurrent_vector<T>`` never moves an element until the array is
cleared, which can be an advantage over the STL std::vector even for
single-threaded code. However, ``concurrent_vector`` does have more
overhead than std::vector. Use ``concurrent_vector`` only if you really
need the ability to dynamically resize it while other accesses are (or
might be) in flight, or require that an element never move.


.. CAUTION:: 
   Operations on ``concurrent_vector`` are concurrency safe with respect
   to *growing*, not for clearing or destroying a vector. Never invoke
   method ``clear()`` if there are other operations in flight on the
   ``concurrent_vector``.
