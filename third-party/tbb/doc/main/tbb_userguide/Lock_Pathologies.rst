.. _Lock_Pathologies:

Lock Pathologies
================


Locks can introduce performance and correctness problems. If you are new
to locking, here are some of the problems to avoid:


.. container:: section


   .. rubric:: Deadlock
      :class: sectiontitle

   Deadlock happens when threads are trying to acquire more than one
   lock, and each holds some of the locks the other threads need to
   proceed. More precisely, deadlock happens when:


   -  There is a cycle of threads


   -  Each thread holds at least one lock on a mutex, and is waiting on
      a mutex for which the *next* thread in the cycle already has a
      lock.


   -  No thread is willing to give up its lock.


   Think of classic gridlock at an intersection – each car has
   "acquired" part of the road, but needs to "acquire" the road under
   another car to get through. Two common ways to avoid deadlock are:


   -  Avoid needing to hold two locks at the same time. Break your
      program into small actions in which each can be accomplished while
      holding a single lock.


   -  Always acquire locks in the same order. For example, if you have
      "outer container" and "inner container" mutexes, and need to
      acquire a lock on one of each, you could always acquire the "outer
      sanctum" one first. Another example is "acquire locks in
      alphabetical order" in a situation where the locks have names. Or
      if the locks are unnamed, acquire locks in order of the mutex’s
      numerical addresses.


   -  Use atomic operations instead of locks.


.. container:: section


   .. rubric:: Convoying
      :class: sectiontitle

   Another common problem with locks is *convoying*. Convoying occurs
   when the operating system interrupts a thread that is holding a lock.
   All other threads must wait until the interrupted thread resumes and
   releases the lock. Fair mutexes can make the situation even worse,
   because if a waiting thread is interrupted, all the threads behind it
   must wait for it to resume.


   To minimize convoying, try to hold the lock as briefly as possible.
   Precompute whatever you can before acquiring the lock.


   To avoid convoying, use atomic operations instead of locks where
   possible.

