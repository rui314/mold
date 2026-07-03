#include <oneapi/tbb/tbb_stddef.h> // for split tags

struct TrivialNaturalRange {
    // restore the default constructor
    TrivialNaturalRange() {}

    size_t lower;
    size_t upper;
    bool empty() const { return lower == upper; }
    bool is_divisible() const { return upper > lower + 1; }

    // basic splitting constructor
    TrivialNaturalRange(TrivialNaturalRange& r, oneapi::tbb::split) {
        size_t m = r.lower + (r.upper - r.lower) / 2;
        upper = r.upper;
        r.upper = lower = m;
    }

    // optional proportional splitting constructor
    TrivialNaturalRange(TrivialNaturalRange& r, oneapi::tbb::proportional_split p) {
        size_t m = r.lower + ((r.upper - r.lower) * p.left()) / (p.left() + p.right());
        if (m == r.lower)
            m++;
        else if (m == r.upper)
            m--;
        upper = r.upper;
        r.upper = lower = m;
    }

    // optional trait that enables proportional split
    static const bool is_splittable_in_proportion = true;
};
