.. _Advanced_Topic_Other_Kinds_of_Iteration_Spaces:

Advanced Topic: Other Kinds of Iteration Spaces
===============================================


The examples so far have used the class ``blocked_range<T>`` to specify ranges.
This class is useful in many situations, but it does not fit every situation.
You can use |full_name| to define your own iteration space objects. The object
must specify how it can be split into subspaces by providing a basic splitting
constructor, an optional proportional splitting constructor, and two predicate
methods. If your class is called ``R``, the methods and constructors should be
as follows:


::


   class R {
       // True if range is empty
       bool empty() const;
       // True if range can be split into non-empty subranges
       bool is_divisible() const;
       // Splits r into subranges r and *this
       R( R& r, split );
       // (optional) Splits r into subranges r and *this in proportion p
       R( R& r, proportional_split p );
       ...
   };
       



The method ``empty`` should return true if the range is empty. The
method ``is_divisible`` should return true if the range can be split
into two non-empty subspaces, and such a split is worth the overhead.
The basic splitting constructor should take two arguments:


-  The first of type ``R``


-  The second of type oneapi::tbb::split


The second argument is not used; it serves only to distinguish the
constructor from an ordinary copy constructor. The basic splitting
constructor should attempt to split ``r`` roughly into two halves, and
update ``r`` to be the first half, and set the constructed object as the
second half.


Unlike the basic splitting constructor, the proportional splitting
constructor is optional and takes the second argument of type
``oneapi::tbb::proportional_split``. The type has methods ``left`` and ``right``
that return the values of the proportion. These values should be used to
split ``r`` accordingly, so that the updated ``r`` corresponds to the
left part of the proportion, and the constructed object corresponds to
the right part.


Both splitting constructors should guarantee that the updated ``r`` part
and the constructed object are not empty. The parallel algorithm
templates call the splitting constructors on ``r`` only if
``r.is_divisible`` is true.


The iteration space does not have to be linear. Look at
``oneapi/tbb/blocked_range2d.h`` for an example of a range that is
two-dimensional. Its splitting constructor attempts to split the range
along its longest axis. When used with ``parallel_for``, it causes the
loop to be "recursively blocked" in a way that improves cache usage.
This nice cache behavior means that using ``parallel_for`` over a
``blocked_range2d<T>`` can make a loop run faster than the sequential
equivalent, even on a single processor.
