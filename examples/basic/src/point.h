#ifndef POINT_H
#define POINT_H

/** Maximum coordinate magnitude allowed for any Point. */
#define POINT_MAX_COORD 10000

/** Computes the squared distance between the origin and (x, y). */
#define POINT_DIST2(x, y) ((x) * (x) + (y) * (y))

/** A 2D point with integer coordinates. */
typedef struct {
    int x; /**< X coordinate. */
    int y; /**< Y coordinate. */
} Point;

/** Named color for a rendered point. */
struct NamedColor {
    const char *name;
    unsigned char r, g, b;
};

/** Point rendering style. */
typedef enum {
    POINT_STYLE_DOT,   /**< Rendered as a single pixel. */
    POINT_STYLE_CIRCLE, /**< Rendered as a filled circle. */
    POINT_STYLE_SQUARE = 10 /**< Rendered as a filled square. */
} PointStyle;

/** Callback invoked once per point during a traversal. */
typedef void (*PointVisitor)(const Point *p, void *user_data);

#endif
