/* Dodo BSD-2-Clause window ABI. Only libraries the operating system ships are
 * used: AppKit through the plain C Objective-C runtime on macOS, user32 and
 * gdi32 on Windows, and on Linux nothing beyond libc, because std/window/x11
 * speaks the X11 protocol itself over the socket opened here.
 *
 * macOS and Windows keep one window per process in static state. Its pixel
 * buffer is 32-bit BGRA, top-down, and replaced only inside
 * dodo_window_next_event when the drawing area changes size. A window opened
 * for the GPU has no buffer and only tracks its size. Events are queued in a
 * ring and handed out one at a time in the std/window/raw Event layout.
 *
 * Error codes match std/window/raw from_code: 0 success, 1 NoDisplay,
 * 2 AlreadyOpen, 3 Closed, 4 InvalidInput, 5 OutOfMemory, 6 Protocol,
 * 7 Unsupported, 8 System. dodo_window_next_event returns them negated.
 *
 * Frame pacing: dodo_window_clock is a monotonic nanosecond clock on every
 * OS. macOS 14 and later report display refreshes with a CADisplayLink as
 * FRAME events; Windows reads the next refresh time from the compositor. */
#if defined(__linux__)
#define _GNU_SOURCE
#endif
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

enum {
    E_NO_DISPLAY = 1, E_ALREADY_OPEN = 2, E_CLOSED = 3, E_INVALID = 4,
    E_MEMORY = 5, E_PROTOCOL = 6, E_UNSUPPORTED = 7, E_SYSTEM = 8
};
enum {
    EV_CLOSE = 1, EV_RESIZE = 2, EV_FOCUS = 3, EV_KEY = 4, EV_TEXT = 5,
    EV_MOVE = 6, EV_BUTTON = 7, EV_LEAVE = 8, EV_SCROLL = 9, EV_FRAME = 10
};
enum { F_PRESSED = 1, F_REPEAT = 2 };
enum { M_SHIFT = 1, M_CONTROL = 2, M_ALT = 4, M_META = 8, M_CAPS = 16 };
/* dodo_window_open flags, and what dodo_window_native reports. */
enum { O_RESIZABLE = 1, O_PIXEL_BUFFER = 2 };
enum { NATIVE_APPKIT = 1, NATIVE_WIN32 = 2 };
#define FOREVER 0xffffffffu

struct dodo_window_event {
    uint32_t kind, code, modifiers, flags;
    int32_t x, y;
};

/* An empty buffer still needs a valid address for a Dodo slice. */
static uint32_t empty_pixels[1];

#if defined(__APPLE__) || defined(_WIN32)
#define QUEUE 256
static struct dodo_window_event queue[QUEUE];
static unsigned queue_head, queue_count;

static void push(uint32_t kind, uint32_t code, uint32_t modifiers, uint32_t flags, int32_t x, int32_t y) {
    /* Coalesce motion so a slow consumer sees the latest position. */
    if (kind == EV_MOVE && queue_count) {
        struct dodo_window_event *last = &queue[(queue_head + queue_count - 1) % QUEUE];
        if (last->kind == EV_MOVE) { last->x = x; last->y = y; last->modifiers = modifiers; return; }
    }
    if (queue_count == QUEUE) return;
    struct dodo_window_event *event = &queue[(queue_head + queue_count) % QUEUE];
    event->kind = kind; event->code = code; event->modifiers = modifiers;
    event->flags = flags; event->x = x; event->y = y;
    queue_count++;
}
static int pop(struct dodo_window_event *event) {
    if (!queue_count) return 0;
    *event = queue[queue_head];
    queue_head = (queue_head + 1) % QUEUE;
    queue_count--;
    return 1;
}
/* UTF-8 text from the keyboard layout becomes one TEXT event per scalar;
 * control characters and the private-use range macOS uses for function keys
 * are not text. */
static void push_text(uint32_t code_point) {
    if (code_point < 0x20 || code_point == 0x7f || (code_point >= 0x80 && code_point < 0xa0)) return;
    if (code_point >= 0xf700 && code_point <= 0xf8ff) return;
    if (code_point > 0x10ffff || (code_point >= 0xd800 && code_point <= 0xdfff)) return;
    push(EV_TEXT, code_point, 0, 0, 0, 0);
}

/* The pixel buffer. Resizing keeps the old picture in the top-left corner.
 * Without one (has_pixels 0) pixels stays NULL and only the size is kept;
 * sized says whether the size is known yet. */
static uint8_t *pixels;
static uint32_t pixel_width, pixel_height;
static int has_pixels, sized;

static int resize_buffer(uint32_t width, uint32_t height) {
    if (!has_pixels) {
        pixel_width = width; pixel_height = height; sized = 1;
        return 0;
    }
    size_t bytes = (size_t)width * height * 4;
    uint8_t *next = calloc(bytes ? bytes : 4, 1);
    if (!next) return E_MEMORY;
    if (pixels) {
        uint32_t rows = height < pixel_height ? height : pixel_height;
        uint32_t columns = width < pixel_width ? width : pixel_width;
        for (uint32_t y = 0; y < rows; ++y)
            memcpy(next + (size_t)y * width * 4, pixels + (size_t)y * pixel_width * 4, (size_t)columns * 4);
        free(pixels);
    }
    pixels = next; pixel_width = width; pixel_height = height; sized = 1;
    return 0;
}
static void free_buffer(void) {
    free(pixels); pixels = NULL; pixel_width = 0; pixel_height = 0; sized = 0;
}

/* A window without a buffer reports its size with an empty one. */
void dodo_window_buffer(intptr_t handle, uint8_t **out, uint32_t *width, uint32_t *height, size_t *stride) {
    if (!handle || !sized) {
        *out = (uint8_t *)empty_pixels; *width = 0; *height = 0; *stride = 0;
        return;
    }
    if (!pixels) {
        *out = (uint8_t *)empty_pixels; *width = pixel_width; *height = pixel_height; *stride = 0;
        return;
    }
    *out = pixels; *width = pixel_width; *height = pixel_height; *stride = (size_t)pixel_width * 4;
}
#endif

/* ------------------------------------------------------------------------ */
#if defined(__APPLE__)
#include <objc/runtime.h>
#include <objc/message.h>
#include <CoreGraphics/CoreGraphics.h>
#include <math.h>
#include <pthread.h>
#include <time.h>

extern void *objc_autoreleasePoolPush(void);
extern void objc_autoreleasePoolPop(void *pool);
extern id const kCAGravityTopLeft;
extern id const kCAFilterNearest;
extern id const NSDefaultRunLoopMode;

/* Typed views of objc_msgSend, one per signature used. */
#define SEND(type) ((type)objc_msgSend)
typedef id (*send_id)(id, SEL);
typedef id (*send_id_id)(id, SEL, id);
typedef void (*send_void)(id, SEL);
typedef void (*send_void_id)(id, SEL, id);
typedef void (*send_void_bool)(id, SEL, BOOL);
typedef void (*send_void_long)(id, SEL, long);
typedef void (*send_void_double)(id, SEL, double);
typedef BOOL (*send_bool)(id, SEL);
typedef BOOL (*send_bool_id)(id, SEL, id);
typedef double (*send_double)(id, SEL);
typedef unsigned short (*send_ushort)(id, SEL);
typedef unsigned long (*send_ulong)(id, SEL);
typedef long (*send_long)(id, SEL);
typedef CGPoint (*send_point)(id, SEL);
typedef CGPoint (*send_point_point_id)(id, SEL, CGPoint, id);
typedef CGSize (*send_size_size)(id, SEL, CGSize);
typedef void (*send_void_size)(id, SEL, CGSize);
typedef const char *(*send_cstring)(id, SEL);

static id cls(const char *name) { return (id)objc_getClass(name); }
static SEL sel(const char *name) { return sel_registerName(name); }
static id msg(id target, const char *name) { return SEND(send_id)(target, sel(name)); }
static void msg_void(id target, const char *name) { SEND(send_void)(target, sel(name)); }
static void msg_void_id(id target, const char *name, id value) { SEND(send_void_id)(target, sel(name), value); }
static void msg_void_bool(id target, const char *name, BOOL value) { SEND(send_void_bool)(target, sel(name), value); }

/* NSRect is returned in memory on x86_64, which needs the _stret entry. */
static CGRect msg_rect(id target, const char *name) {
#if defined(__x86_64__)
    CGRect result;
    ((void (*)(CGRect *, id, SEL))objc_msgSend_stret)(&result, target, sel(name));
    return result;
#else
    return ((CGRect (*)(id, SEL))objc_msgSend)(target, sel(name));
#endif
}

static id app, window, view, delegate;
/* The CAMetalLayer dodo_window_metal_layer made the view's backing layer,
 * retained here until the window closes. */
static id metal_layer;
static CGColorSpaceRef color_space;
static double backing_scale = 1;
static int cursor_hidden, mouse_inside;
static int launched;
/* A CADisplayLink, paused except between request_frame and the refresh it
 * reports, the refresh period it measured, and the App Nap opt-out held
 * while animating. */
static id display_link, activity;
static uint64_t link_period;

static uint32_t modifiers_of(unsigned long flags) {
    uint32_t bits = 0;
    if (flags & (1ul << 17)) bits |= M_SHIFT;
    if (flags & (1ul << 18)) bits |= M_CONTROL;
    if (flags & (1ul << 19)) bits |= M_ALT;
    if (flags & (1ul << 20)) bits |= M_META;
    if (flags & (1ul << 16)) bits |= M_CAPS;
    return bits;
}

/* Generated from the Key order in std/window.dodo: macOS virtual key codes,
 * which name physical positions, to Key ordinals. */
static const uint8_t mac_keys[128] = {
      1,  19,   4,   6,   8,   7,  26,  24,   3,  22,  97,   2,  17,  23,   5,  18,
     25,  20,  28,  29,  30,  31,  33,  32,  43,  36,  34,  42,  35,  27,  45,  15,
     21,  44,   9,  16,  38,  12,  10,  48,  11,  47,  46,  50,  52,  14,  13,  51,
     39,  41,  49,  40,   0,  37, 106, 102, 100,  53, 101,  99, 104, 105, 103,   0,
      0,  95,   0,  81,   0,  83,   0,  79,   0,   0,   0,  80,  84,   0,  82,   0,
      0,  96,  85,  86,  87,  88,  89,  90,  91,  92,   0,  93,  94,   0,   0,   0,
     58,  59,  60,  56,  61,  62,   0,  64,   0,   0,   0,   0,   0,  63,  98,  65,
      0,   0,  69,  70,  71,  72,  57,  73,  55,  74,  54,  76,  75,  77,  78,   0,
};

/* Device-dependent flag bit of each modifier key, so left and right keys
 * report their own presses. */
static unsigned long modifier_bit(unsigned short code) {
    switch (code) {
    case 0x38: return 0x2;    case 0x3c: return 0x4;     /* shift */
    case 0x3b: return 0x1;    case 0x3e: return 0x2000;  /* control */
    case 0x3a: return 0x20;   case 0x3d: return 0x40;    /* option */
    case 0x37: return 0x8;    case 0x36: return 0x10;    /* command */
    case 0x39: return 1ul << 16;                         /* caps lock */
    default: return 0;
    }
}

/* The view's layer is fetched when needed rather than kept: a GPU program
 * may replace it. */
static void update_scale(void) {
    double scale = SEND(send_double)(window, sel("backingScaleFactor"));
    backing_scale = scale > 0 ? scale : 1;
    if (has_pixels) SEND(send_void_double)(msg(view, "layer"), sel("setContentsScale:"), backing_scale);
    if (metal_layer) SEND(send_void_double)(metal_layer, sel("setContentsScale:"), backing_scale);
}

/* Keep the Metal layer's drawables at the drawing area's size in pixels. */
static void size_metal_layer(void) {
    if (!metal_layer) return;
    CGSize size = CGSizeMake(pixel_width ? pixel_width : 1, pixel_height ? pixel_height : 1);
    SEND(send_void_size)(metal_layer, sel("setDrawableSize:"), size);
}

/* Bring the buffer to the content view's size in pixels. */
static int sync_size(int report) {
    CGRect bounds = msg_rect(view, "bounds");
    update_scale();
    double width = floor(bounds.size.width * backing_scale + 0.5);
    double height = floor(bounds.size.height * backing_scale + 0.5);
    uint32_t w = width > 0 ? (uint32_t)width : 0, h = height > 0 ? (uint32_t)height : 0;
    if (sized && w == pixel_width && h == pixel_height) return 0;
    int code = resize_buffer(w, h);
    if (code) return code;
    size_metal_layer();
    if (report) push(EV_RESIZE, 0, 0, 0, (int32_t)w, (int32_t)h);
    return 0;
}

/* Closing is the program's decision, so the close button only reports. */
static BOOL should_close(id self, SEL command, id sender) {
    (void)self; (void)command; (void)sender;
    push(EV_CLOSE, 0, 0, 0, 0, 0);
    return NO;
}
static void quit_requested(id self, SEL command, id sender) {
    (void)self; (void)command; (void)sender;
    push(EV_CLOSE, 0, 0, 0, 0, 0);
}
static void became_key(id self, SEL command, id note) {
    (void)self; (void)command; (void)note;
    push(EV_FOCUS, 0, 0, F_PRESSED, 0, 0);
}
static void resigned_key(id self, SEL command, id note) {
    (void)self; (void)command; (void)note;
    push(EV_FOCUS, 0, 0, 0, 0, 0);
}
static void did_resize(id self, SEL command, id note) {
    (void)self; (void)command; (void)note;
    if (window) sync_size(1);
}
/* Marks frame_ready's application-defined wake-up event in data1. */
#define FRAME_WAKE 0x646f646fl

/* The display refreshed: report one FRAME and pause until the next request. */
static void frame_ready(id self, SEL command, id link) {
    (void)self; (void)command;
    double last = SEND(send_double)(link, sel("timestamp"));
    double next = SEND(send_double)(link, sel("targetTimestamp"));
    if (next > last && next - last < 0.1) link_period = (uint64_t)((next - last) * 1e9 + 0.5);
    msg_void_bool(link, "setPaused:", YES);
    push(EV_FRAME, 0, 0, 0, 0, 0);
    /* nextEventMatchingMask: returns only for events, so post an
     * application-defined one to end its wait. */
    id wake = SEND(id (*)(id, SEL, unsigned long, CGPoint, unsigned long, double, long, id, short, long, long))(
        cls("NSEvent"), sel("otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"),
        15ul, CGPointZero, 0ul, 0.0, 0l, nil, (short)0, FRAME_WAKE, 0l);
    if (wake) SEND(void (*)(id, SEL, id, BOOL))(app, sel("postEvent:atStart:"), wake, NO);
}

static Class delegate_class(void) {
    Class existing = objc_getClass("DodoWindowDelegate");
    if (existing) return existing;
    Class created = objc_allocateClassPair(objc_getClass("NSObject"), "DodoWindowDelegate", 0);
    class_addMethod(created, sel("windowShouldClose:"), (IMP)should_close, "c@:@");
    class_addMethod(created, sel("windowDidBecomeKey:"), (IMP)became_key, "v@:@");
    class_addMethod(created, sel("windowDidResignKey:"), (IMP)resigned_key, "v@:@");
    class_addMethod(created, sel("windowDidResize:"), (IMP)did_resize, "v@:@");
    class_addMethod(created, sel("windowDidChangeBackingProperties:"), (IMP)did_resize, "v@:@");
    class_addMethod(created, sel("dodoQuit:"), (IMP)quit_requested, "v@:@");
    class_addMethod(created, sel("dodoFrame:"), (IMP)frame_ready, "v@:@");
    objc_registerClassPair(created);
    return created;
}

static id string_from(const uint8_t *bytes, size_t length) {
    id text = SEND(id (*)(id, SEL, const void *, unsigned long, unsigned long))(
        msg(cls("NSString"), "alloc"), sel("initWithBytes:length:encoding:"), bytes, (unsigned long)length, 4ul);
    return text ? text : SEND(id (*)(id, SEL, const char *))(msg(cls("NSString"), "alloc"), sel("initWithUTF8String:"), "");
}

/* An application menu whose Quit item (Command-Q) reports Close. */
static void install_menu(void) {
    id bar = msg(msg(cls("NSMenu"), "alloc"), "init");
    id app_item = msg(msg(cls("NSMenuItem"), "alloc"), "init");
    msg_void_id(bar, "addItem:", app_item);
    id menu = msg(msg(cls("NSMenu"), "alloc"), "init");
    id name = msg(msg(cls("NSProcessInfo"), "processInfo"), "processName");
    id prefix = SEND(id (*)(id, SEL, const char *))(cls("NSString"), sel("stringWithUTF8String:"), "Quit ");
    id title = SEND(send_id_id)(prefix, sel("stringByAppendingString:"), name);
    id key = SEND(id (*)(id, SEL, const char *))(cls("NSString"), sel("stringWithUTF8String:"), "q");
    id quit = SEND(id (*)(id, SEL, id, SEL, id))(msg(cls("NSMenuItem"), "alloc"),
        sel("initWithTitle:action:keyEquivalent:"), title, sel("dodoQuit:"), key);
    msg_void_id(quit, "setTarget:", delegate);
    msg_void_id(menu, "addItem:", quit);
    msg_void_id(app_item, "setSubmenu:", menu);
    msg_void_id(app, "setMainMenu:", bar);
    msg_void(quit, "release"); msg_void(menu, "release");
    msg_void(app_item, "release"); msg_void(bar, "release");
}

static BOOL responds(id target, const char *name) {
    return SEND(BOOL (*)(id, SEL, SEL))(target, sel("respondsToSelector:"), sel(name));
}

/* NSView display links exist from macOS 14; older systems pace by timer. */
static void create_display_link(void) {
    if (!responds(view, "displayLinkWithTarget:selector:")) return;
    id link = SEND(id (*)(id, SEL, id, SEL))(view, sel("displayLinkWithTarget:selector:"), delegate, sel("dodoFrame:"));
    if (!link) return;
    display_link = msg(link, "retain");
    msg_void_bool(display_link, "setPaused:", YES);
    SEND(void (*)(id, SEL, id, id))(display_link, sel("addToRunLoop:forMode:"),
        msg(cls("NSRunLoop"), "mainRunLoop"), NSDefaultRunLoopMode);
}

static void end_activity(void) {
    if (!activity) return;
    msg_void_id(msg(cls("NSProcessInfo"), "processInfo"), "endActivity:", activity);
    msg_void(activity, "release");
    activity = nil;
}

static void release_all(void) {
    if (display_link) {
        msg_void(display_link, "invalidate");
        msg_void(display_link, "release");
        display_link = nil;
    }
    link_period = 0;
    end_activity();
    if (window) {
        msg_void_id(window, "setDelegate:", nil);
        msg_void_id(window, "orderOut:", nil);
        msg_void(window, "close");
        msg_void(window, "release");
    }
    if (metal_layer) msg_void(metal_layer, "release");
    /* The menu's Quit item targets the delegate released below. */
    if (app && delegate) msg_void_id(app, "setMainMenu:", nil);
    if (delegate) msg_void(delegate, "release");
    if (color_space) CGColorSpaceRelease(color_space);
    if (cursor_hidden) msg_void(cls("NSCursor"), "unhide");
    window = view = delegate = metal_layer = nil;
    color_space = NULL; cursor_hidden = 0; mouse_inside = 0; has_pixels = 0;
    queue_head = queue_count = 0;
    free_buffer();
}

int32_t dodo_window_open(const uint8_t *title, size_t title_length, uint32_t width, uint32_t height, uint32_t flags, intptr_t *handle) {
    if (window) return E_ALREADY_OPEN;
    if (!pthread_main_np()) return E_UNSUPPORTED;
    int resizable = (flags & O_RESIZABLE) != 0;
    has_pixels = (flags & O_PIXEL_BUFFER) != 0;
    void *pool = objc_autoreleasePoolPush();
    app = msg(cls("NSApplication"), "sharedApplication");
    if (!app) { objc_autoreleasePoolPop(pool); return E_NO_DISPLAY; }
    if (!launched) {
        SEND(send_void_long)(app, sel("setActivationPolicy:"), 0); /* regular */
        msg_void(app, "finishLaunching");
        launched = 1;
    }
    delegate = msg(msg((id)delegate_class(), "alloc"), "init");
    install_menu();
    unsigned long style = 1 | 2 | 4 | (resizable ? 8 : 0); /* titled, closable, miniaturizable, resizable */
    CGRect frame = CGRectMake(0, 0, width, height);
    window = SEND(id (*)(id, SEL, CGRect, unsigned long, unsigned long, BOOL))(msg(cls("NSWindow"), "alloc"),
        sel("initWithContentRect:styleMask:backing:defer:"), frame, style, 2ul, NO);
    if (!window) { release_all(); objc_autoreleasePoolPop(pool); return E_SYSTEM; }
    msg_void_bool(window, "setReleasedWhenClosed:", NO);
    msg_void_bool(window, "setAcceptsMouseMovedEvents:", YES);
    msg_void_id(window, "setDelegate:", delegate);
    id text = string_from(title, title_length);
    msg_void_id(window, "setTitle:", text);
    msg_void(text, "release");
    view = msg(window, "contentView");
    /* The pixel buffer is shown as the contents of the view's layer. A GPU
     * window leaves the view alone for NSOpenGLContext or a Metal layer. */
    if (has_pixels) {
        msg_void_bool(view, "setWantsLayer:", YES);
        id layer = msg(view, "layer");
        msg_void_id(layer, "setContentsGravity:", kCAGravityTopLeft);
        msg_void_id(layer, "setMagnificationFilter:", kCAFilterNearest);
    }
    create_display_link();
    /* Moves and exits over the drawing area, focused or not. */
    id area = SEND(id (*)(id, SEL, CGRect, unsigned long, id, id))(msg(cls("NSTrackingArea"), "alloc"),
        sel("initWithRect:options:owner:userInfo:"), CGRectZero, 0x01ul | 0x02ul | 0x80ul | 0x200ul, view, nil);
    msg_void_id(view, "addTrackingArea:", area);
    msg_void(area, "release");
    color_space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    msg_void(window, "center");
    msg_void_id(window, "makeKeyAndOrderFront:", nil);
    msg_void_bool(app, "activateIgnoringOtherApps:", YES);
    int code = color_space ? sync_size(0) : E_SYSTEM;
    if (code) { release_all(); objc_autoreleasePoolPop(pool); return code; }
    objc_autoreleasePoolPop(pool);
    *handle = 1;
    return 0;
}

void dodo_window_close(intptr_t handle) {
    if (!handle || !window) return;
    void *pool = objc_autoreleasePoolPush();
    release_all();
    objc_autoreleasePoolPop(pool);
}

static void decode_utf8(const char *text) {
    const unsigned char *s = (const unsigned char *)text;
    while (s && *s) {
        uint32_t c = *s++;
        int extra = c >= 0xf0 ? 3 : c >= 0xe0 ? 2 : c >= 0xc0 ? 1 : 0;
        if (extra) c &= 0x3f >> extra;
        for (; extra && (*s & 0xc0) == 0x80; --extra) c = (c << 6) | (*s++ & 0x3f);
        if (!extra) push_text(c);
    }
}

static void mouse_position(id event, int32_t *x, int32_t *y) {
    CGPoint location = SEND(send_point)(event, sel("locationInWindow"));
    CGPoint local = SEND(send_point_point_id)(view, sel("convertPoint:fromView:"), location, nil);
    CGRect bounds = msg_rect(view, "bounds");
    *x = (int32_t)floor(local.x * backing_scale);
    *y = (int32_t)floor((bounds.size.height - local.y) * backing_scale);
}

/* Translates an event into the queue. Returns whether AppKit should also
 * see it: keys go only to menu shortcuts, which avoids the beep AppKit plays
 * for keys no responder handles. */
static int translate(id event) {
    long type = SEND(send_long)(event, sel("type"));
    unsigned long flags = SEND(send_ulong)(event, sel("modifierFlags"));
    uint32_t modifiers = modifiers_of(flags);
    int ours = msg(event, "window") == window;
    int32_t x = 0, y = 0;
    switch (type) {
    case 15: /* application-defined, such as frame_ready's wake-up */
        return SEND(send_long)(event, sel("data1")) != FRAME_WAKE;
    case 10: case 11: { /* key down, key up */
        unsigned short code = SEND(send_ushort)(event, sel("keyCode"));
        uint32_t key = code < 128 ? mac_keys[code] : 0;
        if (type == 10 && (modifiers & M_META)) {
            id menu = msg(app, "mainMenu");
            if (menu && SEND(send_bool_id)(menu, sel("performKeyEquivalent:"), event)) return 0;
        }
        uint32_t state = type == 10 ? F_PRESSED : 0;
        if (type == 10 && SEND(send_bool)(event, sel("isARepeat"))) state |= F_REPEAT;
        push(EV_KEY, key, modifiers, state, 0, 0);
        if (type == 10 && !(modifiers & (M_META | M_CONTROL)))
            decode_utf8(SEND(send_cstring)(msg(event, "characters"), sel("UTF8String")));
        return 0;
    }
    case 12: { /* modifier flags changed */
        unsigned short code = SEND(send_ushort)(event, sel("keyCode"));
        unsigned long bit = modifier_bit(code);
        if (bit && code < 128) push(EV_KEY, mac_keys[code], modifiers, (flags & bit) ? F_PRESSED : 0, 0, 0);
        return 1;
    }
    case 1: case 2: case 3: case 4: case 25: case 26: { /* mouse buttons */
        if (!ours) return 1;
        mouse_position(event, &x, &y);
        long number = SEND(send_long)(event, sel("buttonNumber"));
        uint32_t button = number == 0 ? 0 : number == 1 ? 2 : number == 2 ? 1 : number == 3 ? 3 : number == 4 ? 4 : 99;
        int down = type == 1 || type == 3 || type == 25;
        /* Clicks on the title bar move the window; only the drawing area counts. */
        CGRect bounds = msg_rect(view, "bounds");
        if (down && (y < 0 || x < 0 || x >= (int32_t)(bounds.size.width * backing_scale))) return 1;
        if (button != 99) push(EV_BUTTON, button, modifiers, down ? F_PRESSED : 0, x, y);
        return 1;
    }
    case 5: case 6: case 7: case 27: { /* moved, dragged */
        if (!ours) return 1;
        mouse_position(event, &x, &y);
        CGRect bounds = msg_rect(view, "bounds");
        int inside = x >= 0 && y >= 0 && x < (int32_t)(bounds.size.width * backing_scale)
            && y < (int32_t)(bounds.size.height * backing_scale);
        if (inside || type != 5) { mouse_inside = 1; push(EV_MOVE, 0, modifiers, 0, x, y); }
        else if (mouse_inside) { mouse_inside = 0; push(EV_LEAVE, 0, 0, 0, 0, 0); }
        return 1;
    }
    case 9: /* exited */
        if (mouse_inside) { mouse_inside = 0; push(EV_LEAVE, 0, 0, 0, 0, 0); }
        return 1;
    case 22: { /* scroll wheel */
        if (!ours) return 1;
        double dx = SEND(send_double)(event, sel("scrollingDeltaX"));
        double dy = SEND(send_double)(event, sel("scrollingDeltaY"));
        /* Precise deltas are points; about ten make one notch. */
        double unit = SEND(send_bool)(event, sel("hasPreciseScrollingDeltas")) ? 12 : 120;
        int32_t sx = (int32_t)lround(-dx * unit), sy = (int32_t)lround(-dy * unit);
        if (sx || sy) push(EV_SCROLL, 0, modifiers, 0, sx, sy);
        return 1;
    }
    default:
        return 1;
    }
}

int32_t dodo_window_next_event(intptr_t handle, uint32_t milliseconds, struct dodo_window_event *event) {
    if (!handle || !window) return -E_CLOSED;
    if (pop(event)) return 1;
    void *pool = objc_autoreleasePoolPush();
    id until = milliseconds == 0 ? msg(cls("NSDate"), "distantPast")
        : milliseconds == FOREVER ? msg(cls("NSDate"), "distantFuture")
        : SEND(id (*)(id, SEL, double))(cls("NSDate"), sel("dateWithTimeIntervalSinceNow:"), milliseconds / 1000.0);
    int found = 0;
    for (;;) {
        id next = SEND(id (*)(id, SEL, unsigned long long, id, id, BOOL))(app,
            sel("nextEventMatchingMask:untilDate:inMode:dequeue:"), ~0ull, until, NSDefaultRunLoopMode, YES);
        if (!next) break;
        if (translate(next)) msg_void_id(app, "sendEvent:", next);
        if (window) sync_size(1);
        if (queue_count) break;
    }
    if (window) msg_void(app, "updateWindows");
    found = pop(event);
    objc_autoreleasePoolPop(pool);
    return found;
}

int32_t dodo_window_present(intptr_t handle, int32_t x, int32_t y, uint32_t width, uint32_t height) {
    (void)x; (void)y; (void)width; (void)height;
    if (!handle || !window) return E_CLOSED;
    if (!pixels) return E_UNSUPPORTED;
    if (!pixel_width || !pixel_height) return 0;
    void *pool = objc_autoreleasePoolPush();
    /* The image takes a copy, so the program can keep drawing at once. */
    size_t stride = (size_t)pixel_width * 4;
    CFDataRef data = CFDataCreate(NULL, pixels, (CFIndex)(stride * pixel_height));
    CGDataProviderRef provider = data ? CGDataProviderCreateWithCFData(data) : NULL;
    CGImageRef image = provider ? CGImageCreate(pixel_width, pixel_height, 8, 32, stride, color_space,
        kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst, provider, NULL, false, kCGRenderingIntentDefault) : NULL;
    int code = image ? 0 : E_MEMORY;
    if (image) {
        id transaction = cls("CATransaction");
        msg_void(transaction, "begin");
        msg_void_bool(transaction, "setDisableActions:", YES);
        msg_void_id(msg(view, "layer"), "setContents:", (id)image);
        msg_void(transaction, "commit");
        msg_void(transaction, "flush");
        CGImageRelease(image);
    }
    if (provider) CGDataProviderRelease(provider);
    if (data) CFRelease(data);
    objc_autoreleasePoolPop(pool);
    return code;
}

int32_t dodo_window_set_title(intptr_t handle, const uint8_t *title, size_t title_length) {
    if (!handle || !window) return E_CLOSED;
    void *pool = objc_autoreleasePoolPush();
    id text = string_from(title, title_length);
    msg_void_id(window, "setTitle:", text);
    msg_void(text, "release");
    objc_autoreleasePoolPop(pool);
    return 0;
}

int32_t dodo_window_set_cursor_visible(intptr_t handle, uint32_t visible) {
    if (!handle || !window) return E_CLOSED;
    if (visible && cursor_hidden) { msg_void(cls("NSCursor"), "unhide"); cursor_hidden = 0; }
    if (!visible && !cursor_hidden) { msg_void(cls("NSCursor"), "hide"); cursor_hidden = 1; }
    return 0;
}

uint32_t dodo_window_scale_percent(intptr_t handle) {
    if (!handle || !window) return 100;
    return (uint32_t)lround(backing_scale * 100);
}

/* mach_absolute_time in nanoseconds, the clock CADisplayLink uses. */
uint64_t dodo_window_clock(void) {
    return clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
}

uint64_t dodo_window_frame_period(intptr_t handle) {
    if (!handle || !window) return 0;
    if (link_period) return link_period;
    id screen = msg(window, "screen");
    if (screen && responds(screen, "maximumFramesPerSecond")) {
        long rate = SEND(send_long)(screen, sel("maximumFramesPerSecond"));
        if (rate > 0) return (UINT64_C(1000000000) + (uint64_t)rate / 2) / (uint64_t)rate;
    }
    return 0;
}

/* The display link reports refreshes as they happen instead. */
uint64_t dodo_window_next_vblank(intptr_t handle, uint64_t now) {
    (void)handle; (void)now;
    return 0;
}

int32_t dodo_window_request_frame(intptr_t handle) {
    if (!handle || !window) return -E_CLOSED;
    if (!display_link) return 0;
    msg_void_bool(display_link, "setPaused:", NO);
    return 1;
}

int32_t dodo_window_native(intptr_t handle, uintptr_t *first, uintptr_t *second) {
    if (!handle || !window) return -E_CLOSED;
    *first = (uintptr_t)window; *second = (uintptr_t)view;
    return NATIVE_APPKIT;
}

/* A layer-hosting view: the CAMetalLayer is set before wantsLayer. */
int32_t dodo_window_metal_layer(intptr_t handle, uintptr_t *out) {
    if (!handle || !window) return E_CLOSED;
    if (has_pixels) return E_UNSUPPORTED;
    if (!metal_layer) {
        id type = cls("CAMetalLayer");
        if (!type) return E_UNSUPPORTED;
        void *pool = objc_autoreleasePoolPush();
        id created = msg(msg(type, "alloc"), "init");
        if (!created) { objc_autoreleasePoolPop(pool); return E_SYSTEM; }
        metal_layer = created;
        msg_void_id(view, "setLayer:", metal_layer);
        msg_void_bool(view, "setWantsLayer:", YES);
        update_scale();
        size_metal_layer();
        objc_autoreleasePoolPop(pool);
    }
    *out = (uintptr_t)metal_layer;
    return 0;
}

void dodo_window_set_animating(intptr_t handle, uint32_t animating) {
    if (!handle || !window) return;
    if (!animating) { end_activity(); return; }
    if (activity) return;
    void *pool = objc_autoreleasePoolPush();
    static const char reason[] = "Animating a window";
    id text = string_from((const uint8_t *)reason, sizeof(reason) - 1);
    /* NSActivityUserInitiatedAllowingIdleSystemSleep: no App Nap, but the
     * display and system may still sleep when idle. */
    id token = SEND(id (*)(id, SEL, unsigned long long, id))(msg(cls("NSProcessInfo"), "processInfo"),
        sel("beginActivityWithOptions:reason:"), 0x00FFFFFFull & ~(1ull << 20), text);
    msg_void(text, "release");
    if (token) activity = msg(token, "retain");
    objc_autoreleasePoolPop(pool);
}
#endif

/* ------------------------------------------------------------------------ */
#if defined(__linux__)
/* X11 transport. std/window/x11 builds and parses every request; this part
 * only finds the server, connects and moves bytes. */
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <poll.h>
#include <stdatomic.h>
#include <stdio.h>
#include <time.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <netinet/in.h>
#include <netinet/tcp.h>

static atomic_int claimed;

int32_t dodo_window_claim(void) {
    int expected = 0;
    return atomic_compare_exchange_strong(&claimed, &expected, 1) ? 0 : E_ALREADY_OPEN;
}
void dodo_window_release(void) { atomic_store(&claimed, 0); }

uint8_t *dodo_window_alloc(size_t bytes) { return calloc(bytes ? bytes : 4, 1); }
void dodo_window_free(uint8_t *memory) { free(memory); }
uint8_t *dodo_window_empty(void) { return (uint8_t *)empty_pixels; }

static int connect_unix(const char *path, int abstract) {
    struct sockaddr_un address;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    size_t length = strlen(path);
    if (length + 2 > sizeof(address.sun_path)) return -1;
    memcpy(address.sun_path + (abstract ? 1 : 0), path, length);
    socklen_t size = (socklen_t)(offsetof(struct sockaddr_un, sun_path) + length + (abstract ? 1 : 0));
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return -1;
    if (connect(fd, (struct sockaddr *)&address, size) != 0) { close(fd); return -1; }
    return fd;
}

static int connect_tcp(const char *host, int display) {
    char port[16];
    snprintf(port, sizeof(port), "%d", 6000 + display);
    struct addrinfo hints, *found = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC; hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(host, port, &hints, &found) != 0) return -1;
    int fd = -1;
    for (struct addrinfo *entry = found; entry; entry = entry->ai_next) {
        fd = socket(entry->ai_family, entry->ai_socktype | SOCK_CLOEXEC, entry->ai_protocol);
        if (fd < 0) continue;
        if (connect(fd, entry->ai_addr, entry->ai_addrlen) == 0) {
            int one = 1;
            setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
            break;
        }
        close(fd); fd = -1;
    }
    freeaddrinfo(found);
    return fd;
}

static int read_u16(FILE *file, unsigned *value) {
    int high = fgetc(file), low = fgetc(file);
    if (high == EOF || low == EOF) return 0;
    *value = ((unsigned)high << 8) | (unsigned)low;
    return 1;
}
static int read_field(FILE *file, char *out, size_t capacity, size_t *length) {
    unsigned size;
    if (!read_u16(file, &size)) return 0;
    char discard[1];
    for (unsigned i = 0; i < size; ++i) {
        int c = fgetc(file);
        if (c == EOF) return 0;
        if (i < capacity) out[i] = (char)c; else discard[0] = (char)c;
    }
    (void)discard;
    *length = size < capacity ? size : capacity;
    return size <= capacity;
}

/* Finds the MIT-MAGIC-COOKIE-1 for the display in the Xauthority file:
 * entries for this host (family Local), any host (Wild), or for a TCP
 * display any network address. No file or no entry means no authorization,
 * which servers such as Xvfb accept by default. */
static size_t find_cookie(int display, int tcp, uint8_t *cookie, size_t capacity) {
    char path[4096];
    const char *file_name = getenv("XAUTHORITY");
    if (!file_name || !*file_name) {
        const char *home = getenv("HOME");
        if (!home) return 0;
        snprintf(path, sizeof(path), "%s/.Xauthority", home);
        file_name = path;
    }
    FILE *file = fopen(file_name, "rbe");
    if (!file) return 0;
    char host[256] = {0};
    gethostname(host, sizeof(host) - 1);
    char number[16];
    snprintf(number, sizeof(number), "%d", display);
    size_t found = 0;
    for (;;) {
        unsigned family;
        char address[256], entry_number[32], name[64], data[256];
        size_t address_length, number_length, name_length, data_length;
        if (!read_u16(file, &family)) break;
        int complete = read_field(file, address, sizeof(address), &address_length);
        complete &= read_field(file, entry_number, sizeof(entry_number), &number_length);
        complete &= read_field(file, name, sizeof(name), &name_length);
        complete &= read_field(file, data, sizeof(data), &data_length);
        if (!complete) { if (feof(file)) break; continue; }
        int host_matches = family == 65535
            || (family == 256 && address_length == strlen(host) && memcmp(address, host, address_length) == 0)
            || (tcp && (family == 0 || family == 6));
        if (!host_matches) continue;
        if (number_length != strlen(number) || memcmp(entry_number, number, number_length) != 0) continue;
        if (name_length != 18 || memcmp(name, "MIT-MAGIC-COOKIE-1", 18) != 0) continue;
        if (data_length > capacity) continue;
        memcpy(cookie, data, data_length);
        found = data_length;
        break;
    }
    fclose(file);
    return found;
}

/* Connects to the display named by DISPLAY: ":N[.S]" or "unix:N" over the
 * local socket (the abstract one first), "host:N" over TCP. Writes the
 * socket, the screen number and the cookie, whose length is the result
 * (0 for none); negative results are negated error codes. */
int32_t dodo_window_x11_connect(int32_t *socket_out, uint32_t *screen_out, uint8_t *cookie, size_t capacity) {
    const char *name = getenv("DISPLAY");
    if (!name || !*name) return -E_NO_DISPLAY;
    const char *colon = strrchr(name, ':');
    if (!colon || colon[1] < '0' || colon[1] > '9') return -E_NO_DISPLAY;
    char *end;
    long display = strtol(colon + 1, &end, 10);
    long screen = 0;
    if (*end == '.') screen = strtol(end + 1, NULL, 10);
    if (display < 0 || display > 65535 - 6000 || screen < 0) return -E_NO_DISPLAY;
    char host[256];
    size_t host_length = (size_t)(colon - name);
    if (host_length >= sizeof(host)) return -E_NO_DISPLAY;
    memcpy(host, name, host_length);
    host[host_length] = 0;
    int tcp = host_length && strcmp(host, "unix") != 0 && host[0] != '/';
    int fd;
    if (tcp) {
        fd = connect_tcp(host, (int)display);
    } else {
        char path[64];
        snprintf(path, sizeof(path), "/tmp/.X11-unix/X%ld", display);
        fd = connect_unix(path, 1);
        if (fd < 0) fd = connect_unix(path, 0);
    }
    if (fd < 0) return -E_NO_DISPLAY;
    *socket_out = fd;
    *screen_out = (uint32_t)screen;
    return (int32_t)find_cookie((int)display, tcp, cookie, capacity);
}

/* Bytes read (at least one), 0 when the server closed the connection, or a
 * negated error code. */
int64_t dodo_window_x11_read(int32_t fd, uint8_t *buffer, size_t length) {
    for (;;) {
        ssize_t count = recv(fd, buffer, length, 0);
        if (count >= 0) return count;
        if (errno != EINTR) return -E_CLOSED;
    }
}

int32_t dodo_window_x11_write(int32_t fd, const uint8_t *data, size_t length) {
    while (length) {
        ssize_t count = send(fd, data, length, MSG_NOSIGNAL);
        if (count < 0) {
            if (errno == EINTR) continue;
            return E_CLOSED;
        }
        data += count; length -= (size_t)count;
    }
    return 0;
}

/* 1 when readable within the time, 0 on timeout, or a negated error code. */
int32_t dodo_window_x11_wait(int32_t fd, uint32_t milliseconds) {
    struct pollfd entry = { .fd = fd, .events = POLLIN };
    int timeout = milliseconds == FOREVER ? -1 : milliseconds > 0x7fffffff ? 0x7fffffff : (int)milliseconds;
    for (;;) {
        int ready = poll(&entry, 1, timeout);
        if (ready >= 0) return ready > 0;
        if (errno != EINTR) return -E_SYSTEM;
    }
}

void dodo_window_x11_close(int32_t fd) { close(fd); }

/* CLOCK_MONOTONIC in nanoseconds, the clock of Present's timestamps. */
uint64_t dodo_window_clock(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now)) return 0;
    return (uint64_t)now.tv_sec * UINT64_C(1000000000) + (uint64_t)now.tv_nsec;
}
#endif

/* ------------------------------------------------------------------------ */
#if defined(_WIN32)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <dwmapi.h>

#ifndef CREATE_WAITABLE_TIMER_HIGH_RESOLUTION
#define CREATE_WAITABLE_TIMER_HIGH_RESOLUTION 0x2
#endif

static HWND hwnd;
/* Waits shorter than the 15.6 ms system tick, for frame pacing. */
static HANDLE wait_timer;
static int cursor_hidden, mouse_tracking, buttons_down;
static UINT dpi = 96;
static WCHAR high_surrogate;

/* Generated from the Key order in std/window.dodo: set-1 scan codes, with
 * 0x80 added for extended (E0) keys, to Key ordinals. */
static const uint8_t scan_keys[256] = {
      0,  37,  28,  29,  30,  31,  32,  33,  34,  35,  36,  27,  42,  43,  40,  39,
     17,  23,   5,  18,  20,  25,  21,   9,  15,  16,  44,  45,  38,  99,   1,  19,
      4,   6,   7,   8,  10,  11,  12,  47,  48,  49, 100,  46,  26,  24,   3,  22,
      2,  14,  13,  50,  51,  52, 104,  81, 101,  41,  53,  54,  55,  56,  57,  58,
     59,  60,  61,  62,  63,  68,  67,  92,  93,  94,  82,  89,  90,  91,  83,  86,
     87,  88,  85,  95,   0,   0,  97,  64,  65,  96,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,  84, 103,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,  80,   0,  66, 105,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,  79,   0,  70,  78,  71,   0,  76,   0,  75,   0,  73,
     77,  74,  69,  72,   0,   0,   0,   0,   0,   0,   0, 102, 106,  98,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
      0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
};
enum { KEY_PAUSE = 68, KEY_NUM_LOCK = 79 };

static uint32_t current_modifiers(void) {
    uint32_t bits = 0;
    if (GetKeyState(VK_SHIFT) & 0x8000) bits |= M_SHIFT;
    if (GetKeyState(VK_CONTROL) & 0x8000) bits |= M_CONTROL;
    if (GetKeyState(VK_MENU) & 0x8000) bits |= M_ALT;
    if ((GetKeyState(VK_LWIN) | GetKeyState(VK_RWIN)) & 0x8000) bits |= M_META;
    if (GetKeyState(VK_CAPITAL) & 1) bits |= M_CAPS;
    return bits;
}

/* Optional user32 functions of Windows 10, looked up so older systems and
 * older import libraries still work. */
static UINT window_dpi(void) {
    typedef UINT (WINAPI *dpi_for_window)(HWND);
    static dpi_for_window lookup;
    static int resolved;
    if (!resolved) {
        lookup = (dpi_for_window)(void (*)(void))GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetDpiForWindow");
        resolved = 1;
    }
    UINT value = lookup && hwnd ? lookup(hwnd) : 96;
    return value ? value : 96;
}
static void become_dpi_aware(void) {
    typedef BOOL (WINAPI *set_context)(HANDLE);
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    set_context set = (set_context)(void (*)(void))GetProcAddress(user32, "SetProcessDpiAwarenessContext");
    /* DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 */
    if (!set || !set((HANDLE)(intptr_t)-4)) SetProcessDPIAware();
}

static WCHAR *wide_from(const uint8_t *text, size_t length) {
    if (length > 0x7fffffff) return NULL;
    int count = length ? MultiByteToWideChar(CP_UTF8, 0, (const char *)text, (int)length, NULL, 0) : 0;
    WCHAR *wide = calloc((size_t)count + 1, sizeof(WCHAR));
    if (wide && count) MultiByteToWideChar(CP_UTF8, 0, (const char *)text, (int)length, wide, count);
    return wide;
}

static void blit(HDC dc, int32_t x, int32_t y, uint32_t width, uint32_t height) {
    /* A top-down DIB starting at row y avoids the ambiguous source origin
     * of partial top-down transfers. */
    BITMAPINFO info;
    memset(&info, 0, sizeof(info));
    info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = (LONG)pixel_width;
    info.bmiHeader.biHeight = -(LONG)height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    StretchDIBits(dc, x, y, (int)width, (int)height, x, 0, (int)width, (int)height,
        pixels + (size_t)y * pixel_width * 4, &info, DIB_RGB_COLORS, SRCCOPY);
}

static void button(UINT message, WPARAM wparam, LPARAM lparam) {
    uint32_t code, down;
    switch (message) {
    case WM_LBUTTONDOWN: code = 0; down = 1; break;
    case WM_LBUTTONUP: code = 0; down = 0; break;
    case WM_MBUTTONDOWN: code = 1; down = 1; break;
    case WM_MBUTTONUP: code = 1; down = 0; break;
    case WM_RBUTTONDOWN: code = 2; down = 1; break;
    case WM_RBUTTONUP: code = 2; down = 0; break;
    case WM_XBUTTONDOWN: code = HIWORD(wparam) == XBUTTON1 ? 3 : 4; down = 1; break;
    default: code = HIWORD(wparam) == XBUTTON1 ? 3 : 4; down = 0; break;
    }
    /* Keep receiving the release when it happens outside the window. */
    if (down) { if (!buttons_down++) SetCapture(hwnd); }
    else if (buttons_down && !--buttons_down) ReleaseCapture();
    push(EV_BUTTON, code, current_modifiers(), down ? F_PRESSED : 0,
        (int32_t)(short)LOWORD(lparam), (int32_t)(short)HIWORD(lparam));
}

static LRESULT CALLBACK procedure(HWND window, UINT message, WPARAM wparam, LPARAM lparam) {
    switch (message) {
    case WM_CLOSE:
        push(EV_CLOSE, 0, 0, 0, 0, 0);
        return 0;
    case WM_SIZE:
        if (wparam != SIZE_MINIMIZED) {
            uint32_t width = LOWORD(lparam), height = HIWORD(lparam);
            if (width != pixel_width || height != pixel_height) {
                if (resize_buffer(width, height) == 0) push(EV_RESIZE, 0, 0, 0, (int32_t)width, (int32_t)height);
            }
        }
        return 0;
    case WM_ERASEBKGND:
        return 1;
    case WM_PAINT: {
        PAINTSTRUCT paint;
        HDC dc = BeginPaint(window, &paint);
        if (pixels && pixel_width && pixel_height) blit(dc, 0, 0, pixel_width, pixel_height);
        EndPaint(window, &paint);
        return 0;
    }
    case WM_SETFOCUS:
        push(EV_FOCUS, 0, 0, F_PRESSED, 0, 0);
        return 0;
    case WM_KILLFOCUS:
        push(EV_FOCUS, 0, 0, 0, 0, 0);
        return 0;
    case WM_KEYDOWN: case WM_SYSKEYDOWN: case WM_KEYUP: case WM_SYSKEYUP: {
        int down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        unsigned scan = (unsigned)((lparam >> 16) & 0x7f) | ((lparam & (1 << 24)) ? 0x80 : 0);
        uint32_t key = scan_keys[scan & 0xff];
        if (wparam == VK_PAUSE) key = KEY_PAUSE;
        if (wparam == VK_NUMLOCK) key = KEY_NUM_LOCK;
        uint32_t state = down ? F_PRESSED : 0;
        if (down && (lparam & (1 << 30))) state |= F_REPEAT;
        push(EV_KEY, key, current_modifiers(), state, 0, 0);
        /* Alt-F4 still closes through the default handling; other system
         * keys would enter the menu loop. */
        if (message == WM_SYSKEYDOWN && wparam == VK_F4) break;
        return 0;
    }
    case WM_CHAR: {
        WCHAR unit = (WCHAR)wparam;
        if (unit >= 0xd800 && unit <= 0xdbff) { high_surrogate = unit; return 0; }
        if (unit >= 0xdc00 && unit <= 0xdfff) {
            if (high_surrogate) push_text(0x10000 + (((uint32_t)high_surrogate - 0xd800) << 10) + (unit - 0xdc00));
            high_surrogate = 0;
            return 0;
        }
        high_surrogate = 0;
        push_text(unit);
        return 0;
    }
    case WM_SYSCHAR:
        return 0;
    case WM_MOUSEMOVE:
        if (!mouse_tracking) {
            TRACKMOUSEEVENT track = { sizeof(track), TME_LEAVE, window, 0 };
            mouse_tracking = TrackMouseEvent(&track) ? 1 : 0;
        }
        push(EV_MOVE, 0, current_modifiers(), 0, (int32_t)(short)LOWORD(lparam), (int32_t)(short)HIWORD(lparam));
        return 0;
    case WM_MOUSELEAVE:
        mouse_tracking = 0;
        push(EV_LEAVE, 0, 0, 0, 0, 0);
        return 0;
    case WM_LBUTTONDOWN: case WM_LBUTTONUP: case WM_MBUTTONDOWN: case WM_MBUTTONUP:
    case WM_RBUTTONDOWN: case WM_RBUTTONUP:
        button(message, wparam, lparam);
        return 0;
    case WM_XBUTTONDOWN: case WM_XBUTTONUP:
        button(message, wparam, lparam);
        return TRUE;
    case WM_MOUSEWHEEL:
        push(EV_SCROLL, 0, current_modifiers(), 0, 0, -(int32_t)GET_WHEEL_DELTA_WPARAM(wparam));
        return 0;
    case WM_MOUSEHWHEEL:
        push(EV_SCROLL, 0, current_modifiers(), 0, (int32_t)GET_WHEEL_DELTA_WPARAM(wparam), 0);
        return 0;
    case 0x02E0: { /* WM_DPICHANGED: take the size Windows suggests. */
        const RECT *suggested = (const RECT *)lparam;
        dpi = HIWORD(wparam);
        SetWindowPos(window, NULL, suggested->left, suggested->top, suggested->right - suggested->left,
            suggested->bottom - suggested->top, SWP_NOZORDER | SWP_NOACTIVATE);
        return 0;
    }
    case WM_SETCURSOR:
        if (cursor_hidden && LOWORD(lparam) == HTCLIENT) { SetCursor(NULL); return TRUE; }
        break;
    default:
        break;
    }
    return DefWindowProcW(window, message, wparam, lparam);
}

/* Sizes at open are only the initial layout, not a resize. */
static void drop_resizes(void) {
    struct dodo_window_event kept[QUEUE];
    unsigned count = 0;
    struct dodo_window_event event;
    while (pop(&event)) if (event.kind != EV_RESIZE) kept[count++] = event;
    for (unsigned i = 0; i < count; ++i) push(kept[i].kind, kept[i].code, kept[i].modifiers, kept[i].flags, kept[i].x, kept[i].y);
}

int32_t dodo_window_open(const uint8_t *title, size_t title_length, uint32_t width, uint32_t height, uint32_t flags, intptr_t *handle) {
    static ATOM registered;
    if (hwnd) return E_ALREADY_OPEN;
    int resizable = (flags & O_RESIZABLE) != 0;
    has_pixels = (flags & O_PIXEL_BUFFER) != 0;
    HINSTANCE instance = GetModuleHandleW(NULL);
    if (!registered) {
        become_dpi_aware();
        WNDCLASSEXW type;
        memset(&type, 0, sizeof(type));
        type.cbSize = sizeof(type);
        type.style = CS_OWNDC;
        type.lpfnWndProc = procedure;
        type.hInstance = instance;
        type.hCursor = LoadCursorW(NULL, (LPCWSTR)IDC_ARROW);
        type.hIcon = LoadIconW(NULL, (LPCWSTR)IDI_APPLICATION);
        type.lpszClassName = L"DodoWindow";
        registered = RegisterClassExW(&type);
        if (!registered) return E_SYSTEM;
    }
    WCHAR *text = wide_from(title, title_length);
    if (!text) return E_MEMORY;
    DWORD style = WS_OVERLAPPEDWINDOW;
    if (!resizable) style &= ~(DWORD)(WS_THICKFRAME | WS_MAXIMIZEBOX);
    hwnd = CreateWindowExW(0, L"DodoWindow", text, style, CW_USEDEFAULT, CW_USEDEFAULT,
        (int)width, (int)height, NULL, NULL, instance, NULL);
    free(text);
    if (!hwnd) {
        /* Services and other sessions without a desktop cannot create windows. */
        return GetLastError() == ERROR_ACCESS_DENIED ? E_NO_DISPLAY : E_SYSTEM;
    }
    /* Options are in units of the system scale; the drawing area is in pixels. */
    dpi = window_dpi();
    RECT frame = { 0, 0, (LONG)MulDiv((int)width, (int)dpi, 96), (LONG)MulDiv((int)height, (int)dpi, 96) };
    AdjustWindowRectEx(&frame, style, FALSE, 0);
    SetWindowPos(hwnd, NULL, 0, 0, frame.right - frame.left, frame.bottom - frame.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    RECT client;
    GetClientRect(hwnd, &client);
    int code = resize_buffer((uint32_t)(client.right - client.left), (uint32_t)(client.bottom - client.top));
    if (code) { DestroyWindow(hwnd); hwnd = NULL; return code; }
    ShowWindow(hwnd, SW_SHOWDEFAULT);
    UpdateWindow(hwnd);
    SetForegroundWindow(hwnd);
    drop_resizes();
    *handle = 1;
    return 0;
}

/* The performance counter in nanoseconds, the compositor's clock. */
static uint64_t counter_nanos(uint64_t ticks) {
    static uint64_t rate;
    if (!rate) {
        LARGE_INTEGER frequency;
        rate = QueryPerformanceFrequency(&frequency) && frequency.QuadPart > 0 ? (uint64_t)frequency.QuadPart : 1;
    }
    return ticks / rate * UINT64_C(1000000000) + ticks % rate * UINT64_C(1000000000) / rate;
}

uint64_t dodo_window_clock(void) {
    LARGE_INTEGER counter;
    QueryPerformanceCounter(&counter);
    return counter_nanos((uint64_t)counter.QuadPart);
}

/* Sleep until input arrives or `milliseconds` pass. A high-resolution
 * waitable timer (Windows 10 1803 and later) keeps short waits short. */
static int wait_input(DWORD milliseconds) {
    if (milliseconds && milliseconds != INFINITE) {
        if (!wait_timer) wait_timer = CreateWaitableTimerExW(NULL, NULL, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS);
        LARGE_INTEGER due;
        due.QuadPart = -(LONGLONG)milliseconds * 10000;
        if (wait_timer && SetWaitableTimer(wait_timer, &due, 0, NULL, NULL, FALSE))
            return MsgWaitForMultipleObjectsEx(1, &wait_timer, INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE) != WAIT_FAILED;
    }
    return MsgWaitForMultipleObjectsEx(0, NULL, milliseconds, QS_ALLINPUT, MWMO_INPUTAVAILABLE) != WAIT_FAILED;
}

/* The compositor's refresh period and latest refresh, in nanoseconds of
 * dodo_window_clock. Fails without composition, as on Windows 7 Basic. */
static int composition_timing(uint64_t *period, uint64_t *vblank) {
    typedef HRESULT (WINAPI *timing_info)(HWND, DWM_TIMING_INFO *);
    static timing_info lookup;
    static int resolved;
    if (!resolved) {
        HMODULE dwm = LoadLibraryW(L"dwmapi.dll");
        if (dwm) lookup = (timing_info)(void (*)(void))GetProcAddress(dwm, "DwmGetCompositionTimingInfo");
        resolved = 1;
    }
    if (!lookup) return 0;
    DWM_TIMING_INFO info;
    memset(&info, 0, sizeof(info));
    info.cbSize = sizeof(info);
    /* Since Windows 8.1 only the whole desktop's timing, NULL, is valid. */
    if (FAILED(lookup(NULL, &info)) || !info.qpcRefreshPeriod) return 0;
    *period = counter_nanos(info.qpcRefreshPeriod);
    *vblank = counter_nanos(info.qpcVBlank);
    return *period != 0;
}

uint64_t dodo_window_frame_period(intptr_t handle) {
    if (!handle || !hwnd) return 0;
    uint64_t period, vblank;
    if (composition_timing(&period, &vblank)) return period;
    DEVMODEW mode;
    memset(&mode, 0, sizeof(mode));
    mode.dmSize = sizeof(mode);
    /* 0 and 1 stand for the hardware's default rate. */
    if (EnumDisplaySettingsW(NULL, ENUM_CURRENT_SETTINGS, &mode) && mode.dmDisplayFrequency > 1)
        return (UINT64_C(1000000000) + mode.dmDisplayFrequency / 2) / mode.dmDisplayFrequency;
    return 0;
}

uint64_t dodo_window_next_vblank(intptr_t handle, uint64_t now) {
    if (!handle || !hwnd) return 0;
    uint64_t period, vblank;
    if (!composition_timing(&period, &vblank)) return 0;
    if (vblank > now) return vblank;
    return vblank + ((now - vblank) / period + 1) * period;
}

/* DwmFlush would block input until the compositor's next frame, so Windows
 * paces by the refresh times of next_vblank instead of FRAME events. */
int32_t dodo_window_request_frame(intptr_t handle) {
    return !handle || !hwnd ? -E_CLOSED : 0;
}

void dodo_window_set_animating(intptr_t handle, uint32_t animating) {
    (void)handle; (void)animating;
}

void dodo_window_close(intptr_t handle) {
    if (!handle || !hwnd) return;
    if (buttons_down) ReleaseCapture();
    if (wait_timer) { CloseHandle(wait_timer); wait_timer = NULL; }
    DestroyWindow(hwnd);
    hwnd = NULL;
    MSG message;
    while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) DispatchMessageW(&message);
    cursor_hidden = mouse_tracking = buttons_down = 0;
    high_surrogate = 0;
    queue_head = queue_count = 0;
    free_buffer();
    has_pixels = 0;
}

int32_t dodo_window_next_event(intptr_t handle, uint32_t milliseconds, struct dodo_window_event *event) {
    if (!handle || !hwnd) return -E_CLOSED;
    uint64_t start = dodo_window_clock();
    for (;;) {
        if (pop(event)) return 1;
        MSG message;
        while (PeekMessageW(&message, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if (pop(event)) return 1;
        DWORD wait = INFINITE;
        if (milliseconds != FOREVER) {
            uint64_t elapsed = (dodo_window_clock() - start) / 1000000;
            if (elapsed >= milliseconds) return 0;
            wait = (DWORD)(milliseconds - elapsed);
        }
        if (!wait_input(wait)) return -E_SYSTEM;
    }
}

int32_t dodo_window_present(intptr_t handle, int32_t x, int32_t y, uint32_t width, uint32_t height) {
    if (!handle || !hwnd) return E_CLOSED;
    if (!pixels) return E_UNSUPPORTED;
    if (!pixel_width || !pixel_height) return 0;
    HDC dc = GetDC(hwnd);
    if (!dc) return E_SYSTEM;
    blit(dc, x, y, width, height);
    ReleaseDC(hwnd, dc);
    return 0;
}

int32_t dodo_window_set_title(intptr_t handle, const uint8_t *title, size_t title_length) {
    if (!handle || !hwnd) return E_CLOSED;
    WCHAR *text = wide_from(title, title_length);
    if (!text) return E_MEMORY;
    BOOL done = SetWindowTextW(hwnd, text);
    free(text);
    return done ? 0 : E_SYSTEM;
}

int32_t dodo_window_set_cursor_visible(intptr_t handle, uint32_t visible) {
    if (!handle || !hwnd) return E_CLOSED;
    cursor_hidden = !visible;
    POINT point;
    RECT client;
    if (GetCursorPos(&point) && ScreenToClient(hwnd, &point) && GetClientRect(hwnd, &client) && PtInRect(&client, point))
        SetCursor(visible ? LoadCursorW(NULL, (LPCWSTR)IDC_ARROW) : NULL);
    return 0;
}

uint32_t dodo_window_scale_percent(intptr_t handle) {
    if (!handle || !hwnd) return 100;
    return (uint32_t)MulDiv((int)dpi, 100, 96);
}

int32_t dodo_window_native(intptr_t handle, uintptr_t *first, uintptr_t *second) {
    if (!handle || !hwnd) return -E_CLOSED;
    *first = (uintptr_t)hwnd; *second = (uintptr_t)GetModuleHandleW(NULL);
    return NATIVE_WIN32;
}

/* Metal layers are macOS only. */
int32_t dodo_window_metal_layer(intptr_t handle, uintptr_t *out) {
    *out = 0;
    return !handle || !hwnd ? E_CLOSED : E_UNSUPPORTED;
}
#endif
