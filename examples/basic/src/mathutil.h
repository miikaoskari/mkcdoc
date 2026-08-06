#ifndef MATHUTIL_H
#define MATHUTIL_H

/**
 * @brief Adds two integers.
 * @param a First addend.
 * @param b Second addend.
 * @return The sum of a and b.
 */
int mu_add(int a, int b);

/**
 * @brief Clamps a value to the inclusive range [lo, hi].
 * @param value The value to clamp.
 * @param lo Lower bound.
 * @param hi Upper bound.
 * @return value, or the nearest bound if value is out of range.
 */
double mu_clamp(double value, double lo, double hi);

#endif
