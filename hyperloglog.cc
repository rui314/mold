// This file implements HyperLogLog algorithm, which estimates
// the number of unique items in a given multiset.
//
// For more info, read
// https://engineering.fb.com/2018/12/13/data-infrastructure/hyperloglog

#include "mold.h"

#include <cmath>

namespace mold::elf {

i64 HyperLogLog::get_cardinality() const {
  double z = 0;
  for (i64 val : buckets)
    z += pow(2, -val);
  return ALPHA * NBUCKETS * NBUCKETS / z;
}

void HyperLogLog::merge(const HyperLogLog &other) {
  for (i64 i = 0; i < NBUCKETS; i++)
    merge_one(i, other.buckets[i]);
}

} // namespace mold::elf
