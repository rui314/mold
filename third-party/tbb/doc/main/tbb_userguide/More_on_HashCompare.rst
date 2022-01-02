.. _More_on_HashCompare:

More on HashCompare
===================


There are several ways to make the ``HashCompare`` argument for
``concurrent_hash_map`` work for your own types.


-  Specify the ``HashCompare`` argument explicitly


-  Let the ``HashCompare`` default to ``tbb_hash_compare<Key>`` and do
   one of the following:


   -  Define a specialization of template ``tbb_hash_compare<Key>``.


For example, if you have keys of type ``Foo``, and ``operator==`` is
defined for ``Foo``, you just have to provide a definition of
``tbb_hasher`` as shown below:


::


   size_t tbb_hasher(const Foo& f) {
       size_t h = ...compute hash code for f...
       return h;
   };


In general, the definition of ``tbb_hash_compare<Key>`` or
``HashCompare`` must provide two signatures:


-  A method ``hash`` that maps a ``Key`` to a ``size_t``


-  A method ``equal`` that determines if two keys are equal


The signatures go together in a single class because *if two keys are
equal, then they must hash to the same value*, otherwise the hash table
might not work. You could trivially meet this requirement by always
hashing to ``0``, but that would cause tremendous inefficiency. Ideally,
each key should hash to a different value, or at least the probability
of two distinct keys hashing to the same value should be kept low.


The methods of ``HashCompare`` should be ``static`` unless you need to
have them behave differently for different instances. If so, then you
should construct the ``concurrent_hash_map`` using the constructor that
takes a ``HashCompare`` as a parameter. The following example is a
variation on an earlier example with instance-dependent methods. The
instance performs both case-sensitive or case-insensitive hashing, and
comparison, depending upon an internal flag ``ignore_case``.


::


   // Structure that defines hashing and comparison operations
   class VariantHashCompare {
       // If true, then case of letters is ignored.
       bool ignore_case;
   public:
       size_t hash(const string& x) const {
           size_t h = 0;
           for(const char* s = x.c_str(); *s; s++) 
               h = (h*16777179)^*(ignore_case?tolower(*s):*s);
           return h;
       }
       // True if strings are equal
       bool equal(const string& x, const string& y) const {
           if( ignore_case )
               strcasecmp(x.c_str(), y.c_str())==0;
           else
               return x==y;
       }
       VariantHashCompare(bool ignore_case_) : ignore_case(ignore_case_) {}
   };
    

   typedef concurrent_hash_map<string,int, VariantHashCompare> VariantStringTable;
    

   VariantStringTable CaseSensitiveTable(VariantHashCompare(false));
   VariantStringTable CaseInsensitiveTable(VariantHashCompare(true));
