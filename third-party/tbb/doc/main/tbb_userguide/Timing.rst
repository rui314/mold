.. _Timing:

Timing
======


When measuring the performance of parallel programs, it is usually *wall
clock* time, not CPU time, that matters. The reason is that better
parallelization typically increases aggregate CPU time by employing more
CPUs. The goal of parallelizing a program is usually to make it run
*faster* in real time.


The class ``tick_count`` in |full_name|
provides a simple interface for measuring wall clock time. A
``tick_count`` value obtained from the static method tick_count::now()
represents the current absolute time. Subtracting two ``tick_count``
values yields a relative time in ``tick_count::interval_t``, which you
can convert to seconds, as in the following example:


::


   tick_count t0 = tick_count::now();
   ... do some work ...
   tick_count t1 = tick_count::now();
   printf("work took %g seconds\n",(t1-t0).seconds());
       



Unlike some timing interfaces, ``tick_count`` is guaranteed to be safe
to use across threads. It is valid to subtract ``tick_count`` values
that were created by different threads. A ``tick_count`` difference can
be converted to seconds.


The resolution of ``tick_count`` corresponds to the highest resolution
timing service on the platform that is valid across threads in the same
process. Since the CPU timer registers are *not* valid across threads on
some platforms, this means that the resolution of tick_count can not be
guaranteed to be consistent across platforms.


.. note::

   On Linux\* OS, you may need to add -lrt to the linker command when
   you use oneapi::tbb::tick_count class. For more information, see
   `http://fedoraproject.org/wiki/Features/ChangeInImplicitDSOLinking 
   <http://fedoraproject.org/wiki/Features/ChangeInImplicitDSOLinking>`_.

