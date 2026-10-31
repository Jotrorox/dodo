/* C side of the by-value struct ABI fixture: the C compiler supplies the
 * reference classification of every shape the Dodo side passes and returns. */
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
typedef struct { uint64_t w, h, d; } Size;
typedef struct { int32_t a, b; } Pair;
typedef struct { double r, g, b, a; } Color;
typedef struct { float x, y; } Vec2;
typedef struct { int64_t a; double b; } Mixed;
typedef struct { float a, b, c; } Tri;
typedef struct { uint8_t a, b, c; } Bytes3;
typedef struct { Pair p; float f; } Nested;
typedef struct { uint16_t v[3]; bool flag; } Arr;
typedef struct { double x, y, w, h, n, f; } Viewport;
typedef struct { Size origin, size; } Region;
typedef struct { int64_t a, b; } Wide;
typedef struct { double d; float f; } DF;
typedef struct { float f; double d; } FD;

static int failures;
#define CHECK(c) do { if (!(c)) { printf("C check failed: %s (line %d)\n", #c, __LINE__); failures++; } } while (0)

uint64_t take_size(Size s) { CHECK(s.w == 1 && s.h == 2 && s.d == 3); return s.w * 100 + s.h * 10 + s.d; }
Size make_size(uint64_t w, uint64_t h, uint64_t d) { return (Size){w, h, d}; }
int32_t take_pair(Pair p) { CHECK(p.a == -5 && p.b == 7); return p.a + p.b; }
Pair make_pair(int32_t a, int32_t b) { return (Pair){a, b}; }
double take_color(Color c) { CHECK(c.r == 0.25 && c.g == 0.5 && c.b == 0.75 && c.a == 1.0); return c.r + c.g + c.b + c.a; }
Color make_color(double r, double g, double b, double a) { return (Color){r, g, b, a}; }
float take_vec2(Vec2 v) { CHECK(v.x == 1.5f && v.y == -2.5f); return v.x * v.y; }
Vec2 make_vec2(float x, float y) { return (Vec2){x, y}; }
double take_mixed(Mixed m) { CHECK(m.a == -9 && m.b == 2.5); return m.a + m.b; }
Mixed make_mixed(int64_t a, double b) { return (Mixed){a, b}; }
float take_tri(Tri t) { CHECK(t.a == 1 && t.b == 2 && t.c == 3); return t.a + t.b + t.c; }
Tri make_tri(float a, float b, float c) { return (Tri){a, b, c}; }
uint32_t take_bytes3(Bytes3 b) { CHECK(b.a == 1 && b.b == 2 && b.c == 3); return b.a + b.b + b.c; }
Bytes3 make_bytes3(uint8_t a, uint8_t b, uint8_t c) { return (Bytes3){a, b, c}; }
float take_nested(Nested n) { CHECK(n.p.a == 4 && n.p.b == 5 && n.f == 0.5f); return n.p.a + n.p.b + n.f; }
Nested make_nested(int32_t a, int32_t b, float f) { return (Nested){{a, b}, f}; }
uint32_t take_arr(Arr a) { CHECK(a.v[0] == 10 && a.v[1] == 20 && a.v[2] == 30 && a.flag); return a.v[0] + a.v[1] + a.v[2] + a.flag; }
Arr make_arr(uint16_t x, uint16_t y, uint16_t z, bool f) { return (Arr){{x, y, z}, f}; }
double take_viewport(Viewport v) { CHECK(v.x == 1 && v.y == 2 && v.w == 3 && v.h == 4 && v.n == 5 && v.f == 6); return v.x + v.y + v.w + v.h + v.n + v.f; }
Viewport make_viewport(double x) { return (Viewport){x, x + 1, x + 2, x + 3, x + 4, x + 5}; }
uint64_t take_region(Region r) { CHECK(r.origin.w == 1 && r.origin.d == 3 && r.size.w == 4 && r.size.d == 6); return r.origin.w + r.origin.h + r.origin.d + r.size.w + r.size.h + r.size.d; }
Region make_region(uint64_t base) { return (Region){{base, base + 1, base + 2}, {base + 3, base + 4, base + 5}}; }
/* Register pressure: five integers leave one GPR, so Wide goes to the stack on SysV. */
int64_t crowded(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, Wide w, int64_t f) {
    CHECK(a == 1 && b == 2 && c == 3 && d == 4 && e == 5 && w.a == 60 && w.b == 70 && f == 8);
    return a + b + c + d + e + w.a + w.b + f;
}
/* Seven doubles leave one SSE register; Vec2 then Mixed. */
double crowded_floats(double a, double b, double c, double d, double e, double f, double g, Mixed m, Vec2 v, double h) {
    CHECK(a == 1 && b == 2 && c == 3 && d == 4 && e == 5 && f == 6 && g == 7 && m.a == 8 && m.b == 9 && v.x == 10 && v.y == 11 && h == 12);
    return a + b + c + d + e + f + g + m.a + m.b + v.x + v.y + h;
}
double take_df(DF x) { CHECK(x.d == 1.25 && x.f == 2.5f); return x.d + x.f; }
FD make_fd(float f, double d) { return (FD){f, d}; }
Size identity_size(Size s, Pair p) { CHECK(p.a == 1 && p.b == 2); return s; }
uint8_t narrow_after(Bytes3 b, uint8_t n) { CHECK(n == 200); return b.a + n; }

/* Dodo definitions called from C. */
extern Color dodo_scale_color(Color c, double k);
extern Pair dodo_swap_pair(Pair p);
extern Size dodo_bump_size(Size s);
extern Vec2 dodo_vec2(Mixed m, Tri t);
extern Bytes3 dodo_bytes3(Bytes3 b);
extern int64_t dodo_crowded(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, Wide w, int64_t f);
extern Viewport dodo_viewport(Viewport v, Region r);

int32_t run_callbacks(void) {
    Color c = dodo_scale_color((Color){1, 2, 3, 4}, 0.5);
    CHECK(c.r == 0.5 && c.g == 1 && c.b == 1.5 && c.a == 2);
    Pair p = dodo_swap_pair((Pair){3, -4});
    CHECK(p.a == -4 && p.b == 3);
    Size s = dodo_bump_size((Size){10, 20, 30});
    CHECK(s.w == 11 && s.h == 21 && s.d == 31);
    Vec2 v = dodo_vec2((Mixed){3, 0.5}, (Tri){1, 2, 4});
    CHECK(v.x == 3.5f && v.y == 7.0f);
    Bytes3 b = dodo_bytes3((Bytes3){1, 2, 3});
    CHECK(b.a == 3 && b.b == 2 && b.c == 1);
    CHECK(dodo_crowded(1, 2, 3, 4, 5, (Wide){60, 70}, 8) == 153);
    Viewport vp = dodo_viewport((Viewport){1, 2, 3, 4, 5, 6}, (Region){{1, 2, 3}, {4, 5, 6}});
    CHECK(vp.x == 22 && vp.f == 27);
    return failures;
}
/* C function pointers in both directions. */
typedef Mixed (*MakeMixed)(int64_t, double);
MakeMixed mixed_maker(void) { return make_mixed; }
double apply(Color (*scale)(Color, double), Region (*shift)(Region, Size), Vec2 (*combine)(Mixed, Tri)) {
    Color c = scale((Color){1, 2, 3, 4}, 2);
    Region r = shift((Region){{1, 2, 3}, {4, 5, 6}}, (Size){10, 20, 30});
    Vec2 v = combine((Mixed){1, 0.5}, (Tri){1, 1, 1});
    CHECK(c.a == 8 && r.origin.w == 11 && r.size.d == 36 && v.x == 1.5f && v.y == 3);
    return c.r + r.origin.h + v.y;
}
int32_t c_failures(void) { return failures; }
