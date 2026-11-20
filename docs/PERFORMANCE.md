# Performance in Bare86

How Bare86 runs DOS programs fast, where the time goes, and how to measure
it. The design journal ([JOURNAL.md](JOURNAL.md), in French) holds the
detailed measurements behind each step; this page is the overview.

Figures below were measured on a Ryzen AI 9 HX 370 under Linux, pinned to
the same cores (`taskset -c 0-3`).

## 1. Native execution, and what it cannot do

The guest runs on the real processor under KVM: real mode, 16-bit and
32-bit protected mode, DOS extenders, all native. An instruction costs what
it costs on the host. Memory that behaves like plain memory (conventional
RAM, extended memory, EMS pages, the linear framebuffer of VESA modes, the
text screen, mode 13h) is mapped straight into the guest and never leaves
it.

What KVM cannot run natively is **device emulation**. Every access to an
I/O port, and every access to memory that is not plain RAM, stops the guest
(a *VM exit*), switches to Bare86, which emulates the device, then resumes
the guest. A round trip costs **about 4 µs**, several thousand
instructions' worth. A program that touches a device a few thousand times
per second does not notice; one that does it millions of times does.

The worst case of the era is **planar VGA** (16-colour modes and "mode X",
used by Doom, Rise of the Triad, Wolfenstein 3D, many demos). Video memory
is split into four planes behind a single 64 KB window, and the card
combines them through latches, bit masks and logical operations. No
memory mapping can reproduce that. Doom also selects the plane for each
column of the screen it draws (an `out` to port 3C5h): about 1,400 times
per frame. Without help, about two thirds of Doom's time went into these
exits.

## 2. Mappings and bursts: fewer exits

- **Plane mappings.** When a program writes to only one plane at a time
  in the simple write mode, the window is mapped onto that plane and
  accesses stay native. Changing plane swaps the contents of a "stage"
  region (two 64 KB copies) instead of remapping guest memory, which would
  cost a system call and page faults.
- **Interpreter bursts.** When an exit cannot be avoided (a planar access
  with latches, a burst of port writes such as a palette upload), Bare86
  does not resume the guest after one instruction. A small x86
  interpreter in the host (`src/interp/`) runs the following instructions
  itself, serving the device accesses directly, until the code stops
  touching the device. One exit then covers hundreds of accesses. Quake's
  `timedemo`, dominated by palette uploads, went from about 330 to about
  970 frames per second this way.
- **Coalesced writes.** Some port writes need no answer and nothing
  depends on them right away (the CMOS index and data ports, written by
  DOS/4GW at each switch to real mode): KVM queues them in a ring that
  Bare86 reads after the next exit, without stopping the guest.

## 3. The dynamic translator (JIT)

For games whose own code keeps touching planar VGA, even bursts are not
enough: the interpreter is about 30 ns per instruction, and the game's
rendering loops are millions of instructions per second. The **dynamic
translator** (`src/jit/`) takes over such code:

- **What it translates.** Flat 32-bit protected-mode code (segments based
  at 0 with a 4 GB limit, no paging): the model of DOS/4GW, DOS/32A and
  PMODE/W games. Each basic block of the guest is decoded and re-encoded
  for x86-64 almost unchanged, then run in the host process: guest
  registers live in host registers, flags are computed by the host
  processor itself, and a memory operand becomes the same instruction on
  `[base + guest address]`.
- **Memory without checks.** Translated code reaches guest memory through
  a second mapping of it where the video window, the ROM and the pages of
  translated code are protected. A normal access costs nothing extra; an
  access that faults (video memory, self-modifying code) is caught by a
  `SIGSEGV` handler, done by the interpreter, and that instruction is
  translated with explicit checks from then on (`src/jit/fastmem.rs`).
  Planar writes then go to per-plane views, so the plane switch is just a
  change of base address.
- **Device accesses inside translated code.** Plane selection (`out 3C5h`)
  and reads of the video window are done by the translated code itself,
  without going back to Rust.
- **Blocks chained together.** A block jumps straight to the next one
  once that one is translated; indirect jumps and returns go through a
  lookup table. A run of translated code goes on until the game stops
  touching the card, or until the timer signal is due (so interrupts are
  not delayed).
- **String copies.** `movs` / `rep movs` (Doom's `memcpy`) are run by the
  host's own string instructions on the guest view.
- **Everything else** (code it does not translate, 16-bit code, rare
  instructions, faults) goes to the interpreter, one instruction or one
  burst at a time.

**When it takes over.** By default (`jit = auto`), a policy
(`src/jit/policy.rs`) watches the machine's counters in 250 ms windows: it
estimates the time lost outside native execution (4 µs per video exit, 30
ns per interpreted instruction). Above 5 %, the translator takes over the
video bursts. Once engaged, it checks that it helps: if most of what it
runs goes back to the interpreter, it retreats, for a time that doubles at
each failure (1 to 32 s). It also hands back to native execution after 8,192
instructions without video activity: native code is faster when the card
is not involved. Games that do not need it (mode 13h, VESA, Quake) never
see it.

**Results.**

| Game | Without JIT | With JIT |
|---|---|---|
| Doom 1.9, `timedemo demo1` (5,026 game tics) | ≈ 1,660 realtics, ≈ 106 fps | ≈ 150 realtics, ≈ 1,170 fps |
| Rise of the Triad (capped at 35 fps) | — | −33 % processor time |
| Second Reality, 3D city (16-bit code, interpreted bursts) | ≈ 10 fps | ≈ 24 fps |

**Correctness.** The translator is checked against the interpreter on
random instruction sequences (`jit_matches_interpreter`), and the
interpreter itself against the processor, instruction by instruction
under KVM single step (`interpreter_matches_cpu`); see the Developer corner
of the README for the commands. Doom's E1M1 picture is identical to native
execution.

**Where Doom's time goes now.** The rendering loops run at the speed of
native code: they wait on their dependent memory reads (texel, then colour
map), as on the real processor. About 0.66 s of the 4.3 s `timedemo` is the
game waiting for vertical retrace before palette changes: faithful to the
hardware, and incompressible. The remaining time outside translated code
(about 30 %) is spread out: calls into Rust for other ports, reading the
clock, VM exits. No single item is worth more than a few percent.

## 4. Not wasting the host when the game waits

Many games wait for the next frame by polling: the timer, the VGA status
register (3DAh), or a tick counter in memory. On real hardware that is
free; on the host it keeps a core at 100 %. Bare86 recognizes these waits
and sleeps until the next interrupt:

- polling of the PIT or of the VGA status register (VM exits that repeat
  with nothing else happening);
- loops that never leave the guest (a tick counter in memory): on the
  timer signal, a dry run in the interpreter (`interp::probe_spin`)
  checks that the guest is in a loop that repeats identically, which only
  an interrupt can end;
- `INT 16h` "key ready?" with an empty buffer is answered in the ROM,
  without an exit, so the loop becomes a pure wait the probe can catch.

Quake at rest went from 99 % to about 32 % of a core, Doom playing its
demos from 102 % to about 54 %, Commander Keen 4 from 90 % to 31 %.

## 5. Slowing down: `--mips`

Some programs need a slower processor: games that run as fast as they can,
or that compute their timing from a loop that is far too fast on a modern
host (Carmageddon's race timer counts one 10 ms step per frame, and its
frames come 12 times too fast). `--mips N` caps the guest at N million
instructions per second:

- a hardware performance counter (`perf_event`, guest instructions only)
  interrupts the guest every millisecond's worth of instructions, and
  Bare86 sleeps until the time those instructions should have taken;
- instructions run outside the guest (interpreter, translator) are counted
  too, and trigger the same pacing; translated runs are cut to the same
  slice, so the pauses stay around one millisecond;
- it needs `kernel.perf_event_paranoid` at 1 or less (the guest's DOS code
  runs at ring 0, which the default setting of 2 forbids counting).

`bare86 --throttle` measures its accuracy from 1 to 500 MIPS.

## 6. Hardware-accelerated 3D

Glide (3dfx) and OpenGL 1.1 games do not run their rendering in the guest
at all: Bare86's own `glide2x.dll` / `glide2x.ovl` and `opengl32.dll` send
the calls to the host, which replays them with its OpenGL. Quake II
reaches about 995 fps in `timedemo` with `opengl32`. GLQuake (through 3dfx's
MiniGL and Glide) is held at 60 fps, like a real Voodoo, which waits for
the vertical retrace at each buffer swap.

## 7. Measuring

- Pin the process (`taskset -c 0-3`): on hybrid processors, the small cores
  make random runs slower. Game benchmarks are noisy (±3 %): take the
  median of several runs.
- Check that no other `bare86` is running (`pgrep -a bare86`): a forgotten
  test in the background skews everything.
- `--stats` prints the machine's counters at the end: VM exits by kind,
  busiest ports, interpreted and translated instructions, translator
  engagements and retreats, sleeps in busy waits.
- `--no-jit`, `--jit` (always engaged) and `--no-interp` (one exit per
  emulated access) compare the mechanisms with each other.
- `BARE86_JIT=count` reports what the translator left to the interpreter;
  `BARE86_PERFMAP=1` writes a symbol map of translated code for `perf`.
- `--video F.mkv` and `tools/compare.py` compare a session with a reference
  video (a real PC, DOSBox) to find where a demo runs slower or faster.
- `bare86 --bench` measures the cost of a VM exit and of its alternatives;
  `bare86 --remap` the cost of remapping a VGA plane.

## 8. Possible next steps

None is urgent; Doom's rendering already runs at native speed.

- Translate 16-bit code too (demos and games in real mode), which the
  interpreter handles today.
- Keep the block chaining counters out of memory.
- Try more 32-bit DOS/4GW games with the translator (Heretic, Hexen, Duke
  Nukem 3D; Duke 3D first needs a fix of its chained VGA logo screen, which
  fails with or without the translator).
