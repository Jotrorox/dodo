# Changelog

## Unreleased

- `gpu.wgsl("...")` writes a std/gpu shader once in WGSL: the compiler
  translates it into SPIR-V, Metal Shading Language and GLSL 4.50 when the
  program is built (through the `naga` crate, behind the new default `wgsl`
  feature), so one source runs on Vulkan, Metal and OpenGL with no shader
  tools installed. Mistakes are compile errors pointing into the literal.
  Slot N is `@group(0) @binding(N)`, and a sampled texture's sampler is
  `@group(1) @binding(N)`. See [Shaders in WGSL](docs/src/content/docs/gpu.md).
- `gpu.Surface` presents std/gpu frames in std/window windows: a Vulkan
  swapchain on Linux and Windows, the view's `CAMetalLayer` on macOS, or the
  window's own OpenGL context (from the new `gpu.Device.open_for_window`).
  `acquire()` hands out a frame to render into, `present()` shows it, with
  two or three frames in flight (`gpu.SurfaceOptions`) and `resize()` for
  window size changes; the new `gpu.Error.Outdated` asks for one. See
  [Surfaces](docs/src/content/docs/gpu.md) and the new
  `examples/gpu_window.dodo`.
- std/gpu no longer waits for the GPU to write data: `write_buffer`,
  `write_texture` and `create_buffer_with_data` copy into a staging ring and
  record the copy into a command buffer submitted ahead of the next
  submission, still ordered as before. On Vulkan, buffers and images are
  carved out of large memory blocks, buffers live in device memory (mapped on
  unified-memory GPUs), and reads come back through host-cached memory.
  `DODO_GPU_UNMAPPED` makes the Vulkan backend take the discrete-GPU paths
  anywhere, for testing.
- std/gpu derives barriers from how commands use each buffer and texture
  instead of placing a full barrier before every pass, copy and dispatch on
  Vulkan (where render passes now record into secondary command buffers) or
  after every pass and dispatch on OpenGL. Vulkan command pools are reused.
- The std/gpu examples write their shaders once in WGSL.
- New database packages. `std/db` holds the shared vocabulary for drivers:
  `db.Error` with portable `db.ErrorKind`s and `db.Constraint`s plus the
  driver's native code, the borrowed `db.Value`, and `db.IsolationLevel`.
  `std/db/sql` defines the `sql.Conn`, `sql.Row`, and `sql.RowSink`
  interfaces that every SQL driver matches, so generic code
  (`<C: sql.Conn>`) and runtime-selected code (`&mut dyn sql.Conn`) work with
  any of them.
- `std/db/sqlite` is an SQLite-compatible database engine written entirely in
  Dodo, with no native library: it reads and writes SQLite 3 database files
  (every page size, overflow pages, freelist, rollback journal with hot-journal
  recovery, and SQLite's locking protocol), so the same file can be used by
  SQLite itself at the same time. It runs inside a caller-owned workspace and
  never allocates. SQL support covers tables, indexes (including SQLite's
  automatic indexes), views, joins, subqueries, compound selects, common table
  expressions including `WITH RECURSIVE`, aggregates, `GROUP BY`/`HAVING`,
  `ORDER BY`/`LIMIT`, UPSERT, `RETURNING`, constraints with conflict clauses,
  `AUTOINCREMENT`, `ALTER TABLE`, transactions with statement-level rollback,
  common pragmas, and SQLite's core, aggregate, and date/time functions with
  its type affinity rules. Differential tests compare results and written
  files against SQLite. See the new [SQLite guide](docs/src/content/docs/sqlite.md).
- `fs.File.try_lock_range(start, length, shared)` and `unlock_range` take
  non-blocking advisory byte-range locks (POSIX `fcntl` record locks through
  the fs C adapter, `LockFileEx` on Windows).
- String literals no longer conflict with borrows of `static mut` storage
  (for example passing `"literal"` while holding `&mut WORKSPACE[..]`).
- `.len` and indexing through a reference to a slice or string (such as the
  `&&[u8]` a `match &value` binds) no longer fail in code generation.

- `std/script/lua` is a complete Lua 5.4 implementation written in Dodo,
  for embedding scripts, configuration and plugins in Dodo programs. It
  follows PUC Lua 5.4.9, including its error messages and the
  `LUA_COMPAT_5_3` behaviour of a default build, passes the official Lua
  5.4.9 test suite (except the parts that need the internal `T` library),
  and loads binary chunks from the reference `luac` 5.4. All standard
  libraries are included; `io` and `os` wrap the target's C library on
  Linux, macOS and Windows. The embedding API mirrors the Lua C API: a stack,
  native functions (`lua.NativeFn`) with argument checking, protected calls,
  userdata with metatables, coroutines with continuations, Dodo modules for
  `require` (`preload_module`), memory limits, warnings, hooks and Ctrl-C
  interrupts. There is no C API, so C modules cannot be loaded. At `-O 2`
  it runs the `benchmarks/lua` set about 18% faster than the reference
  interpreter. `examples/lua` builds a standalone `lua` interpreter
  equivalent to the reference one, and `examples/lua_embed` shows embedding;
  the [Lua guide](https://jotrorox.github.io/dodo/script/) covers both.
  `tests/lua_corpus.rs` runs the `tests/lua` corpus against output recorded
  from the reference interpreter.
- Functions take `@inline`, `@inline(never)` and `@cold` attributes, which
  optimized builds apply. A `match` on integer literals compiles to one LLVM
  `switch`, and an interpreter loop of the shape `for { ...; match <u8> { ... } }`
  with at least 16 literal arms dispatches through a table of block addresses
  (threaded dispatch).
- On Linux, constants that hold addresses (such as a `const NAME: &str`)
  are placed in `.data.rel.ro` sections. They were in writable `.rodata`
  sections, which GNU ld merged into the code segment, producing a
  writable and executable segment and a linker warning.
- GPUs draw straight into `std/window` windows:
  - `win.native_handle()` returns a `window.NativeHandle` (defined in the
    new, dependency-free `std/window/handle`): `Xcb(connection, window,
    screen, visual)` on Linux, `AppKit(window, view)` on macOS and
    `Win32(hwnd, instance)` on Windows. On Linux the first call loads
    `libxcb.so.1` at run time and opens a second connection to the X
    server, since Vulkan and EGL cannot use std/window's own socket;
    programs that never ask still need nothing beyond libc. Every window of
    the `App` shares that connection, which closes with the `App`.
  - `window.Options.gpu(title, width, height)` opens a window without a
    pixel buffer, and `Options` gains the `pixel_buffer` field (struct
    literals must now set it). Its
    `canvas()` is empty and `update()` fails with `Unsupported`.
    `win.has_pixel_buffer()` tells the two kinds apart.
  - `win.metal_layer()` gives a GPU window's view a `CAMetalLayer` on macOS,
    for Metal and Vulkan's `VK_EXT_metal_surface`. std/window keeps its
    drawable size and contents scale in step with the window.
  - `opengl.Context.create_for_window(&native, options)` draws into a
    window's framebuffer 0 through EGL's XCB platform (or Xlib and EGL's X11
    platform), an `NSOpenGLContext` or WGL, with `swap_buffers()`,
    `set_swap_interval()` and `update_surface()`.
  - `examples/opengl_window.dodo` now presents its frames directly instead of
    reading them back through the CPU, and `examples/vulkan_window.dodo` and
    `examples/metal_window.dodo` draw a spinning triangle with Vulkan (XCB,
    Win32 and Metal surfaces, rebuilding the swapchain on resize) and Metal.
- `std/graphics` strokes paths: `canvas.stroke_path(&path, &style, color)`
  paints a band of the stroke's width along a path, as borders, chart lines
  and icons need. A `graphics.Stroke` sets the width (in subpixels), miter,
  round or bevel joins with a miter limit, butt, round or square caps, and a
  dash pattern with an offset, with the defaults of SVG and the HTML canvas.
  Open subpaths get caps, `close` joins, zero-length subpaths are dots, and a
  closed subpath's last dash joins its first. Strokes stay integer-only and
  exact like fills: a stroke along pixel edges covers whole pixels, and a
  translucent stroke paints each pixel once where it overlaps itself.
  `stroke_outline` strokes any `graphics.Outline`, TrueType glyphs included,
  `gradient_stroke_path` and `gradient_stroke_outline` paint with a gradient,
  and `graphics.Stroked` is a stroke as an outline of its own for coverage
  masks. Work follows what is visible: segments, joins, caps and dashes
  outside the clip rectangle are skipped, even for strokes far wider than
  the canvas. A new golden image covers every join, cap and dash.
- Persistent storage for firmware. `hal.Flash` is a device protocol for NOR
  flash (erase blocks to `0xFF`, then write erased bytes once), and
  `std/embedded/storage` builds on it: `storage.Store`, a key-value store
  with text keys and byte values (`get`, `set`, `remove`, `contains`,
  `len_of`, `get_u32`/`set_u32`, key listing, `clear`), and
  `storage.Region`, a window of any flash device. The store is a log over
  2 to 64 erase blocks with even wear and compaction; every record has a
  CRC-32 and is verified, writes never touch written bytes, and a power cut
  at any point loses at most the operation in progress. On the RP2040 and
  RP2350, `b.flash` (`std/embedded/chip/rp_flash`) reads the flash and hands
  out regions above the program, which reflashing keeps: the chip runtimes
  erase and program through the boot ROM, from SRAM with execute-in-place
  restored by a copy of `.boot2` on the RP2040 and through `flash_op` on the
  RP2350, with interrupts masked, and refuse while core 1 runs a task.
  `fake.Flash` checks the protocol strictly and can cut the power mid-write
  for tests, which sweep power cuts over every erase and write of a long run.
  `examples/boot_counter.dodo` counts restarts, and the Pico self-tests check
  regions, writes, and a store on the flash.
- `std/window` supports several windows per process, for dialogs, popups
  and tool windows. This changes the API: a `window.App` connects to the
  window system and runs the one event loop, `app.open(options)` opens any
  number of windows, and `app.wait_event()`, `poll_event()` and
  `wait_event_timeout()` return the events of all of them. Every `Event`
  variant now starts with the `WindowId` of its window (`win.id()`,
  `window.event_window(&event)`); ids are never reused within a process.
  Animation asks per window: `win.request_frame()` delivers one
  `Event.Frame(id)` at that window's next display refresh, with the same
  pacing as before, replacing `wait_event_until_frame()`. Command-Q on macOS
  now reports the app-level `Event.Quit` instead of closing the window.
  Windows do not borrow their App, and each owns its pixel buffer, which is
  only resized inside the window's own calls. `window.open` is gone; use
  `window.App.new()?` and `app.open(...)`. `examples/window_palette.dodo`
  paints with colors picked in a second window.
- Fixed: X11 focus events took their mode from the wrong byte, so focus
  changes caused by pointer and keyboard grabs were reported too.
- `async fn` and `await`: an optional concurrency model for firmware on top of
  `std/embedded/executor`. Calling an async fn returns a future, a struct
  named after the function (`blink` → `BlinkFuture`) whose `poll` is the
  executor's task method, so async and hand-written tasks run side by side;
  `await` waits for another future and yields its `output`. The compiler
  generates the state machine: the body's locals live in the future, so
  references between them survive suspensions, and dropping an unfinished
  future drops what it holds. Methods and generic functions can be async.
  `await` begins its statement, possibly followed by `?`, `!`, or `as`; a
  started future traps if polled after moving, and a future that would hold
  itself is rejected. The executor gains the futures async code waits on:
  `sleep_ms`, `sleep`, `sleep_until`, `yield_now`, `now`, `Ticker.tick`,
  `Signal.wait`, `Signal.next_value`, and `select` and `join` for two
  futures. `examples/async_tasks` is `examples/tasks` written with them.
- `std/embedded/sync`: `Mutex<T>` shares values between interrupt handlers,
  tasks, and cores from a plain static, without `static mut` or `unsafe`.
  `lock()` masks interrupts on this core and returns a guard; `with`,
  `set`, `replace`, and `try_lock` cover the rest, and locking a mutex this
  core already holds traps. All mutexes lock through one nesting
  system-wide section, `sync.enter()`. On the RP2040 and RP2350,
  `Core1.start` installs hardware spinlock 31 as its cross-core lock, so
  the same mutex also holds off the other core; spinlock 31 is no longer
  handed out by `Spinlocks`. `examples/interrupts` uses mutexes now.
- The RP2350's doorbells: `b.doorbells` on the Pico 2 and Pico 2 W hands out
  the eight doorbells between the cores. `ring()` interrupts the other core,
  and `set_signal` makes the runtime's new `SIO_IRQ_BELL` handler raise an
  executor signal there, to wake a task on the other core without the FIFO.
- Random numbers for firmware: `std/embedded/hal` gains the `Rng`
  protocol (`fill(bytes)`), with `random_u32`, `random_u64`, and the unbiased
  `random_below`, and `hal.SoftRng`, a xoshiro128** generator seeded from a
  hardware one with `from_rng` or from a fixed seed with `new`.
  `hal/fake` gains `fake.Rng`. Every board has `b.rng`, and the chip packages
  `Peripherals.rng`, from the new `std/embedded/chip/rp_rng`: on the RP2040,
  `rp_rng.Rosc` samples the ring oscillator's RANDOMBIT about every 10 us and
  debiases it with von Neumann's method, about 3 KB a second and not for
  keys; on the RP2350, `rp_rng.Trng` runs the TRNG with its hardware
  debiasing and all health tests on, about 20 KB a second and fit for keys,
  restarting it after failed autocorrelation tests and keeping a block's
  unused bytes for the next fill. A stopped source reports `Timeout` and
  failing health checks `Bus`. Both drivers are generic over
  `hal.Registers` and tested against simulated registers on the desktop.
- `std/gpu`: portable GPU compute and rendering on Metal, Vulkan and
  OpenGL, written in Dodo on top of `std/gpu/metal`, `std/gpu/vulkan` and
  `std/gpu/opengl`. It follows WebGPU's model: a `Device` creates buffers,
  2D textures, samplers and compute and render pipelines; a
  `CommandEncoder` records compute passes, render passes and copies;
  `submit()` runs them in order, and `write_buffer`, `read_buffer`,
  `write_texture` and `read_texture` move data in queue order.
  `Device.open` picks Metal on macOS and Vulkan elsewhere, falling back to
  OpenGL 4.5. Shaders come as SPIR-V, MSL and GLSL in one `ShaderSource`,
  with numbered binding slots that mean the same in every language, and
  clip space, depth range, winding and texture rows agree on every backend.
  Every call is checked (usages, ranges, pipeline layouts and attachment
  formats, workgroup limits, and SPIR-V entry points), and resources dropped
  while the GPU may still use them are destroyed once it has finished.
  Backends implement the `std/gpu/hal` interface; `std/gpu/hal/native`
  selects the OS's own API. A fixture runs the same compute, rendering,
  depth, sampling, blending, copy and lifetime checks on Metal, on Vulkan
  with lavapipe and on OpenGL with llvmpipe, clean under the Vulkan
  validation and synchronization layers and Metal's debug layer.
  `examples/gpu_compute.dodo` and `examples/gpu_triangle.dodo` show compute
  and a window. The Vulkan guide moved to its own page, and
  `std/gpu/opengl` now declares `dlsym` and `GetProcAddress` with the
  pointer results `std/dylib` uses, so one program can load both.
- Large values loaded whole and stored after code that may write their
  source, such as a big struct returned in a `Result` while locals with
  drops are still alive, are now copied aside in memory instead of being
  split by LLVM into one operation per byte. Such functions took minutes to
  compile at -O1 and above.
- Fixed: `Fifo.set_signal` and the FIFO interrupt hook of
  `std/embedded/chip/rp_multicore` wrote 0 to `FIFO_ST` to clear its sticky
  WOF and ROE flags, which only a 1 clears. A flag left set, such as the ROE
  that core 1's boot ROM leaves while it waits to be launched, kept the FIFO
  interrupt raised, so an executor waiting for FIFO words could spin instead
  of sleeping.
- CI runs firmware on emulated RP2040 and RP2350 chips: a self-test of the
  startup runtime, the integer and floating-point helpers, interrupts,
  SysTick, timer alarms, the executor, DMA, PIO, a UART, and the second core
  with its FIFOs and spinlocks, at -O0 and -O2. `scripts/test_emulator.py`
  builds `tests/emulator` and runs it in `emulator/`, a runner around the
  picoem emulator crates with fixes in `emulator/patches`; it takes seconds.
- Real-time clocks share one protocol in `std/embedded/hal`: `hal.Rtc` reads
  and sets the time as Unix milliseconds and `hal.RtcAlarm` fires once at a
  time, with `hal.rtc_now`, `rtc_set`, `rtc_alarm_at`, and `rtc_alarm_in`
  converting to and from `std/time`'s `DateTime`. Every board has `b.rtc`:
  the RP2040's RTC, clocked from the crystal and counting seconds, and the
  RP2350's always-on timer, a millisecond counter that keeps counting through
  resets. The register-level drivers are in `std/embedded/chip/rp_rtc`, with
  RTC_IRQ and POWMAN_IRQ_TIMER raised by their alarms.
  `std/embedded/rtc/ds3231` drives a battery-backed DS3231 clock chip on I2C
  through the same protocols, and `fake.Rtc` tests clock code on a desktop.
  The Pico self-tests check the clocks and their alarms on the boards.
- `std/embedded/hal` settles its device protocols before a second chip
  vendor arrives, so drivers written now keep working:
  - Polling forms of the buses, `hal.I2cPoll`, `hal.SpiPoll`, and, in the
    new `std/embedded/hal/serial` (which keeps `hal` free of `std/io`),
    `serial.PollPort`, never wait: each call does what the hardware allows
    and returns true once the operation has finished, keeping its state in
    the bus rather than in borrowed buffers, so transactions may be any
    length and run across an executor task's polls. `hal.BlockingI2c` and
    `hal.BlockingSpi` turn a polling-only controller into a blocking one, and
    `hal.SpiDevice` keeps a device selected across polls. `serial.Port` adds
    `flush` to std/io's streams.
  - Target sides: `hal.I2cTarget` with `hal.I2cTargetEvent`, and
    `hal.SpiTarget`.
  - Pulse measurement: `hal.PwmInput` and `hal.PulseCapture` (period, high
    time, frequency, duty).
  - CAN: `hal.Can`, `hal.CanFrame` (standard, extended, remote, and CAN FD
    frames, checked as they are set), `hal.CanFilter`, `hal.CanErrorState`,
    and `hal.can_timing`, which computes bit timing within any controller's
    limits.
  - I2S: `hal.I2sOutput` and `hal.I2sInput` with MSB-aligned `i32` samples
    and `hal.I2sFormat`.
  - Background transfers: `hal.DmaBuffer` and `hal.SpiDma` for sessions that
    hold a buffer, `hal.DmaStream`, the DMA-backed session every peripheral
    driver needs, written once against `hal.DmaChannel`, and `hal.dma_copy`.
  - Helpers that wait with a poll limit: `hal.capture`, `hal.send_frame`,
    `hal.receive_frame`, `hal.write_samples`, `hal.read_samples`, and
    `hal.finish_transfer`.
  - `std/embedded/hal/fake` gains a fake for every new protocol, polled
    modes for `fake.I2c`, `fake.Spi`, and `fake.Serial`, and
    `fake.DmaChannel`, which runs DMA transfers in host memory.
- The RP2040 and RP2350 implement the new protocols: the DesignWare I2C
  bus `hal.I2cPoll` for transactions of any length (replacing the
  FIFO-sized `start_*`/`poll_finish` methods), with an interrupt whenever a
  poll can make progress; I2C targets `hal.I2cTarget` (the driver's own
  `TargetEvent` is gone); PL022 SPI buses `hal.SpiPoll`, targets
  `hal.SpiTarget`, and `pl022.DmaBus` `hal.SpiDma` with `poll_finish` and
  `abort`; serial ports and the USB serial port `serial.Port` and
  `serial.PollPort` with `poll_flush`.
- `pins.pwm_input(pin, window_us)` measures pulses on an odd GPIO with a PWM
  slice's counting modes, as a `hal.PwmInput`: the frequency and duty of a
  PWM signal, a fan's tachometer, or a servo pulse, to within microseconds.
- `std/embedded/chip/rp_i2s` plays and records I2S audio on PIO, as
  `hal.I2sOutput` and `hal.I2sInput`: Philips, left- and right-justified
  framing, 16 to 32-bit samples, mono or stereo, a fractional clock reported
  with its error in ppm, silence instead of swapped channels when the queue
  runs dry, and playback by DMA through `hal.DmaStream`.
- `std/embedded/can/mcp2515` drives the MCP2515 CAN controller over any SPI
  bus as a `hal.Can`, so boards without a CAN controller, such as the Pico,
  get CAN: bit timing from the crystal, filters mapped onto its masks,
  frames sent and received in order, overrun and error states, and its
  interrupt pin.
- `std/embedded/power` is one low-power layer for every chip: sleep and deep
  sleep modes that every wait honours, including timer alarms and the
  executor; clock gating with `power.Clocks` sets; and a dormant state, with
  every oscillator stopped, that pins in a `power.Wake` end. The RP2040 and
  RP2350 implement its `Power` protocol as `Peripherals.power`, and every board
  hands it out as `b.power`. The chips name each clock gate
  (`CLOCK_UART0`, `ALARM_CLOCKS`, `ESSENTIAL_CLOCKS`, ...).
  `disable_unused_clocks` stops the clocks of unclaimed peripherals, and
  claiming a peripheral starts them again. `disable_clocks` refuses clocks
  the program needs or a claimed peripheral uses. `dormant` runs from the
  crystal, stops the PLL and both oscillators, and restores clk_sys on wake.
  `fake.Power` tests power-aware code on a desktop.
  `cortex_m.set_deep_sleep`, `set_sleep_on_exit`, and `set_event_on_pend`
  expose the core's SCR. The self-tests gain on-board checks, and
  `examples/low_power.dodo` shows all three levels.
- `std/embedded/hal` gains a shared timer protocol: `Counter` (`now_us`), a
  free-running microsecond count, and `Alarm` (`now_us`, `schedule_at_us`,
  `cancel`, `fired`, `clear`), a one-shot compare that fires at once for a
  time already reached. `hal.Deadline` and `hal.Ticker` give drivers
  timeouts and drift-free periodic work on any counter, with
  `elapsed_us`, `wait_until`, `schedule_in_us`, and `wait_for_alarm`
  helpers. Board timers are counters, and the RP2040 and RP2350 alarms
  implement the alarm protocol for times up to 35 minutes ahead, forcing
  their interrupt when a time has passed before the alarm was armed.
  `std/embedded/hal/fake` gains `Alarm`, and `fake.Delay` is a counter whose
  time is the delays asked of it, so timeouts are tested without waiting.
- Watchdog for the RP2040 and RP2350: `b.watchdog` on every board (and
  `watchdog` in the chips' `Peripherals`) is a `std/embedded/chip/rp_watchdog`
  `Watchdog`. It is stopped after `take()`. `start(timeout_ms)` makes it reset
  the chip unless `feed()` is called in time, from 1 ms up to 8388 ms on the
  RP2040 (whose count runs twice per tick, erratum RP2040-E1) or 16777 ms on
  the RP2350. `stop()`, `remaining_us()`, and `restart()` for a deliberate
  reset complete it. The reset restarts everything but the oscillators, and
  the count pauses while a debugger halts a core. `reset_reason()` reads
  `hal.ResetReason.Timeout`, `Restart`, or `Other`. A Pico SDK-style mark tells
  the program's watchdog resets apart from the boot ROM's, so the first run
  after flashing reads `Other`. Two `scratch` words survive the reset.
  `std/embedded/hal` gains the `Watchdog` protocol (`feed`), so drivers and
  tasks can feed any chip's watchdog. Desktop tests run the driver against a
  model of both chips' watchdogs, and the board self-tests gain a watchdog
  check.
- `std/gpu/opengl` creates OpenGL contexts without a window, and
  `std/gpu/opengl/gl` binds the complete OpenGL 4.6 core profile (656
  commands, 1364 enums), generated from the Khronos registry by
  `scripts/generate_opengl.py`. Both are pure Dodo with no C runtime: each
  OS's interface is opened at run time (EGL on Linux, surfaceless and without
  an X server; CGL on macOS; WGL with a hidden window on Windows), so programs
  link no graphics library and start on machines without one.
  `opengl.Target` is an offscreen framebuffer whose pixels read back in
  `std/graphics` row order, as RGBA or as the BGRA a `std/window` canvas uses;
  `examples/opengl_window.dodo` shows a shaded triangle in a window. A
  fixture renders and reads back a triangle with Mesa's llvmpipe in Linux CI
  and with CGL on macOS.
- `std/gpu/metal` binds Apple's Metal API on macOS (Apple silicon and Intel)
  entirely in Dodo, with no C runtime: each method is an `objc_msgSend`
  declared with its own signature. It covers devices, command queues,
  command buffers, compute, render and blit encoders, buffers, textures,
  samplers, shader libraries from source or `.metallib` data, compute and render
  pipelines, vertex, depth-stencil and render-pass descriptors, fences,
  events and `CAMetalLayer`. Objects release on drop, and compile and
  pipeline errors carry Metal's message. Buffer and texture copies check
  their ranges, and binding indices are bounds-checked. `std/gpu/metal/objc`
  exposes the Objective-C runtime layer for methods the module lacks. Loading
  the package links Metal, QuartzCore, Foundation and libobjc, and importing
  it for a non-macOS target is an error. The test suite runs compute,
  rendering and readback on the GPU, including x86-64 under Rosetta, and
  checks every constant against the SDK headers.
- `truetype.GlyphCache` keeps rasterized glyphs in caller-provided storage,
  so text drawn again is blended instead of rasterized: about 13 times
  faster at 14 pixels and 6 times faster at 96. It draws like `Font.draw`
  except that glyphs are placed to a quarter pixel, and a full cache evicts
  its oldest glyphs. `std/graphics` gains the coverage calls it is built on:
  `outline_bounds`, `coverage_mask` with `Canvas.fill_mask` and
  `gradient_mask`, and the run-length `coverage_spans` with
  `Canvas.fill_spans` and `gradient_spans`, all drawing exactly what
  `fill_outline` draws.
- `std/gpu/vulkan`: the Vulkan API in pure Dodo, one to one like Rust's
  `ash`, as the bottom layer for the portable `std/gpu`. Constants,
  handles, enums, bitmasks, structs and the command tables are generated from
  the Khronos registry by `scripts/generate_vulkan.py` and cover core Vulkan
  1.0 to 1.4, the Xlib, XCB, Wayland, Win32, Metal and headless surfaces,
  swapchains, portability (MoltenVK) and debug utils. Nothing is linked:
  `vk.open()` loads `libvulkan.so.1`, `vulkan-1.dll` or the macOS loader or
  MoltenVK at run time and resolves every command through
  `vkGetInstanceProcAddr`/`vkGetDeviceProcAddr` into `extern "C" fn` fields
  of `EntryFns`, `InstanceFns` and `DeviceFns`. Structs have a `default()`
  with `s_type` set, and helpers cover Result checking and names, API
  versions, name fields and clear-value unions. All 2,572 struct sizes,
  alignments and field offsets match the C headers
  (`scripts/vulkan_layout_check.py`), and `examples/vulkan_compute.dodo` runs
  a compute shader on Mesa's lavapipe.
- `std/dylib` opens shared libraries at run time (dlopen on Linux and macOS,
  LoadLibrary on Windows) and looks up their symbols; Linux builds that use
  it also link libdl for glibc before 2.34.

- `&mut T` parameters of Dodo functions, methods and closures are `noalias`
  in LLVM, as Rust's are, so optimized code no longer reloads fields of
  `self` after every store through a slice or pointer it holds. A loop that
  fills `self.data` now compiles to `memset` instead of reloading and
  bounds-checking the slice per byte, without the local-subslice workaround.
  The flow checker already guarantees exclusivity for safe code. Unsafe code
  must not reach storage passed as `&mut` by another route until the call
  returns (spec PTR-ALIAS). Shared references stay unmarked because atomics
  may change shared storage, and so do `&mut dyn` pairs, slices and C
  functions.
- `std/graphics` solid fills and same-format blits are 9–35× faster at -O2
  (1280×800 `clear` 1.4 ms → 0.06 ms, full-frame `blit` 2.3 ms → 0.07 ms).
  Their loops now store through a local row subslice, which the optimizer
  turns into vector stores and `memcpy`.
- `@link_name("symbol")` binds an `extern "C"` prototype to a symbol other
  than its own name, and several prototypes may bind one symbol with different
  signatures. Each call uses its own declaration's signature, so Objective-C
  bindings can declare one `objc_msgSend` per method signature. Without
  `@link_name`, clashing signatures for one symbol are still an error.
- `extern "C" fn(A) -> R` is a C function pointer type, for code that uses the
  platform C calling convention: pointers from `dlsym`, `GetProcAddress` or
  `vkGetInstanceProcAddr`, COM method tables, and callbacks passed to C. An
  `extern "C"` function used as a value now has this type instead of being
  rejected. Calls through one need `unsafe` and follow the C convention,
  including narrow-integer extension, and unsafe `as` converts between C
  function pointers, raw pointers and `usize`. The type never converts to or
  from Dodo's `fn(..)` and closure types, which keep their internal ABI. C
  declarations may take and return C function pointers.
- `extern "C"` functions pass and return `@repr(C)` structs by value, both
  when Dodo calls C and when C calls a Dodo `extern "C" fn`. The compiler
  classifies each struct as the target's C compiler does for x86-64 System V
  (Linux, macOS), Windows x64 and AArch64 (Linux, macOS, Windows), so calls
  such as Metal's `dispatchThreadgroups:threadsPerThreadgroup:` with
  `MTLSize` need no C shim. Struct fields must themselves be C types, and
  structs with destructors are rejected. Other targets report an error for a
  struct by value. A fixture checks every classification case against the
  host C compiler, including structs that move to the stack when registers
  run out.
- Antialiased drawing, gradients and TrueType text for `std/graphics`, still
  integer-only, allocation-free and identical on every target, firmware
  included. The `aa_` canvas calls draw lines, thick round-capped lines,
  circles, ellipses, rounded rectangles and polygons with exact
  per-pixel coverage, composited over the canvas; axis-aligned ones match
  their integer counterparts pixel for pixel. `graphics.Path` stores move,
  line, quadratic and cubic commands in caller-owned words, and any type
  with a `trace(&self, pen)` method is a `graphics.Outline` drawn without
  storage, filled by the nonzero or even-odd rule. The rasterizer's cost
  follows a shape's edges rather than its bounding box: an outline is
  flattened once, coverage is computed only in pixels an edge crosses, and
  interiors are filled as whole spans. `graphics.Gradient` paints
  linear and radial gradients with up to eight stops, premultiplied
  interpolation, pad/repeat/reflect spreads and optional ordered dithering
  for RGB565 and monochrome panels. `std/graphics/truetype` reads `.ttf` and
  `.ttc` fonts in place and draws them at any size with kerning and subpixel
  positioning, decoding glyph outlines straight from the font data (simple
  and composite glyphs, cmap formats 0, 4, 6 and 12). Coverage, symmetry,
  band rendering and hostile-font fuzzing are checked at `-O0` and `-O3`, and
  for WebAssembly and Cortex-M0+; new golden images cover the antialiased
  shapes, gradients and text from generated test fonts.
- `std/graphics/image` reads and writes PNG. The decoder handles every color
  type and bit depth (1 to 16 bits), palettes, `tRNS` transparency and Adam7
  interlacing, checks every chunk CRC and the zlib checksum, and ignores
  gamma and color profiles. `decode_png` and `draw_png` take caller scratch
  of `png_scratch_size` bytes; `decode` and `draw` use 16 KiB of stack.
  `encode_png` writes uncompressed 8-bit RGBA of exactly `png_size` bytes,
  and `encode_png_compressed` adds per-row adaptive filters and deflate with
  caller storage. `examples/poster.dodo` draws a poster with a TrueType font
  and saves it as a PNG.
- Fixed: the deflate encoder wrote wrong codes for literal bytes 144 to 255
  in fixed-Huffman blocks, corrupting small compressed outputs that contained
  such bytes.
- `std/window` opens a desktop window whose drawing area is a
  `std/graphics` canvas, using only what each system ships: the X11
  protocol spoken directly over the display socket on Linux (libc only, no
  Xlib), AppKit through the Objective-C runtime on macOS, and user32/gdi32
  on Windows. `win.canvas()` lends out the window's pixel buffer and
  `update(dirty)` shows the part that changed; the window also implements
  `graphics.Display`. Events are polled with `wait_event`, `poll_event` or
  `wait_event_timeout`: close requests, resizes, focus, physical keys named
  by their US-layout position, layout-aware text, mouse moves, buttons,
  leaving and scrolling. Sizes follow the system scale on high-density
  displays, with the canvas in physical pixels and `scale()` reporting the
  factor. `std/window/x11` holds the protocol encoders and parsers as pure
  functions that build for every target. `dodo run` needs no flags or
  system packages.
- `std/window` paces animation: `wait_event_until_frame()` returns events as
  they arrive and `none` once per display refresh, so a steady stream of
  mouse moves no longer holds frames back, and `frame_interval()` gives the
  refresh period. Frames follow the display where the system reports it: a
  `CADisplayLink` on macOS 14 and later, DWM's refresh timing on Windows and
  the X11 Present extension on Linux, with a timer elsewhere. Animating
  programs on macOS are exempt from App Nap until they call `wait_event()`
  again, and Windows waits use a high-resolution timer instead of the
  15.6 ms system tick. `examples/window.dodo` uses it and moves the ball by
  time, at the same speed on 60 and 120 Hz displays.
- MQTT 3.1.1 packages for both ends of a publish/subscribe system, in the
  style of the web packages. `std/mqtt/broker` runs a broker in one line,
  `broker.new().run(b"0.0.0.0:1883")`. It serves 16 clients from one thread
  with QoS 0 to 2, wildcards, retained messages, wills, keep-alive and session
  takeover. `.password(user, pass)` or `.authenticate(value)` controls who may
  connect, and `.on(filter, handler)` handlers see every published message.
  `std/mqtt/app` is a long-running client:
  `app.new(id).on(filter, handler).every(ms, task).status(topic).run(url)`
  subscribes, calls each matching handler with the message and an outbox for
  replies, runs timers, keeps a retained online/offline status, and
  reconnects with backoff. Both builders check filters before connecting.
  Their `build()` results test handlers and tasks in process with
  `deliver`/`publish`/`tick` and chainable `mqtt.Delivery` assertions.
  `std/mqtt/hosted` is the step-by-step blocking client underneath
  (`connect`, `publish_with`, `subscribe`, `receive`, `poll`). `std/mqtt`
  holds the shared `Message`, `Outbox`, `reply`/`forward` handlers, packet
  encoders and decoders for both sides, and topic matching, and stays
  portable to WebAssembly and bare-metal targets. Spec-derived packet vectors,
  a scripted loopback broker, raw-socket checks of the Dodo broker, and an app
  talking to it check everything at `-O0` and `-O3`. TLS (`mqtts://`) and
  MQTT 5 are not supported yet.
- Display panels: `std/embedded/display/st7789` drives ST7789 colour panels
  (240x240, 240x320, 135x240, 240x280, 172x320 and 170x320, in four
  rotations) and `std/embedded/display/ssd1306` SSD1306 OLED panels (128x64,
  128x32, 96x16, 64x48 and 72x40). Both implement `graphics.Display` and
  send only the dirty rectangle, so drawing code tested against
  `fake.Display` runs unchanged on a panel. They talk through
  `std/embedded/display`'s `SpiInterface` (an SPI bus with a data/command
  pin) or `I2cInterface`, or any value with the `display.Interface`
  methods. `canvas.set_origin(x, y)` places a canvas at a point of a larger
  screen, and `graphics.Bands` moves one down the screen band by band, so
  frames bigger than RAM are drawn with the same code; displays now accept
  canvases that cover part of them and ignore what lies outside. The
  thermometer examples show the chip's temperature on an ST7789 panel, and
  `examples/oled_demo.dodo` animates a 0.96" SSD1306 module on I2C.
- QUIC connections and streams, through OpenSSL 3.5's QUIC stack:
  `std/quic/openssl` binds a `Listener` on a UDP socket and connects a
  verified `Connection` to it, each opening and accepting bidirectional and
  unidirectional streams that send, receive, conclude with FIN, and reset
  with an application code. Every call is nonblocking; `wait` sleeps until a
  datagram or the next protocol timer, and `timeout_ms` hands that timer to
  another poller. Connections require ALPN, verify the server's hostname or
  IP, support mutual authentication and an idle timeout, and close with an
  application code that the peer reads from `close_info`. Streams also
  implement the `std/io` polling contract. The portable `std/quic` package
  holds the error and state types, and compiles for freestanding targets.
- `std/graphics` draws into caller-owned pixel storage on every target,
  firmware included, with no allocation, operating system or floating
  point: lines, rectangles, rounded rectangles, circles, ellipses and
  even-odd polygons, outlined or filled, plus text, canvas-to-canvas copies
  with format conversion, and alpha blending. Canvases come in RGBA, BGRA,
  big-endian RGB565, 8-bit gray and 1-bit formats, clip every call, and
  track the rectangle each call changed, so a display can be sent only what
  changed. Rendering is integer-only and gives the same pixels everywhere.
  `std/graphics/font` adds 6x8 and 8x16 bitmap fonts for ASCII and Latin-1,
  `std/graphics/image` decodes PPM/PGM, QOI and BMP and encodes PPM, QOI
  and BMP into caller buffers, and the `graphics.Display` protocol with the
  in-memory `std/graphics/fake` display lets drawing code be tested under
  `dodo test`. Golden images in `tests/graphics/golden/` pin the output.
- PIO on the RP2040 and RP2350: `std/embedded/chip/rp_pio` drives the chips'
  programmable I/O blocks, and `rp_pio.assemble` assembles PIO programs
  when the firmware is compiled, from string literals in the language of the
  Pico SDK's pioasm, so SDK `.pio` programs work unchanged and a mistake is a
  compile error at its line. The assembler exists only in firmware for these
  chips and checks each program against the chip's PIO version, so the
  RP2350's additions (`mov pindirs`, RX FIFO registers, neighbouring blocks'
  IRQ flags, `wait jmppin`) are errors on the RP2040. `pins.pio(n)` claims a
  block and `pins.pio_pins` hands it GPIOs; a block loads programs into its
  instruction memory, relocating their jumps, and hands out its four state
  machines, which are configured from the program's directives, take and
  give words through their FIFOs, execute single instructions, and raise
  the block's interrupts. `examples/ws2812.dodo` drives WS2812 LEDs, and the
  board self-tests gain three PIO checks.
- The RP2040 and RP2350 run code on their second core. `b.core1.start(&mut
  stack, task)` runs a task value's `run(self)` method on core 1, with the
  task and the stack in memory the program provides, until the returned
  handle is dropped or stopped, which holds core 1 in reset again. Tasks are
  checked like `std/thread` tasks, so they own what they hold and carry no
  references. On the RP2350 the stack's bottom is core 1's stack limit, and
  core 1 enables its FPU and double-precision coprocessor as core 0 does.
  `b.fifos.claim()` hands out each core's end of the hardware FIFOs between
  the cores, and `b.spinlocks.claim(n)` the hardware spinlocks, whose guard
  also masks interrupts on the holding core. On the RP2350, the spinlocks that
  erratum RP2350-E2 can unlock are never handed out. Each core can run its own
  `std/embedded/executor`: signals now belong to the core that raises and
  takes them, and `fifo.set_signal(&signal)` wakes a task when the other core
  writes, through the chip runtime's default FIFO interrupt handler. The new
  `cpu.core_id()` intrinsic returns the executing core's number: `mhartid` on
  RISC-V, the chip runtime's `dodo_core_id` on a Cortex-M, and 0 on hosted
  targets. `examples/two_cores.dodo` searches for primes on core 1 and reports
  them from core 0, and the board self-tests gain three checks of core 1, the
  FIFOs, and spinlocks.

- New portable compression packages read and write DEFLATE
  (`std/compress/deflate`), zlib (`std/compress/zlib`), and gzip
  (`std/compress/gzip`) through caller-supplied buffers, without allocating.
  `encode(input, output, level)` compresses at levels 0 through 9 with lazy
  LZ77 matching and optimal Huffman codes, choosing the smallest of stored,
  fixed, and dynamic blocks; its output matches zlib's size, and
  `max_encoded_len` bounds it. `decode` checks the Adler-32 or CRC-32 and
  stops after one stream or gzip member. Each package also has incremental
  `Decoder` and `Encoder` types for buffers of any size, gzip headers can
  carry a name, comment, and time, and `std/compress/io` adapts all three
  formats to `std/io` readers and writers.
- Records larger than 1 KiB passed, returned, or moved by value compile as
  memory copies at every optimization level. Before, LLVM split them into
  one operation per element: a 256 KiB struct took minutes and gigabytes of
  memory to build at `-O 2` and needed about 10 MB of stack at `-O 0`. Root
  package public functions now pass such records through memory like other
  Dodo functions.
- Ownership is checked on each function's control-flow graph, for every body
  the compiler accepts: initialization, moves, borrows, escapes, destructor
  order, and Result obligations. The earlier checker that ran beside it, and
  covered every body the graph could not yet represent, is gone. Liveness now
  follows each binding exactly: a borrow ends at its last use even across
  shadowing and loops, and a conflict names the binding that holds the
  borrow and points at its next use. Some programs the old checker rejected
  because of its approximations now compile, such as a mutable slice
  `&mut a[..]` iterated and written in place. Some errors are reported
  differently: type errors come before ownership errors, and a use after a
  move into a value block is now reported as one.
- A cryptography package. `std/crypto/random` reads the operating system's
  CSPRNG (`getrandom`, `SecRandomCopyBytes`, `BCryptGenRandom`) with no
  user-space generator or fallback. It fills buffers, draws unbiased values
  below a bound, and provides `SystemRandom`, a source for
  `hash.obtain_key`. `std/crypto/openssl` wraps OpenSSL 3.5+ libcrypto:
  incremental SHA-1 and SHA-256, HMAC-SHA256 with constant-time `verify`,
  AES-128-CFB8 encryptors and decryptors for streams split at any byte, and
  RSA key generation with PKCS#1 v1.5 encryption and decryption (OpenSSL's
  implicit rejection on) plus SubjectPublicKeyInfo DER export and import.
  `std/crypto` holds the shared error type, constant-time `equal` and
  `wipe`, and stays portable. Entropy-only programs link no OpenSSL, and the
  libcrypto backend links no libssl. Published FIPS, RFC 4231 and NIST
  vectors, plus RSA exchanges in both directions with the `openssl` command,
  check it at `-O0` and `-O3`.
- Native adapter areas may be nested, as in `std/crypto/openssl/native`.
- `forbid-unsafe = true` in a manifest's `[project]` table rejects every
  `unsafe` keyword in the project's own sources, including imported local
  packages and `dodo test` documentation examples. The standard library and
  board and chip packages are exempt. Build, check, run, and test stop with
  every location, and the language server reports each one.
- `@unsafe_send` and `@unsafe_sync` now require the struct to be declared
  `unsafe` (`@unsafe_send pub unsafe struct Owner`), since they are unchecked
  promises; `unsafe` on a struct without either contract is an error. Add
  `unsafe` to existing annotated structs.
- `std/signal` delivers `SIGINT`, `SIGTERM` and `SIGHUP`, and Ctrl-C,
  Ctrl-Break and console close on Windows, into a program's poll loop:
  `signal.listen` queues each selected signal in arrival order instead of
  letting it end the process, and the listener registers a wake channel in a
  `std/net/native` `PollSet` next to the program's sockets. A listener is
  also a cancellation source, so SIGTERM ends `reactor.serve_with` with its
  report. The wake channel is a self-pipe on Linux and macOS (signalfd would
  block the signals, and child processes would inherit that mask) and a
  loopback socket pair on Windows. `PollSet.add_raw` registers other
  descriptors that a program owns.
- `f64` arithmetic on the RP2350 runs on its double-precision coprocessor
  (DCP), with no change to programs: division takes about 90 cycles instead
  of 1300, addition about 45 instead of 260, multiplication about 60 instead
  of 200, and conversions between `f64` and `f32` or 32-bit integers a
  third to a tenth of their software time. Results are bit for bit those of
  the software helpers, subnormals included: the cases the coprocessor
  flushes to zero are computed in software, and NaN results stay the default
  NaN. The helpers save and restore the coprocessor's state when one
  interrupts another, so `f64` stays safe in interrupt handlers.
  `scripts/test_rp2350_float.py` compares them with the software helpers on
  a Pico 2 or Pico 2 W, on 1.8 million edge-weighted operands while
  interrupts preempt them, and times both.
- DMA on the RP2040 and RP2350: `b.dma.channel()` claims one of the chip's
  12 or 16 channels, `claim(n)` a given one, and `release(channel)` gives it
  back. `channel.copy` copies buffers, and `channel.set_signal(&SIGNAL)`
  wakes executor tasks when a transfer ends, from a default `DMA_IRQ_0`
  handler in the chip runtimes. The driver, `std/embedded/chip/rp_dma`,
  finds each peripheral's data request from the register a transfer reads or
  writes, and the panic report aborts every claimed channel first.
- `std/embedded/hal` gains `DmaChannel`, the protocol through which
  peripheral drivers start DMA transfers (`DmaTransfer`, `DmaWidth`,
  `DmaPace`, `DmaRing`), and `address_of` for a buffer's address.
- SPI by DMA: `bus.dma(&mut tx, &mut rx, &mut buffer)` returns a
  `pl022.DmaBus` whose `start_transfer` and `start_write` return at once and
  run without gaps between frames while the program or other tasks work.
  The session lends its buffer out only between transfers, so the borrow
  checker keeps the program off memory the channels are moving, and
  dropping it aborts a running transfer.
- Serial ports by DMA: `port.dma(&mut rx, &mut ring, &mut tx, &mut output)`
  returns a `pl011.DmaPort`, a `std/io` Reader and Writer that receives into
  a ring buffer of up to 32 KiB, so input is no longer limited to the
  32-byte FIFO between reads, and writes in the background.
- Polled I2C transactions: the DesignWare controller implements
  `hal.I2cPoll`, whose `poll_write`, `poll_read`, and `poll_write_read` run
  transactions of any length across polls; `listen` raises the controller's
  interrupt whenever a poll can make progress, and `cancel` aborts one.
- `examples/dma.dodo` runs a DMA serial console that polls ten times a
  second next to a task streaming SPI frames by DMA. The board self-tests
  check DMA copies, SPI and UART loopback by DMA, and queued I2C reads, and
  record up to 128 checks.

- The RP2040 and RP2350 boards are USB serial ports on their own USB
  connector: `b.usb.serial()` returns a CDC-ACM port with the `std/io` Reader
  and Writer protocols, so `fmt.printf` prints over the USB cable with no
  adapter. The port is served from `USBCTRL_IRQ`, so it keeps answering the
  host while the program busy-waits. Output written before a terminal opens
  the port waits for it, up to 1 KiB, and printing never blocks the program
  for long or fails. Newlines go out as CR LF, `set_signal` wakes executor
  tasks on input, and a panic is reported on the port, which the default
  panic hook keeps serving after it halts. The driver,
  `std/embedded/chip/rp_usb`, keeps its queues in the controller's own RAM
  and runs against a model of the controller and a simulated host in the
  desktop tests.
- `dodo run` no longer needs BOOTSEL for firmware with a USB serial port:
  when no bootloader drive is mounted, it finds the board's port on Linux
  and macOS, sets it to 1200 baud, which restarts the board into its
  bootloader, waits for the drive, and flashes.
- Fix `dodo fmt` dropping the space after a compound assignment before a
  parenthesis or bracket, which turned `x |= (y)` into `x |=(y)`.
- Fix `dodo fmt` spacing in array types and lengths: `[0u8; 16 * 16]` no
  longer becomes `[0u8; 16 *16]`, `[N - 1]u8` no longer becomes `[N -1]u8`,
  `[0u8; (N) * 2]` keeps the space after `;`, and `[3]&mut T` no longer gains
  a space before `&`.
- Fix `set_break(false)` on PL011 serial ports, which waited for the
  transmitter to go idle while the break itself kept it busy, and so never
  returned.
- Constants and statics get their own ELF sections, as functions already
  did, so the linker drops the unused ones: firmware no longer carries every
  imported package's public constants once it uses one string constant.
- `std/sync/atomic` works on Arm Cortex-M. On cores with exclusive loads and
  stores (ARMv7-M, ARMv7E-M, and ARMv8-M, such as the RP2350's Cortex-M33)
  every operation lowers to native instructions for 8-, 16-, and 32-bit
  integers. ARMv6-M (Cortex-M0 and M0+, as in the RP2040) has atomic loads and
  stores only; `exchange`, `fetch_add`, and `compare_exchange` there are a
  build error naming the reason. An operation a target cannot perform is now
  diagnosed only where the program can reach it, so packages that merely
  contain one build for every target.
- Atomics in statics are shared without `unsafe`:
  `static TICKS: atomic.Atomic<u32> = atomic.Atomic.new(0u32)` is placed in
  writable memory, and interrupt handlers, threads, and the main program call
  its methods directly, without a critical section. A static initializer may
  call a constructor, a safe function whose body only returns a struct
  literal of its parameters and constants, with constant arguments. A
  `const` cannot hold an atomic, since each use of a constant is a copy.
- Add PWM and ADC protocols to `std/embedded/hal`: `PwmOutput` (`max_duty`,
  `set_duty`) with `set_duty_percent`, `set_duty_fraction`, and
  `set_pulse_us` helpers, and `AnalogInput` (`max_value`, `read`) with
  `read_millivolts`. `std/embedded/hal/fake` gains `Pwm` and `Analog`, so
  drivers for LEDs, motors, servos, and analog sensors are tested on a
  desktop. `examples/dimmer.dodo` is a potentiometer-controlled LED dimmer,
  tested with fakes, and `examples/dimmer_board.dodo` runs it on a board.
- The RP2040 and RP2350 chip packages drive PWM and the ADC.
  `pins.pwm(pin, hz)` claims any GPIO as a PWM output at the frequency
  closest to `hz`, with as many duty steps as it allows (up to 65535), and
  `pins.pwm_divided(pin, divider, top)` takes the slice's divider and TOP
  instead. The two outputs of a slice share its frequency and write only their
  own bits of its registers. `pins.analog(pin)` claims GPIO 26 to 29 as 12-bit
  ADC inputs with polled reads, and `pins.temperature()` the on-chip
  temperature sensor, read with `rp_adc.millicelsius`. The ADC's 48 MHz clock
  runs from PLL_USB while an input is claimed. Both drivers,
  `std/embedded/chip/rp_pwm` and `rp_adc`, run against simulated blocks in the
  desktop tests, and the board self-tests check them with nothing connected.
- Add `std/embedded/executor`, a cooperative task executor for firmware.
  A task is any value with `poll(&mut self, cx: &mut executor.Context) ->
  executor.Poll`; the executor polls each task only when a time it asked for
  (`cx.wake_at`, `executor.Ticker`, `executor.Timer`) arrives or a signal it
  took is raised, and sleeps the core in between. `executor.Signal`s,
  numbered by the program, are raised safely from interrupt handlers and
  tasks, count raises, and carry a `u32` value. Up to 32 tasks and 32
  signals; no allocation. The RP2040 and RP2350 timer alarms gain `now_us`
  and `sleep_until`, which make an alarm the executor's clock: it sleeps
  with `wfi` until its deadline or any interrupt, and needs no handler of
  its own. `std/embedded/hal/fake` gains `Clock`, a simulated clock that
  raises signals as simulated interrupts, so tasks are tested on a desktop,
  and `Serial.poll_read`. `examples/tasks` runs a blinking LED, a debounced
  button, and an interrupt-driven serial console as three tasks.
- RP2040 and RP2350 pins raise `IO_IRQ_BANK0` on edges and levels:
  `pin.listen(chip.RISING_EDGE | chip.FALLING_EDGE)`, `events()`,
  `clear_events(edges)`, `ignore(events)`, and `irq()`.
- Add a [JetBrains plugin](editor-support/dodo-jetbrains) for RustRover, CLion,
  and other JetBrains IDEs from 2026.2. It connects to `dodo lsp` like the VS
  Code extension and offers the same editing features, with live templates
  generated from the extension's snippets. Releases include
  `dodo-jetbrains-<version>.zip`.
- Add `@interrupt(VECTOR)` handlers. `@interrupt(UART0_IRQ) fn received()`
  is exported under the vector's name with the target's interrupt-entry
  convention (the C ABI on Cortex-M, `mret` on bare-metal RISC-V), so it
  replaces the runtime's default in the vector table. With a board or chip
  selected, the compiler checks the name against the chip's vector table and
  suggests the closest one, so `TIMER_IRQ0` is an error instead of a handler
  that never runs. Code cannot call a handler or take it as a function value,
  a vector has one handler, and no C function may take its symbol.
- Add `@section("name")` and `@align(N)` for functions and statics. A
  placed definition is kept even when nothing refers to it. The RP2040 and
  RP2350 linker scripts copy `.ramfunc` sections to SRAM at reset and keep
  `.noinit` statics in SRAM without loading or zeroing them, so they survive
  a reset.
- Add inline assembly: `asm("template", ..., in(reg) value, out(reg) Type,
  inout("r0") value, clobber("r4"), options(pure, nomem))` in unsafe code.
  The template names operands `{0}`, `{1}`, ...; the expression returns
  nothing, the output, or a tuple of outputs. Templates are a memory barrier
  unless `options(nomem)`, and the assembler's errors are compile errors
  instead of aborting the compiler.
- Add `std/embedded/cortex_m`: the NVIC (`enable`, `disable`, `pend`,
  `unpend`, `set_priority`, ...), SysTick, the vector table base, PendSV,
  and `wfe`, `sev`, `dsb`, `dmb`, `isb`, PRIMASK, and IPSR. The RP2040 and
  RP2350 chip packages name every IRQ number after its vector
  (`rp2040.UART0_IRQ`), and their timers hand out `Alarm`s that schedule a
  one-shot interrupt. PL011 serial ports gain `listen`, `ignore`,
  `pending_interrupts`, and `clear_interrupts` for their receive, receive
  timeout, transmit, and error interrupts. `examples/interrupts` is an
  interrupt-driven serial echo for every board.
- Borrowing one static no longer conflicts with reading another static or a
  constant: `match &mut PORT { some(port) => port.clear_interrupts(pl011.ERROR_INTERRUPTS) ... }`
  is accepted, and conflicts on the same static name it.
- `?` converts errors into the enclosing function's error enum. When a
  function returning `T!AppError` propagates an `io.Error`, and `AppError`
  has exactly one variant whose only payload is an `io.Error`, such as
  `Io(io.Error)`, `?` returns the error wrapped in that variant. Code that
  mixes stream and driver errors declares
  `enum LogError { Sensor(hal.Error), Output(io.Error) }` and uses `?` on
  both instead of matching every call. The conversion calls no function and
  moves the error with its ownership and borrows; a missing or ambiguous
  variant is reported with the candidates. `std/text_alloc`,
  `std/text_shared`, and `std/fmt_alloc` use it internally. See
  `examples/sensor_log.dodo`.
- Fix `?` accepting a propagated error that borrows a local value or a
  source outside the function's `from(...)` contract. The error path is now
  checked like an explicit `return`.
- Add function values and closures. `fn(A, B) -> R` is a function pointer
  type: a named function, a method (`Point.length`), an imported function,
  or a generic function specialized from the expected type can be stored in
  locals, fields, arrays, Options, constants, and statics, and called like a
  function. A closure literal, `fn(a, b) { a < b }`, takes omitted parameter
  and result types from where it is passed and may use local variables of the
  surrounding code. Closures capture by reference and are checked like
  references to what they capture: a closure that only reads its captures is
  a copyable `&dyn fn(..)`, one that changes them an exclusive
  `&mut dyn fn(..)`, and a closure that captures nothing is a plain function
  pointer. While a closure can still be called, its captured variables keep
  the matching borrow, so it can never outlive them or alias a mutation.
  Function pointers convert to closure references automatically. Calls work
  on any callee expression, such as `(table[i])(x)` and `button.on_press(x)`.
- `std/collections` gains closure forms of its algorithms: `sort_by`,
  `stable_sort_by`, `is_sorted_by`, `binary_search_by`, `partition_by`,
  `position`, `any`, `all`, `count`, `for_each`, `for_each_mut`, `map`, and
  `fold`.
- `std/embedded/hal.Handler` holds an interrupt handler chosen at run time:
  a vector-table entry point calls `run`, and firmware swaps handlers with
  `set` and `clear` inside a critical section. Statics start empty with
  `hal.NO_HANDLER`.
- Static and constant initializers accept `none`, function names, and closures
  that capture nothing, and a constant or static holding a function value is
  called by name.
  Constants with private fields can be used outside their package, since
  their initializer is checked where it is declared.
- Add tuples. `(A, B)` is a type, `(a, b)` builds a value, and `.0`, `.1`
  read and assign elements, so a function can return several values without
  declaring a struct: `fn divmod(a: u32, b: u32) -> (u32, u32)`. Tuple
  patterns destructure in `let (q, r) = divmod(17, 5)`, `match`, `if let`, and
  loops: `for (id, score) in scores` borrows each element's fields and
  `for &(id, score) in scores` copies them. Tuples behave like structs with
  public fields `0`, `1`, ...: they move, own and destroy their elements, and
  borrows of different elements do not conflict.
- Add type aliases. `type Point = (i32, i32)`, `pub type Outcome<T> = T!Error`,
  and `type Grid<T> = [8][8]T` give existing types another name; an alias is
  interchangeable with its type. An alias of a struct or enum also works in
  struct literals and patterns, variant paths, and `Alias.new()` calls.
  Private aliases stay in their package, and a public alias cannot expose a
  private type.
- Diagnostics name specialized generic types and tuples in source notation,
  `Pair<i32, bool>` and `(i32, bool)`, instead of internal mangled names; so
  do hovers and debugger type names.
- `type` is now a reserved word.

- Add interfaces. `interface Name { fn method(&self) -> T }` declares method
  signatures that any type with matching methods implements; an interface
  can include others (`interface Sensor: Named`). Generic parameters take
  bounds, `fn read_line<S: io.Reader + io.Writer>(...)` and
  `struct Holder<S: Sensor>`, which are checked where the generic is used:
  a wrong type is reported at the call, with each missing method or
  mismatched signature, instead of inside the specialized body. A struct may
  state `struct Uart: io.Reader + io.Writer` to have its conformance checked
  where it is declared. Bounded generics are still specialized per type, so
  they cost nothing at run time.
- Add `dyn` references for run-time dispatch: `&dyn Sensor` and
  `&mut dyn Sensor` refer to any value implementing `Sensor` together with a
  constant method table, so different devices fit in one array, a struct
  field, or a driver chosen at run time. References convert implicitly where
  a `dyn` reference is expected (or with `as`), keep their borrows, and
  convert to an interface's first parent. `dyn` types satisfy their own
  bounds, so one specialization of a bounded generic can serve every type.
- `std/io` declares `Reader`, `Writer`, and `ReadWriter`; its helpers
  (`read_exact`, `write_all`, `copy`, `BufferedReader`, ...) are bounded by
  them. `std/embedded/hal` declares `OutputPin`, `InputPin`, `Delay`, `I2c`,
  `Spi`, and `Registers`, bounds its helpers and `SpiDevice`, and the
  peripheral drivers declare the interfaces they implement.
- Method calls reborrow through any number of references, so elements of an
  array of references call their methods directly.
- Add serial ports (UART) to the `rp2040` and `rp2350` chips, so firmware
  can print. `pins.uart(tx, rx, baud)` validates the pins and the baud rate,
  takes the UART once, and returns a port implementing the `std/io` Reader
  and Writer, so `fmt.printf(&mut serial, ...)` and `io.read_line` work on
  it unchanged; `rx` is `none` for a port that only sends. Every board names
  its usual pins, `UART_TX_PIN` (GP0) and `UART_RX_PIN` (GP1), where the
  Raspberry Pi Debug Probe connects. The driver is the new
  `std/embedded/peripheral/pl011` (Arm PL011): 8N1 by default, `set_baud`
  and `set_format` for 5 to 8 data bits, parity, and 1 or 2 stop bits,
  polling `poll_read` and `poll_write`, `flush`, internal loopback, and
  `set_crlf(true)` for terminals, which writes `\n` as CR LF and reads Enter
  as `\n`. Framing, parity, break, and overrun errors fail a read after the
  good bytes before them, with the error's bits in `io.Error.code`.
  Waits for output give up with `TimedOut` after a poll limit
  (`set_poll_limit`), and reads can be bounded the same way
  (`set_read_poll_limit`).
  `pins.release_uart(port)` gives a port back. `examples/serial_console.dodo`
  is a small command line.
- Report panics on the serial port. The chip runtimes' default panic hook,
  `dodo_board_panic`, now calls a reporter that `std/embedded/chip` links
  in, which writes `dodo: index bounds check failed at main.dodo:41:46`, the
  line a desktop program prints, to every serial port the program holds
  before it halts. Nothing needs configuring, a program's own
  `dodo_board_panic` still replaces the hook, and `board.report_panic`
  writes the same report from it.
- I2C reaches 10-bit addresses: `write_ten_bit`, `read_ten_bit`, and
  `write_read_ten_bit` on a `designware_i2c` bus. `probe(address)` finds
  devices with a one-byte read, since the controller cannot send an address
  alone.
- The RP2040 and RP2350 can be I2C and SPI targets (slaves).
  `pins.i2c_target(sda, scl, address, ten_bit, hz)` answers a master at a
  7-bit or 10-bit address; `poll` reports the bytes it writes, its STOPs,
  and its reads, which hold the master up until `respond` answers.
  `pins.spi_target(clock, tx, rx, select, mode)` follows a master's clock
  and chip select, with polled `queue` and `read` and a waiting `transfer`.
- SPI frames of 4 to 16 bits (`set_frame_bits`, with `transfer_words` and
  `write_words` on `u16` words above 8 bits), the TI synchronous serial and
  National Microwire formats (`set_frame_format`), and
  `pins.spi_with_select`, which routes the controller's own chip select.
- Serial ports gain hardware flow control: `pins.uart_flow(tx, rx, cts,
  rts, baud)` claims the CTS and RTS pins, and RTS asks the other end to
  pause from 24 received bytes on. `set_break` holds the line low. On the
  RP2350, `pins.uart` also takes TX and RX on the UART_AUX pins (GPIO n
  with n % 4 of 2 and 3).
- Fix `set_break(false)` waiting for the UART to go idle, which a break
  prevents: ending a break now returns at once.
- `std/embedded/hal/fake` gains `fake.Serial`, an in-memory serial port for
  testing drivers of devices on a serial line. The board self-tests check
  the serial port's pins, ownership, timing at 115200 baud and 1 Mbaud,
  newline translation, and overruns through the internal loopback, and
  desktop tests run the driver against a PL011 model. The self-tests also
  cover 10-bit addressing, both targets, 12-bit and TI frames through the
  SPI loopback, breaks, and the flow control pins, and the desktop models
  gain a DW_apb_i2c target driven by a scripted master.

## 0.1.5 — 2026-10-12

Dodo 0.1.5 brings the compiler and hosted standard library to macOS and AArch64
Linux, and adds firmware builds for the Raspberry Pi Pico, Pico 2, and Pico 2
W, with GPIO, timers, I2C, and SPI behind a portable hardware abstraction
layer. Releases now ship x86-64 and AArch64 Linux, Apple silicon macOS, and
x86-64 Windows archives with checksums and build provenance attestations. The
language specification remains version 0.1.

- Support macOS in the hosted standard library: console, files,
  environment, processes, threads, synchronization, clocks, sockets, TLS
  and HTTP/web, on Apple silicon (`aarch64-apple-darwin`, tested natively)
  and Intel (`x86_64-apple-darwin`, built and linked). macOS uses
  libSystem: `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`,
  `SO_NOSIGPIPE` sockets, `renamex_np` for no-replace renames, and relative
  condition-variable waits on the monotonic clock. Apple arm64 calls now
  extend narrow integer arguments as Apple's ABI requires, atomics accept
  LLVM's `arm64` spelling, and links pass `-arch` so
  `--target x86_64-apple-darwin` works on Apple silicon. TLS programs find
  Homebrew's OpenSSL 3 automatically. CI builds, tests and packages a
  self-contained macOS compiler that needs only system libraries.
- Support AArch64 Linux (glibc). CI builds, tests and packages the compiler
  on native AArch64 runners.
- Linux and macOS share one hosted adapter, `std/AREA/posix`; the C
  boundary now owns every OS- and architecture-specific detail (`struct
  stat` and `struct dirent` layouts, `O_*` flags, errno numbering and
  access, SIGPIPE handling). This fixes `O_NOFOLLOW`, whose value differs
  on AArch64. `std/AREA/native` is unchanged; `std/AREA/linux` and the new
  `std/AREA/macos` select the POSIX adapter on their own OS.
- `sync.Storage` grows from 256 to 384 bytes so that it holds macOS's
  pthread state; allocated synchronization objects reserve a 512-byte
  header. `EILSEQ` (APFS rejecting a non-UTF-8 file name) maps to
  `InvalidInput`.
- Add I2C and SPI buses to the `rp2040` and `rp2350` chips, so portable
  drivers run on a board unchanged: `examples/hal_sensor_board.dodo` runs
  the `examples/hal_sensor.dodo` driver on a board's I2C bus. `pins.i2c(sda,
  scl, hz)` and `pins.spi(clock, tx, rx, mode, hz)` validate the pins and
  frequency, take the controller once, and return buses implementing the
  `std/embedded/hal` I2C and SPI protocols; every board names its usual bus
  pins (`I2C_SDA_PIN` and friends). The controllers are licensed designs, so
  their drivers live once in the new `std/embedded/peripheral`:
  `designware_i2c` (Synopsys DW_apb_i2c, 7-bit master up to 1 MHz, with
  bounded waits) and `pl022` (Arm PL022 SPI master, all four modes,
  internal loopback), whose `Bus` types `pins.i2c` and `pins.spi` return;
  the pin assignments and the pin and controller ownership both chips share
  live in `std/embedded/chip/rp_pins`. `pins.release(pin)`,
  `pins.release_i2c(bus)`, and `pins.release_spi(bus)` give pins and buses
  back, so a stuck I2C bus can be clocked free by hand. The SPI clock is the
  fastest at or below the request, searching every prescaler.
  Both drivers run against any register block, the new `hal.RegisterBlock`
  on a chip or a simulated controller in desktop tests, which model FIFO
  pacing, aborts, and stuck buses. `hal` gains `SpiMode` and
  the `I2C_STANDARD_HZ`, `I2C_FAST_HZ`, and `I2C_FAST_PLUS_HZ` constants, and
  the board self-tests check both buses with nothing attached.
- Add firmware builds for microcontroller boards, starting with the Raspberry
  Pi Pico. `board = "pico"` in a `dodo.toml` target (or `--board pico`) selects
  the board's chip: its target and CPU, a startup runtime the compiler builds
  itself (boot stage 2, vector table, reset handler, memory and integer
  helpers), a linker script, and `ld.lld`. Builds write a flashable `.uf2`
  next to the `.elf`, and `dodo run` copies it to a board in BOOTSEL mode.
  `chip = "rp2040"` (or `--chip`) builds for a custom board.
- Add `import "std/embedded/board"` and `import "std/embedded/chip"`, which resolve to the
  packages of the build's board and chip, so firmware is not tied to one
  board. Every board package provides `take()`, an LED, GPIO pins, a timer,
  and `reboot_to_bootloader()`. Editors follow the manifest's board.
- Add `std/embedded/chip/rp2040` (clocks from a configurable crystal, 125 MHz system
  PLL, GPIO pins implementing the `std/embedded/hal` pin protocols, the microsecond
  timer, and the boot ROM's USB bootloader) and `std/embedded/board/pico`.
- Add the Raspberry Pi Pico 2: `board = "pico2"` on the new `rp2350` chip
  (Arm Cortex-M33 cores, `thumbv8m.main-none-eabi`). The compiler's RP2350
  runtime provides the vector table, the `IMAGE_DEF` block the boot ROM
  requires, and a reset handler that enables the FPU; images use the
  `rp2350-arm-s` UF2 family and flash to the `RP2350` drive.
  `std/embedded/chip/rp2350` runs the system clock at 150 MHz and provides
  GPIO pins, the TIMER0 microsecond timer, and the boot ROM's USB bootloader;
  `std/embedded/board/pico2` follows the board contract, so `examples/blink`
  builds unchanged with `--board pico2`. The memory and integer helpers are
  now shared by both Arm chip runtimes.
- Add the Raspberry Pi Pico 2 W: `board = "pico2_w"` on the `rp2350` chip.
  Its LED hangs off the CYW43439 wireless chip, so the new
  `std/embedded/wireless/cyw43` driver powers the chip up, bit-bangs its
  half-duplex gSPI bus, starts its backplane clock, and drives its GPIO pins
  through ChipCommon and the GCI pin multiplexer, without loading wireless
  firmware.
  `std/embedded/board/pico2_w` follows the board contract, so
  `examples/blink` builds unchanged; its LED also reads back the pin level
  and senses USB power. RP2040 and RP2350 pins gain `set_as_output` and
  `set_as_input` for lines that change direction. Wi-Fi and Bluetooth are
  not supported yet.
- Floating point works in firmware. The shared Arm runtime gains IEEE 754
  `f32` and `f64` addition, subtraction, multiplication, division, `%`,
  comparisons, and conversions to and from integers and between the two
  widths, correctly rounded to nearest with subnormals, under their GNU and
  Arm EABI names. The RP2040 uses all of them; the RP2350's single-precision
  FPU uses the `f64` ones and the conversions. A host test checks every helper
  against hardware floating point on 1.5 million operands, and the board
  self-tests check them on the chips.
- Add the `examples/blink` project and a hardware self-test,
  `scripts/test_pico.py`, that checks startup, clocks, timing, ownership, and
  the runtime helpers on a connected Pico (`--board pico2` for a Pico 2,
  `--board pico2_w` for a Pico 2 W, which also checks the wireless chip's
  LED and USB-power GPIOs).
- Add `std/embedded/hal`, a portable hardware abstraction layer: typed volatile
  registers (`hal.Reg<T>`) with mask-constant fields, interrupt-masking
  `hal.Critical` sections, one shared `hal.Error`, statically checked pin,
  delay, I2C, and SPI protocols with register and chip-select helpers, and
  serial ports through the existing `std/io` protocols.
- Add `std/embedded/hal/fake` in-memory pins, delays, and I2C/SPI buses so drivers run
  under `dodo test` on a desktop.
- Add the `core/cpu` intrinsics `fence`, `disable_interrupts`,
  `restore_interrupts`, and `wait_for_interrupt` (spec CORE-CPU). They lower to
  PRIMASK on bare-metal Cortex-M and `mstatus.MIE` on bare-metal RISC-V, are
  no-ops on hosted targets, and are rejected on other targets only when
  reachable. `cpu` is now a reserved package name.
- Rework the GPIO example into RP2040 chip support built on `std/embedded/hal`, add a
  desktop-testable I2C sensor driver example, and add the
  [hardware and embedded guide](https://jotrorox.github.io/dodo/hardware/).
- Mark every generated function `nounwind`. 32-bit Arm objects no longer
  reference `__aeabi_unwind_cpp_pr0`, which broke firmware linked without
  libgcc; hosted objects keep their unwind tables for debuggers and backtraces.
- Extend the control-flow initialization analysis to integer range loops,
  signed negation, arrays and slices (including `foreach` over them), enums,
  `Option` and `Result`, patterns, and `Result` exits, so it checks more
  function bodies. Only bodies that depend on unsupported constructs fall back
  to the existing checks.
- Fix `dodo fmt` gluing a prefix `!` to the preceding token (`if!x`,
  `return!x`, `f(a,!x)`). `T!E` error types and postfix unwraps stay tight.
- Fix the HTTPS client failing an already complete response when the server
  closes right after responding: a reset while sending the client's
  `close_notify` is now ignored. This made the hosted HTTPS integration test
  flaky on macOS.
- Add generated compiler safety tests and fuzz targets. CI now fuzzes on a
  schedule, tracks benchmark results, tests cold-cache builds, and runs the
  frontend tests on Apple silicon.
- Publish broader language-server latency measurements and shared-prefix web
  routing benchmarks.
- Publish GitHub Releases from `v*` tags. The release workflow checks the tag
  against `Cargo.toml`, the VS Code extension version, and `CHANGELOG.md`, then
  attaches the tested x86-64 and AArch64 Linux, Apple silicon macOS, and x86-64
  Windows archives, the `.vsix`, a `SHA256SUMS`
  file, and build provenance attestations.
- Shrink the release archives: the Linux and macOS archives are `.tar.xz`, release
  binaries no longer carry a symbol table, the Windows ZIP uses maximum
  compression, and the Linux archives include only the shared license texts
  its notices reference.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.4 — 2026-09-20

Dodo 0.1.4 adds optional project manifests, typed JSON codecs, editor inlay
hints and quick fixes, and a complete standard-library API reference. It also
improves compiler correctness and JSON and web performance. The language
specification remains version 0.1.

- Add optional `dodo.toml` manifests with named targets, profiles, saved run
  arguments, hosted test settings, and configuration inspection. Preserve
  standalone source compilation and the `compile` alias.
- Add an owned TOML 1.0 parser and shared CLI option definitions without adding
  dependencies. Improve command help, option syntax, and diagnostics; support
  `-b`, standalone `--release`, test debug information, and quiet/verbose output.
- Add non-overwriting project initialization and shell completion generation.
  Let the language server and VS Code select manifest targets and reload their
  platform settings when the manifest changes.
- Send build/check status to stderr and return exit 2 for CLI usage errors.
  Build/configuration/test failures retain exit 1; run preserves the child status.
- Add portable `std/encoding/json` codecs with `@derive(Json)` for structs,
  checked field conversion, field renaming, optional unknown-field rejection,
  borrowed values and strings, caller-owned output, and streaming I/O helpers.
- Optimize JSON validation, escaped strings, indexed object lookup, and derived
  array and struct decoding. Add repeatable benchmarks and recorded results.
- Reuse web routing metadata and validated paths, index dynamic route prefixes,
  preserve captured parameters, and scan HTTP headers sequentially. Expand
  routing regression tests and loopback HTTP performance measurements.
- Add inferred local-type inlay hints and diagnostic quick fixes for mutable
  bindings and missing package imports. Use structured diagnostic data for
  fixes, and reject editor renames that would introduce name collisions.
- Fix definite initialization at unconditional loop exits, ownership checks,
  and short-circuit expression state. Extend control-flow analysis and use it
  for supported function bodies while retaining checks for other bodies.
- Make constant integer-to-`f32` conversions match runtime conversions and fix
  floating-point cast edge cases. Correct Windows child-process exit statuses
  and filesystem path discovery edge cases.
- Fix overlapping borrows when moving fixed-vector elements between slots,
  restoring checked vector insertion, removal, and collection mutation.
- Rework documentation into beginner guides and a separate reference. Add a
  generated API reference covering every bundled standard-library package,
  runnable examples, project-manifest guidance, and stronger documentation
  link and search checks.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.3 — 2026-09-16

Dodo 0.1.3 adds simpler printing and web application APIs, concurrent HTTPS,
checked mutable slice splitting, reference collections, and improved editor
navigation. The language specification remains version 0.1.

- Add postfix `!` to unwrap a Result or panic on error, preserving `?` for
  propagation. Shorten hello world to a single console call in `fn main()`.
- Add safe hosted console input/output and compiler-checked `print`, `println`,
  and `printf` for primitives and custom printable values. Check literal format
  strings and heterogeneous arguments at compile time.
- Add bounded UTF-8 console and file helpers, command output capture,
  environment conveniences, native clocks, and runnable standard-library
  onboarding guides.
- Add checked `core/slice.split_at_mut` with disjoint mutable views, nested
  splitting, and source lifetimes preserved through moves and returns.
- Support checked shared-reference collection elements and shared-arena owned
  strings. Add scoped `try_update` mutation for vector elements and hash-map
  values, retaining ownership and allocator dependencies on success or error.
  Result-bearing and exclusive-reference elements remain restricted.
- Fix ownership tracking across `break` and `continue`, including loop-carried
  borrows, moved values, and reachable exits.
- Add bounded hosted HTTP/HTTPS clients and fluent `app.new()` and portable
  `application.builder()` APIs. Provide method shortcuts, named handlers,
  middleware, route diagnostics, text/HTML/JSON/redirect responses, request
  accessors, explicit rejections, and owned in-process test responses.
- Add `std/web/app.Server` with bounded default storage, grouped limits and
  timeouts, custom buffers, cooperative cancellation, and explicit serial or
  concurrent execution. The bounded HTTP reactor supports connection reuse and
  ordered pipelined requests.
- Add `std/web/https` with bounded PEM loading, file-specific startup diagnostics,
  and `run_with(address, https.files(certificate, key))`. Support concurrent TLS
  handshakes and requests, HTTPS keep-alive, and graceful connection shutdown;
  TLS remains an explicit import.
- Keep large internal function arguments and results in caller-owned storage
  and compact aggregate copies before LLVM lowering, reducing memory use for
  composed web applications. Exported and C signatures stay stable. Allow up
  to 4,096 generic specializations with a separate nesting limit of 64.
- Open bundled standard-library definitions as read-only sources in VS Code,
  with hover and navigation inside them. Refresh diagnostics and navigation
  when imported files change, preserving unsaved buffers. Add printing
  completions, signature help, and snippets.
- Make frontend tests and Clippy runnable without LLVM through
  `--no-default-features`. Expand native Windows filesystem, process, console,
  thread, synchronization, and networking coverage at `-O0` and `-O3`. Give
  refused-connection checks a fresh deadline and actionable error diagnostics;
  separate HTTP protocol-error checks from short timeout-test deadlines.
- Migrate the backend from Inkwell to llvm-sys 231.0.0 and LLVM 23.1.1; source
  builds now use `LLVM_SYS_231_PREFIX`. Update Rust to 1.98.1, refresh Rust/npm
  dependencies, and adopt TypeScript 7. The VS Code extension requires
  VS Code 1.137 or newer.
- Slim compiler archives to the executable, installation instructions, and
  license notices; documentation and examples remain available online and in
  the source repository. Fix Windows LLVM support-library and linker handling.
- Remove `std/text_unicode` and its Unicode data license notice; text trimming
  uses `Text.trim_ascii`. Remove the Zed extension and Tree-sitter grammar.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.2 — 2026-09-14

Dodo 0.1.2 adds a portable and hosted standard library, native testing, source
debugging, richer editor support, and Windows compiler downloads. The language
specification remains version 0.1.

- Add portable core and explicit allocation packages: uninitialized storage,
  memory and slice utilities, caller-backed arenas and pools, and owned boxes.
- Add byte I/O, formatting, binary views, UTF-8 text, numeric parsing, and
  separately selected growing buffers and strings. Keep allocation explicit.
- Add portable collections, binary64 mathematics, hashing/checksums, and
  duration/calendar/clock contracts without requiring an OS, global allocator,
  or libm. Generate mathematical constants independently and use integer-based
  floating-point formatting.
- Add independently selected Linux/Windows filesystem, process, environment,
  native thread, and synchronization packages, with owned resources, bounded
  output collection, and deterministic guard/thread destruction.
- Add checked cross-thread transfer/sharing contracts, native callback
  specialization, and integer atomics with validated memory orderings and
  target capabilities.
- Add portable network addresses and DNS, Linux/Windows TCP/UDP/resolver
  providers, verified OpenSSL TLS, bounded incremental HTTP/1.1, streaming
  clients and servers, routing/middleware, and optional rooted static files.
  Transport, TLS, clocks, allocation, and execution remain independently selected.
- Add checked owner-bound storage views and shared arena capabilities. Preserve
  access modes through container borrowing, reject mutable reborrows through
  shared aggregates, and prevent callbacks from retaining borrowed request
  storage through external-reference assignments.
- Add `dodo test` with recursive discovery, `@test` and `test_` functions,
  companion test files, assertions, filtering, ignored tests, captured output,
  timeouts, and isolated native execution. Report assertion values and source
  locations, and execute Markdown fences marked `dodo test`.
- Add source debugging with `-g`, source breakpoints, and local variables.
  Report hosted runtime failures with their check and source location, and
  support non-returning board failure handlers through `--panic-hook`.
- Add LSP completion, definition, references, rename, signature help, canonical
  document formatting, multiple diagnostics, configured compilation targets,
  and a reproducible stdio responsiveness benchmark.
- Publish the Dodo VS Code extension with syntax highlighting, snippets, LSP
  integration, configuration, and restart/output commands as an installable VSIX.
- Use `main.dodo` in the current folder for `dodo run`, `dodo check`, and
  `dodo compile`; keep `build` as an alias for `compile`. Explicit project
  folders select `main.dodo`; default outputs use the project folder name.
  Shared code uses ordinary imported subfolders, without a manifest or package
  manager. Update existing scripts that relied on checking an entire directory
  as one CLI package.
- Share payload storage across enum and `Result` alternatives and initialize
  only the active payload. Compile explicit wrapping arithmetic directly,
  remove unreachable functions, and emit separate function/data sections.
- Specify implemented language contracts and implementation-defined choices,
  with expanded conformance tests and updated reference documentation.
- Replace external JSON, LSP protocol, and file-URI dependencies with internal
  implementations. Rename the Cargo package to `dodo`; the library remains `dodoc`.
- Publish x86-64 Linux and Windows compiler archives with embedded LLVM,
  examples, documentation, and dependency notices, including the embedded
  standard library's Unicode notice. The Windows build uses static runtimes.
  Remove the separate checksum manifest.
- Use the official LLVM 22.1.8 Linux archive without a Z3 dependency. Cache
  native build dependencies, retry downloads, and improve Windows extraction
  and release smoke tests.
- Expand verification with native tests and executable documentation at `-O0`
  and `-O3`, debugger and VS Code sessions, Windows/Wine programs, published hash
  vectors, MPFR references, deterministic models, parser mutation/fragmentation,
  independent network/TLS peers, and WebAssembly/Cortex-M0 object generation.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.1 — 2026-09-11

Dodo 0.1.1 improves language ergonomics, editor support, compiler distribution,
and documentation. The language specification remains version 0.1.

- Organize the documentation into getting started, usage, language reference,
  and project sections, with focused installation, command-line, editor, and
  source-build guides.
- Simplify the first-program tutorial and add a copyright and license footer
  throughout the documentation website.
- Add `dodo fmt` with automatic migration to name-first declarations, bracket
  array literals, and explicit `function::<Type>()` generic calls.
- Support inferred and annotated array literals, copy-only array repetition,
  immutable `let` bindings, value-producing blocks, integer ranges, checked
  subslices, and recursive patterns with guards and conditional bindings.
- Add explicit copy patterns in shared collection iteration and leading-dot
  continuation for field and method chains.
- Add `dodo lsp` / `dodo --lsp` with live diagnostics, ownership and type hovers,
  unsaved import checking, and directory-package checking.
- Improve labeled ownership diagnostics for borrow origins, conflicting
  accesses, moves, live uses, and borrowed-return contracts.
- Embed LLVM 22 and all code generation targets in the compiler; provide Linux
  release recipes with bundled support libraries and a fully static profile.
- Fix fully static Ubuntu builds when LLVM reports absolute support-library
  paths, and provide the required static Z3 archive through a pinned build.
- Publish the searchable documentation website with light and dark themes,
  keyboard navigation, and generated PDF and plain-text specification downloads.
- Read the CLI help version from Cargo metadata so help, version output, and
  the language server identify the same compiler release.

The compiler still implements a subset of the language design. See the
[implementation status and limits](https://jotrorox.github.io/dodo/implementation/)
for supported behavior and remaining work.

## 0.1.0 — 2026-09-10

Initial Dodo compiler release, with ahead-of-time LLVM code generation, checked
ownership and borrowing, local packages, native execution, and a published
language specification.
