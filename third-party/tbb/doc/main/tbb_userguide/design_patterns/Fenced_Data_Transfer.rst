.. _Fenced_Data_Transfer:

Fenced Data Transfer
====================


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Write a message to memory and have another processor read it on
   hardware that does not have a sequentially consistent memory model.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   The problem normally arises only when unsynchronized threads
   concurrently act on a memory location, or are using reads and writes
   to create synchronization. High level synchronization constructs
   normally include mechanisms that prevent unwanted reordering.


   Modern hardware and compilers can reorder memory operations in a way
   that preserves the order of a thread's operation from its viewpoint,
   but not as observed by other threads. A serial common idiom is to
   write a message and mark it as ready to ready as shown in the
   following code:


   ::


      bool Ready;
      std::string Message;
       

      void Send( const std::string& src ) {. // Executed by thread 1
         Message=src;
         Ready = true;
      }
       

      bool Receive( std::string& dst ) {    // Executed by thread 2
         bool result = Ready;
         if( result ) dst=Message;
         return result;              // Return true if message was received.
      }


   Two key assumptions of the code are:


   #. ``Ready`` does not become true until ``Message`` is written.


   #. ``Message`` is not read until ``Ready`` becomes true.


   These assumptions are trivially true on uniprocessor hardware.
   However, they may break on multiprocessor hardware. Reordering by the
   hardware or compiler can cause the sender's writes to appear out of
   order to the receiver (thus breaking condition a) or the receiver's
   reads to appear out of order (thus breaking condition b).


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  Creating synchronization via raw reads and writes.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Change the flag from ``bool`` to ``std::atomic<bool>`` for the flag
   that indicates when the message is ready. Here is the previous
   example with modifications.


   ::


      std::atomic<bool> Ready;
      std::string Message;
       

      void Send( const std::string& src ) {. // Executed by thread 1
         Message=src;
         Ready.store(true, std::memory_order_release);
      }
       

      bool Receive( std::string& dst ) {    // Executed by thread 2
         bool result = Ready.load(std::memory_order_acquire);
         if( result ) dst=Message;
         return result;              // Return true if message was received.
      }


   A write to a ``std::atomic`` value has *release* semantics, which
   means that all of its prior writes will be seen before the releasing
   write. A read from ``std::atomic`` value has *acquire* semantics,
   which means that all of its subsequent reads will happen after the
   acquiring read. The implementation of ``std::atomic`` ensures that
   both the compiler and the hardware observe these ordering
   constraints.


.. container:: section


   .. rubric:: Variations
      :class: sectiontitle

   Higher level synchronization constructs normally include the
   necessary *acquire* and *release* fences. For example, mutexes are
   normally implemented such that acquisition of a lock has *acquire*
   semantics and release of a lock has *release* semantics. Thus a
   thread that acquires a lock on a mutex always sees any memory writes
   done by another thread before it released a lock on that mutex.


.. container:: section


   .. rubric:: Non Solutions
      :class: sectiontitle

   Mistaken solutions are so often proposed that it is worth
   understanding why they are wrong.


   One common mistake is to assume that declaring the flag with the
   ``volatile`` keyword solves the problem. Though the ``volatile``
   keyword forces a write to happen immediately, it generally has no
   effect on the visible ordering of that write with respect to other
   memory operations.


   Another mistake is to assume that conditionally executed code cannot
   happen before the condition is tested. However, the compiler or
   hardware may speculatively hoist the conditional code above the
   condition.


   Similarly, it is a mistake to assume that a processor cannot read the
   target of a pointer before reading the pointer. A modern processor
   does not read individual values from main memory. It reads cache
   lines. The target of a pointer may be in a cache line that has
   already been read before the pointer was read, thus giving the
   appearance that the processor presciently read the pointer target.

