.. _Benefits:

|short_name| Benefits
=====================


|full_name| is a library that helps you leverage multi-core performance
without having to be a threading expert. Typically you can improve
performance for multi-core processors by implementing the key points
explained in the early sections of the Developer Guide. As your
expertise grows, you may want to dive into more complex subjects that
are covered in advanced sections.


There are a variety of approaches to parallel programming, ranging from
using platform-dependent threading primitives to exotic new languages.
The advantage of oneTBB is that it works at a higher level than raw
threads, yet does not require exotic languages or compilers. You can use
it with any compiler supporting ISO C++. The library differs from
typical threading packages in the following ways:


-  **oneTBB enables you to specify logical paralleism instead of
   threads**. Most threading packages require you to specify threads.
   Programming directly in terms of threads can be tedious and lead to
   inefficient programs, because threads are low-level, heavy constructs
   that are close to the hardware. Direct programming with threads
   forces you to efficiently map logical tasks onto threads. In
   contrast, the oneTBB run-time library automatically maps logical
   parallelism onto threads in a way that makes efficient use of
   processor resources.


-  **oneTBB targets threading for performance**. Most general-purpose
   threading packages support many different kinds of threading, such as
   threading for asynchronous events in graphical user interfaces. As a
   result, general-purpose packages tend to be low-level tools that
   provide a foundation, not a solution. Instead, oneTBB focuses on the
   particular goal of parallelizing computationally intensive work,
   delivering higher-level, simpler solutions.


-  **oneTBB is compatible with other threading packages.** Because the
   library is not designed to address all threading problems, it can
   coexist seamlessly with other threading packages.


-  **oneTBB emphasizes scalable, data parallel programming**. Breaking a
   program up into separate functional blocks, and assigning a separate
   thread to each block is a solution that typically does not scale well
   since typically the number of functional blocks is fixed. In
   contrast, oneTBB emphasizes *data-parallel* programming, enabling
   multiple threads to work on different parts of a collection.
   Data-parallel programming scales well to larger numbers of processors
   by dividing the collection into smaller pieces. With data-parallel
   programming, program performance increases as you add processors.


-  **oneTBB relies on generic programming**. Traditional libraries
   specify interfaces in terms of specific types or base classes.
   Instead, oneAPI Threading Building Blocks uses generic programming.
   The essence of generic programming is writing the best possible
   algorithms with the fewest constraints. The C++ Standard Template
   Library (STL) is a good example of generic programming in which the
   interfaces are specified by *requirements* on types. For example, C++
   STL has a template function ``sort`` that sorts a sequence abstractly
   defined in terms of iterators on the sequence. The requirements on
   the iterators are:


   -  Provide random access


   -  The expression ``*i<*j`` is true if the item pointed to by
      iterator ``i`` should precede the item pointed to by iterator
      ``j``, and false otherwise.


   -  The expression ``swap(*i,*j)`` swaps two elements.


Specification in terms of requirements on types enables the template to
sort many different representations of sequences, such as vectors and
deques. Similarly, the oneTBB templates specify requirements on types,
not particular types, and thus adapt to different data representations.
Generic programming enables oneTBB to deliver high performance
algorithms with broad applicability.

