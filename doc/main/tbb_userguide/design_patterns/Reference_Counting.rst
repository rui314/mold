.. _Reference_Counting:

Reference Counting
==================


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Destroy an object when it will no longer be used.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Often it is desirable to destroy an object when it is known that it
   will not be used in the future. Reference counting is a common serial
   solution that extends to parallel programming if done carefully.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  If there are cycles of references, basic reference counting is
      insufficient unless the cycle is explicitly broken.


   -  Atomic counting is relatively expensive in hardware.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Thread-safe reference counting is like serial reference counting,
   except that the increment/decrement is done atomically, and the
   decrement and test "count is zero?" must act as a single atomic
   operation. The following example uses ``std::atomic<int>`` to achieve
   this.


   ::


      template<typename T>
      class counted {
         std::atomic<int> my_count;
         T value;
      public:
         // Construct object with a single reference to it.
         counted() {my_count=1;}
         // Add reference
         void add_ref() {++my_count;}
         // Remove reference. Return true if it was the last reference.
         bool remove_ref() {return --my_count==0;}
         // Get reference to underlying object
         T& get() {
             assert(my_count>0);
             return my_value;
         }
      };


   It is incorrect to use a separate read for testing if the count is
   zero. The following code would be an incorrect implementation of
   method ``remove_ref``\ () because two threads might both execute the
   decrement, and then both read ``my_count`` as zero. Hence two callers
   would both be told incorrectly that they had removed the last
   reference.


   ::


            --my_count;
            return my_count==0. // WRONG!


   The decrement may need to have a *release* fence so that any pending
   writes complete before the object is deleted.


   There is no simple way to atomically copy a pointer and increment its
   reference count, because there will be a timing hole between the
   copying and the increment where the reference count is too low, and
   thus another thread might decrement the count to zero and delete the
   object. Two ways to address the problem are "hazard pointers" and
   "pass the buck". See the references below for details.


.. container:: section


   .. rubric:: Variations
      :class: sectiontitle

   Atomic increment/decrement can be more than an order of magnitude
   more expensive than ordinary increment/decrement. The serial
   optimization of eliminating redundant increment/decrement operations
   becomes more important with atomic reference counts.


   Weighted reference counting can be used to reduce costs if the
   pointers are unshared but the referent is shared. Associate a
   *weight* with each pointer. The reference count is the sum of the
   weights. A pointer ``x`` can be copied as a pointer ``x'`` without
   updating the reference count by splitting the original weight between
   ``x`` and ``x'``. If the weight of ``x`` is too low to split, then first add a
   constant W to the reference count and weight of ``x``.


.. container:: section


   .. rubric:: References
      :class: sectiontitle

   D. Bacon and V.T. Rajan, "Concurrent Cycle Collection in Reference
   Counted Systems" in Proc. European Conf. on Object-Oriented
   Programming (June 2001). Describes a garbage collector based on
   reference counting that does collect cycles.


   M. Michael, "Hazard Pointers: Safe Memory Reclamation for Lock-Free
   Objects" in IEEE Transactions on Parallel and Distributed Systems
   (June 2004). Describes the "hazard pointer" technique.


   M. Herlihy, V. Luchangco, and M. Moir, "The Repeat Offender Problem:
   A Mechanism for Supporting Dynamic-Sized, Lock-Free Data Structures"
   in Proceedings of the 16th International Symposium on Distributed
   Computing (Oct. 2002). Describes the "pass the buck" technique.

