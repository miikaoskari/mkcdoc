#include "mathutil.h"

/**
 * @brief Adds two integers.
 *
 * Performs signed 32-bit addition without overflow checking.
 *
 * @param a First addend.
 * @param b Second addend.
 * @return The sum of a and b.
 */
int mu_add(int a, int b) {
    return a + b;
}

/**
 * @brief Clamps a value to the inclusive range [lo, hi].
 * @param value The value to clamp.
 * @param lo Lower bound.
 * @param hi Upper bound.
 * @return value, or the nearest bound if value is out of range.
 * @see mu_add
 */
double mu_clamp(double value, double lo, double hi) {
    if (value < lo) return lo;
    if (value > hi) return hi;
    return value;
}

/* Not a doc comment, no @tags, should be skipped by the extractor. */
static int internal_helper(int x) {
    return x * 2;
}
