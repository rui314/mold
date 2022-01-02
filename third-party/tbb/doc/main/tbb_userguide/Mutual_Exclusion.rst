.. _Mutual_Exclusion:

Mutual Exclusion
================


Mutual exclusion controls how many threads can simultaneously run a
region of code. In |full_name|, mutual
exclusion is implemented by *mutexes* and *locks.* A mutex is an object
on which a thread can acquire a lock. Only one thread at a time can have
a lock on a mutex; other threads have to wait their turn.


The simplest mutex is ``spin_mutex``. A thread trying to acquire a lock
on a ``spin_mutex`` busy waits until it can acquire the lock. A
``spin_mutex`` is appropriate when the lock is held for only a few
instructions. For example, the following code uses a mutex
``FreeListMutex`` to protect a shared variable ``FreeList``. It checks
that only a single thread has access to ``FreeList`` at a time.

::

   Node* FreeList;
   typedef spin_mutex FreeListMutexType;
   FreeListMutexType FreeListMutex;
    

   Node* AllocateNode() {
       Node* n;
       {
           FreeListMutexType::scoped_lock lock(FreeListMutex);
           n = FreeList;
           if( n )
               FreeList = n->next;
       }
       if( !n )
           n = new Node();
       return n;
   }
    

   void FreeNode( Node* n ) {
       FreeListMutexType::scoped_lock lock(FreeListMutex);
       n->next = FreeList;
       FreeList = n;
   }


The constructor for ``scoped_lock`` waits until there are no other locks
on ``FreeListMutex``. The destructor releases the lock. The braces
inside routine ``AllocateNode`` may look unusual. Their role is to keep
the lifetime of the lock as short as possible, so that other waiting
threads can get their chance as soon as possible.


.. CAUTION:: 
   Be sure to name the lock object, otherwise it will be destroyed too
   soon. For example, if the creation of the ``scoped_lock`` object in
   the example is changed to

   ::

      FreeListMutexType::scoped_lock (FreeListMutex);

   then the ``scoped_lock`` is destroyed when execution reaches the
   semicolon, which releases the lock *before* ``FreeList`` is accessed.


The following shows an alternative way to write ``AllocateNode``:


::


   Node* AllocateNode() {
       Node* n;
       FreeListMutexType::scoped_lock lock;
       lock.acquire(FreeListMutex);
       n = FreeList;
       if( n )
           FreeList = n->next;
       lock.release();
       if( !n ) 
           n = new Node();
       return n;
   }


Method ``acquire`` waits until it can acquire a lock on the mutex;
method ``release`` releases the lock.


It is recommended that you add extra braces where possible, to clarify
to maintainers which code is protected by the lock.


If you are familiar with C interfaces for locks, you may be wondering
why there are not simply acquire and release methods on the mutex object
itself. The reason is that the C interface would not be exception safe,
because if the protected region threw an exception, control would skip
over the release. With the object-oriented interface, destruction of the
``scoped_lock`` object causes the lock to be released, no matter whether
the protected region was exited by normal control flow or an exception.
This is true even for our version of ``AllocateNode`` that used methods
``acquire`` and ``release –`` the explicit release causes the lock to be
released earlier, and the destructor then sees that the lock was
released and does nothing.


All mutexes in oneTBB have a similar interface, which not only makes
them easier to learn, but enables generic programming. For example, all
of the mutexes have a nested ``scoped_lock`` type, so given a mutex of
type ``M``, the corresponding lock type is ``M::scoped_lock``.


.. tip::
   It is recommended that you always use a ``typedef`` for the mutex
   type, as shown in the previous examples. That way, you can change the
   type of the lock later without having to edit the rest of the code.
   In the examples, you could replace the ``typedef`` with
   ``typedef queuing_mutex FreeListMutexType``, and the code would still
   be correct.

.. toctree::
   :maxdepth: 4

   ../tbb_userguide/Mutex_Flavors
   ../tbb_userguide/Reader_Writer_Mutexes
   ../tbb_userguide/UpgradeDowngrade
   ../tbb_userguide/Lock_Pathologies
