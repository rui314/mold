#include "range_concept.h"

int main() {
    TrivialNaturalRange r;
    r.upper = 1;
    r.lower = 0;
    return int(r.empty());
}
