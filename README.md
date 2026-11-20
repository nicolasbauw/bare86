# Bare86

Bare86 runs DOS programs on Linux at native speed. It does not emulate the x86
processor: the DOS code runs directly on the host CPU, inside a minimal
virtual machine set up with KVM hardware virtualization. Only the BIOS, DOS and
the PC hardware (VGA, Sound Blaster, keyboard, mouse, disks, CD-ROM, network
card) are provided in software, by Bare86 itself, written in Rust.

The spirit is the one of DOSBox: type the name of a game and play it. The
difference is under the hood, and shows with demanding late-90s games.

- [Features](#features)
- [Requirements](#requirements)
- [Building](#building)
- [Quick start](#quick-start)
- [Command-line options](#command-line-options)
- [Configuration file](#configuration-file)
- [Window, keys and menu](#window-keys-and-menu)
- [Game music: FM or General MIDI](#game-music-fm-or-general-midi)
- [Running programs directly](#running-programs-directly)
- [Launcher](#launcher)
- [The built-in DOS](#the-built-in-dos)
- [Booting a real DOS](#booting-a-real-dos)
- [CD-ROM](#cd-rom)
- [Network](#network)
- [Scripted and windowless sessions](#scripted-and-windowless-sessions)
- [Developer corner](#developer-corner)
  - [Writing DOS programs in assembly (GNU as)](#writing-dos-programs-in-assembly-gnu-as)
  - [Writing DOS programs in C (gcc)](#writing-dos-programs-in-c-gcc)
- [Status and limits](#status-and-limits)
- [License](#license)

## Features

- **Native speed.** No instruction is emulated in the normal course of things.
  Doom 1.9's `timedemo demo1` runs at about 110 frames per second with the
  full-width status bar (`screenblocks 10`) and 122 with the default view
  (`screenblocks 9`), and Quake 1.06 at about 940 frames per second in 320×200
  VGA and 600 in 640×480 VESA, on a Ryzen AI 9 HX 370. A small x86
  interpreter only takes over for bursts of accesses that no hardware mapping
  can serve: planar VGA writes (latches, bit mask, several planes at once) and
  dense port I/O (palette loads, sound and network chips).
- **Dynamic translator.** Games that draw in planar VGA switch planes
  column by column, one VM exit each (about 64 % of Doom's time), or go
  through the interpreter for their video memory accesses. When the
  statistics show such a loss, the flat 32-bit protected-mode code of the
  game (DOS/4GW without paging) runs translated on the host around its
  video work, writing straight into the video planes, and goes back to
  native execution as soon as it stops touching the card. Doom's `timedemo
  demo1` goes from about 105 to about 1,150 frames per second; Rise of the
  Triad, capped at 35, uses about half the processor. Games that do not need
  it (mode 13h, VESA, 16-bit code) never see it; `--no-jit` (or `jit = off`
  in a profile) turns it off. How it all fits together, with measurements:
  [docs/PERFORMANCE.md](docs/PERFORMANCE.md).
- **Idle when the game waits.** A game that busy-waits for its next frame
  (polling the timer, the VGA status register, or a tick counter in memory)
  no longer keeps a host core at 100 %: Bare86 recognizes the wait and sleeps
  until the next interrupt (Quake at rest: about a third of a core; Doom
  playing its demos: two thirds, most of it actual work).
- **A built-in DOS, nothing to install.** Bare86 provides its own DOS (it
  reports version 5.00): a host directory becomes drive C:, `.COM` and `.EXE`
  programs start at once. XMS 3.0, EMS 4.0 (LIM) and the DOS extenders used by
  games work: DOS/4GW, DOS/32A, PMODE/W, CWSDPMI (DJGPP). Long file names are
  supported (the Windows 95 API, INT 21h AX=71xxh): a host file keeps its name,
  and DOS programs that only know 8.3 names see a short alias (`LONGFI~1.TXT`).
- **Or a real DOS.** Boot FreeDOS (tested with 1.3, JEMMEX included) or another
  DOS from floppy and hard disk images, with its own drivers.
- **Some Windows programs, through HX DOS Extender.** The built-in DOS runs
  Win32 programs by name with Japheth's
  [HX DOS Extender](https://github.com/Baron-von-Riedesel/HX) (freeware, free
  to redistribute): console programs, and part of the games using DirectDraw,
  DirectSound and GDI. HX is taken from the game's `PATH` or `C:\HX`; if there
  is none, Bare86 downloads the official HXRT and HXGUI 2.23 archives once
  (with `curl`, SHA-256 checked) into `~/.local/share/bare86/HX` and shows it
  as drive Z:. The Quake II demo
  runs full screen at 640×480 (`--mem 64`; it also needs HX's dummy
  `UNSUPP\WSOCK32.DLL` copied next to `quake2.exe`). There is no Windows
  itself: no Windows 95/98 desktop, no Direct3D.
- **3dfx Glide, drawn by the host's graphics card.** Bare86's own
  `glide2x.dll` (`guest/glide2x/`, prebuilt) passes the Glide 2 calls of a
  Win32 game to the host, which replays them with OpenGL (offscreen, through
  EGL) and shows the result instead of the VGA picture, like a Voodoo card's
  VGA pass-through. No Voodoo hardware is emulated. The built-in DOS shows
  this DLL in every directory that has no `glide2x.dll` of its own (a real
  file wins), so games find it without copying anything: the Quake II demo
  runs with its 3dfx MiniGL driver
  (`+set vid_ref gl +set gl_driver 3dfxgl`), at about 875 frames/s in its
  timedemo against 634 with the software renderer. DOS games linked to
  Glide (DOS/4GW) get the same code as `glide2x.ovl`, provided the same
  way: the 3dfx demo of Grand Theft Auto runs with it. Glide functions a
  game calls but Bare86 lacks are named on the terminal.
- **OpenGL 1.1, drawn by the host's graphics card.** Bare86's own
  `opengl32.dll` (`guest/opengl32/`, prebuilt, provided like `glide2x.dll`)
  passes the OpenGL calls of a Win32 game to the host, which replays them
  in an offscreen OpenGL context (compatibility profile): no translation
  layer, no Voodoo limits. HX's GDI forwards `ChoosePixelFormat` and
  `SwapBuffers` to it, as Windows 9x did. The Quake II demo runs with
  `+set vid_ref gl +set gl_driver opengl32` at about 990 frames/s in its
  timedemo (`+set gl_swapinterval 0`; otherwise frames are paced at 60 Hz,
  the DLL's default), and GLQuake runs once the 3dfx MiniGL
  (`opengl32.dll` in its directory) is removed. The context takes the size
  of the video mode the game has set. The Kingpin demo (OpenGL only) runs
  with `+set gl_driver opengl32`, in full screen up to 1024×768, with sound.
- **Video.** VGA text and graphics modes (mode 13h, mode X, 16-colour planar
  modes), SVGA through VESA VBE 2.0 with a linear framebuffer (or VBE 1.2 with
  banked windows): 320×200 to 1024×768 in 8, 15, 16, 24 and 32 bits per pixel,
  plus 320×240 and 512×384. Text mode uses the built-in Terminus 8×16 font. The window
  keeps a 4:3 aspect ratio with sharp scaling.
- **Sound.** Sound Blaster 16 (DSP 4.05, 8- and 16-bit DMA, mixer) at 220h,
  IRQ 5, DMA 1 and 5, with the `BLASTER` variable set, a Sound Blaster Pro
  1 or 2 with its output filters (`--sb pro1`, `--sb pro2`), or an ESS
  AudioDrive (`--sb ess`); FM synthesis on
  an OPL3, an OPL2 (the sound of the AdLib and early Sound Blasters), two
  OPL2 (Sound Blaster Pro 1), or the FM clones of cheap cards: Creative's CQM
  (Sound Blaster 16 Vibra, AWE64) and ESS's ESFM (AudioDrive), by
  [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3), Nuked OPL2, Nuked CQM
  and [ESFMu](https://github.com/Kagamiin/ESFMu); PC speaker; MPU-401
  MIDI interface at 330h (UART mode), whose General MIDI music is played on
  OPL3 FM chips by [libADLMIDI](https://github.com/Wohlstand/libADLMIDI), with
  79 instrument banks (default: the Windows 9x FM synthesizer), and can be
  recorded to a standard MIDI file (`--midi-out`). With `--gus` (`gus = on`
  in a profile), a Gravis UltraSound as well, at 240h, DMA 3, IRQ 7 (the
  `ULTRASND` variable is set): 1 MB of sample memory, 32 wavetable voices,
  volume ramps, timers, DMA uploads, after DOSBox Staging's emulation of the
  GF1; many demos of the 1990s were written for it.
- **Input.** Keyboard by physical key position (the DOS sees a US keyboard),
  mouse (INT 33h driver under the built-in DOS, PS/2 under a real DOS),
  joystick or game controller on the game port.
- **Disks.** Floppy images for A: and B:, hard disk image for C:. Several
  floppies can be given for A: and swapped while a program runs.
- **CD-ROM.** `.iso` images, and `.cue` sheets with their `.bin` files and
  audio tracks, which play through the sound mix. MSCDEX is built into the
  built-in DOS; a real DOS gets an ATAPI drive for its own CD drivers (FreeDOS
  mounts it as D: without any setup).
- **Network.** NE2000 network card with user-mode NAT
  ([libslirp](https://gitlab.freedesktop.org/slirp/libslirp), the one of
  QEMU): DHCP, DNS, outgoing TCP and UDP, port forwarding, no root rights.
  mTCP under FreeDOS downloads at about 9.5 MB/s.
- **Window.** SDL3 window, full screen, mouse capture, an in-window menu to
  change floppies and CDs, speed and display settings.
- **Speed control.** Throttle to a given number of millions of instructions per
  second for games that run too fast, and change it while playing.
- **Scripting.** Scripted keyboard, screenshots, sound recording and a text
  screen dump make windowless test runs possible.

## Requirements

- Linux on x86-64, with KVM: the processor's virtualization (Intel VT-x or
  AMD-V) enabled in the firmware, and `/dev/kvm` accessible to your user
  (usually through the `kvm` group).
- To build: a recent stable Rust toolchain (edition 2024), a C compiler,
  GNU binutils (`as`, `ld`, `objcopy`, which assemble the BIOS ROM),
  `pkg-config`, and the SDL3 and libslirp development files.

On Arch Linux:

```sh
sudo pacman -S rust base-devel sdl3 libslirp
```

`xorriso` is optional: some tests use it to build CD images, and are skipped
without it.

- Speed limits (`--mips`, Ctrl+Alt+− and Ctrl+Alt++) count the guest's
  instructions with a hardware performance counter, which recent kernels
  restrict (`kernel.perf_event_paranoid` defaults to 2). Allow it once with
  `sudo sysctl kernel.perf_event_paranoid=1`, or for good with a file
  `/etc/sysctl.d/99-bare86.conf` holding `kernel.perf_event_paranoid = 1`.
  Without it, Bare86 says so and runs at full speed.

## Building

```sh
cargo build --release
```

The executable is `target/release/bare86`. `cargo test --release` runs the test
suite (it needs `/dev/kvm`).

The DLLs that run inside the guest (`guest/`, such as `glide2x.dll`) are
prebuilt and committed; rebuilding them needs `clang` and `lld`
(`guest/glide2x/build.sh`, `guest/opengl32/build.sh`, which also needs
Python).

## Quick start

```sh
bare86                                    # built-in DOS shell, current directory as C:
bare86 --window ~/games/doom/DOOM.EXE     # run a game in a window
bare86 --window --cdrom game.cue GAME.EXE # with its CD in D:
bare86 --window --hda freedos.img --boot  # boot a real DOS
```

Options go before the program name: everything after it is passed to the
program as its command line. Without `--window`, Bare86 runs in the terminal:
what the program writes through DOS appears there, and the terminal keyboard is
the DOS keyboard; `--term` shows the whole text screen instead. Graphics and
sound need the window (or `--shot` and `--wav`).

## Command-line options

**Program and drives**

| Option | Effect |
|---|---|
| `--config FILE` | Configuration file to read instead of `./bare86.conf` or `~/.config/bare86/bare86.conf`, see [Configuration file](#configuration-file) |
| `--mount DIR` | Host directory mounted as C: (default: the program's directory, or the current directory for the shell) |
| `--fda IMG`, `--fdb IMG` | Floppy image for A: or B:. Repeat `--fda` for a set of floppies, swapped with Ctrl+Alt+D or the menu |
| `--hda IMG` | Hard disk image for C: (written in place: work on a copy) |
| `--cdrom IMG` | CD image (`.iso`, or `.cue` with its `.bin` files and audio tracks), see [CD-ROM](#cd-rom) |
| `--boot` | Boot from the disk images (A: if present, else C:) instead of the built-in DOS |
| `--net` | NE2000 network card with user-mode NAT, see [Network](#network) |
| `--net-fwd tcp:HOST:GUEST` | Forward a host port (on 127.0.0.1) to the guest; also `udp:`; implies `--net`; repeatable |

**Machine**

| Option | Effect |
|---|---|
| `--mem MB` | Total memory in MB (default 64, as a late-1990s PC; demos of 1997–1999 need 32 or more) |
| `--ems MB` | EMS memory in MB, in addition (default 4, 0 to 32) |
| `--mips N` | Throttle to N million instructions per second (default: full speed); needs `kernel.perf_event_paranoid` at 1 or less, see [Requirements](#requirements) |
| `--vbe1` | VESA 1.2 card: banked windows only, no linear framebuffer |
| `--set NAME=VALUE` | Environment variable for the built-in DOS (repeatable; in a profile, one `set = NAME=VALUE` line each), e.g. `DPMILDR=16` for Heretic II under HX |
| `--sb 16\|pro2\|pro1\|ess` | Sound card: Sound Blaster 16 (default), Sound Blaster Pro 2 (`pro` too: DSP 3.02, OPL3), Pro 1 (two OPL2 chips), or ESS AudioDrive ES1688 (Sound Blaster Pro compatible, ESS identification, ESFM FM: the aggressive, saturated FM sound of many cheap mid-90s PCs); the Creative Pro models filter their output like the real cards |
| `--gus` | Gravis UltraSound (GF1) at 240h, DMA 3, IRQ 7, in addition to the Sound Blaster; `gus = on` in a profile |
| `--opl opl2\|dualopl2\|opl3\|cqm\|esfm` | FM chip, instead of the card's own: OPL2 (YM3812, AdLib and Sound Blaster 1.x/2.0), two OPL2, OPL3 (YMF262), Creative's CQM (Sound Blaster 16 Vibra and AWE64, with its metallic sound) or ESS's ESFM (AudioDrive cards); the last two are OPL3-compatible clones found on many cheap cards |
| `--midi opl3\|off` | General MIDI music played on OPL3 FM chips (default), or not played |
| `--midi-bank N` | FM instrument bank (default 58, the Windows 9x FM synthesizer; 14 is the bank of Doom); `--midi-bank list` prints them |
| `--midi-out F.mid` | Record the MIDI music sent to the MPU-401 to a standard MIDI file (valid at any time, even if the session stops abruptly) |

**Display and sound**

| Option | Effect |
|---|---|
| `--window` | SDL3 window, with sound |
| `--window-auto` | Window shown at the first graphics mode (VGA, VESA, Glide, OpenGL) or at the first text screen the program draws itself (menus, setup screens); until then, and for programs that only print, the terminal |
| `--fullscreen` | Window, full screen from the start |
| `--menu-key L` | Letter of the menu shortcut, Ctrl+Alt+L (default M) |
| `--launcher [DIR]` | List the games of DIR (default: the current directory) and run the chosen one, see [Launcher](#launcher) |
| `--binfmt` | Print the rules that let Linux run DOS and Win32 programs directly, see [Running programs directly](#running-programs-directly) |
| `--font F.psf` | PSF font for text mode (default: the built-in Terminus 8×16) |
| `--term` | Show the text screen live in the terminal |
| `--screen` | Print the text screen when the program ends |
| `--shot F.png` | Screenshot when the program ends |
| `--wav F.wav` | Record the sound to a file (without a window) |
| `--video F.mkv` | Record the screen at 70 frames per second, 640×480, through `ffmpeg` (to compare with a reference with `tools/compare.py`) |

**Scripting and diagnostics**

| Option | Effect |
|---|---|
| `--keys SCRIPT` | Scripted keyboard, then end of the session, see [Scripted and windowless sessions](#scripted-and-windowless-sessions) |
| `--limit S` | Stop after S seconds and print the registers, the code at CS:IP and the screen |
| `--stats` | Machine counters at the end (VM exits, emulated video memory accesses, I/O ports…) |
| `--debug` | Debugger: registers, memory, single step |
| `--trace-int` | Log the interrupts serviced by the host |
| `--trace-io` | Report I/O ports with no device |
| `--no-jit` | No dynamic translator (by default it takes over, when the statistics show a gain, the flat 32-bit code of games that lose time on planar VGA, such as Doom); `jit = off` in a profile |
| `--jit` | Dynamic translator always engaged, whatever the statistics (measurements) |
| `--no-interp` | No interpreter bursts: one VM exit per emulated video memory access (to measure what the interpreter brings) |
| `-h`, `--help` | Print the options |

## Configuration file

A configuration file holds the options of a game, one per line, written as
`option = value`, exactly as on the command line. `option = true` gives an
option without value, `false` leaves it out. `program = …` and `args = …` name
the program to run when the command line gives none. Relative paths (`mount`,
`program`, `fda`, `fdb`, `hda`, `cdrom`, `font`, `shot`, `wav`) start from the
directory of the file. Comments start with `#` or `;`.

```ini
# ~/games/doom/bare86.conf
window = true
mips = 100
mount = c
program = c/DOOM.EXE
args = -nosound
```

Bare86 reads `--config FILE` if given, otherwise `./bare86.conf`, otherwise
`~/.config/bare86/bare86.conf`; only one file is read. Options given on the
command line take precedence over the file.

## Window, keys and menu

A click in the window captures the mouse; the middle button, Ctrl+Alt+G or
leaving the window releases it. Bare86's own shortcuts all use Ctrl+Alt, so
that the function keys and their Ctrl combinations stay free for the DOS
programs and the desktop:

| Keys | Action |
|---|---|
| Ctrl+Alt+M | Menu (the letter can be changed with `--menu-key`) |
| Alt+Enter or Ctrl+Alt+F | Full screen |
| Ctrl+Alt+S | Screenshot (`bare86_NNN.png` in the current directory) |
| Ctrl+Alt+G | Release the mouse |
| Ctrl+Alt+D | Next floppy in A: (when `--fda` was repeated) |
| Ctrl+Alt+− / Ctrl+Alt++ | Slower / faster (3 to 400 MIPS, then full speed) |

The menu opens over the running program, which keeps running but no longer
receives the keyboard. It changes the floppy in A: and the CD (a small file
browser lists the images of the right type), ejects the CD, sets the speed and
full screen, takes a screenshot and recalls the shortcuts. Arrows choose,
Enter confirms, left and right change the speed, Escape closes.

## Game music: FM or General MIDI

DOS games play their music in one of two ways, chosen in their own setup
program:

- **AdLib or Sound Blaster**: the game drives the card's FM chip itself. This
  is the sound of the original hardware, and the one that changes with the
  card (`--sb`) and the chip (`--opl`): OPL2, OPL3, ESS ESFM, Creative CQM.
- **General MIDI (MPU-401)**: the game sends notes to port 330h. A real PC
  then needed a separate synthesizer (a Roland Sound Canvas, a wavetable
  daughterboard); the card's FM chip was not involved. Bare86 plays these
  notes with a modern FM synthesizer (libADLMIDI), whose timbres can be
  chosen (`--midi-bank`; default: the FM synthesizer of Windows 95/98).

For the original experience, set the game to Sound Blaster and pick the card.

## Running programs directly

Linux can hand DOS and Win32 programs to Bare86 by itself (binfmt_misc), so
that `./GAME.EXE` runs from a shell like a native program. Bare86 does not
change the system on its own: `bare86 --binfmt` prints the rules and how to
install them, as root:

```sh
bare86 --binfmt | sudo tee /etc/binfmt.d/zz-bare86.conf
sudo systemctl restart systemd-binfmt
chmod +x GAME.EXE                      # programs need the execute permission
```

`.EXE` files are recognized by their `MZ` signature, `.COM` files by their
extension. The file name sorts after Mono's `mono.conf`, which also claims the
`MZ` signature, so that Bare86's rules come first; .NET programs are handed
back to Mono. A program started this way gets:

- its profile: the `bare86.conf` next to it, or that of a parent directory
  whose `mount` contains it (`./quake2.exe` in `quake2/c/QUAKE2` uses
  `quake2/bare86.conf`); started without arguments, the profile's own program
  also gets the profile's `args`;
- when a graphical display is available and its output goes to a terminal,
  `--window-auto`: the program starts in the terminal, and a window opens at
  its first graphics mode (VGA, VESA, Glide or OpenGL), or at the first
  text screen it draws itself in video memory (a menu, a setup screen),
  taking over the keyboard; a utility that only prints never opens one. Otherwise
  (`./PKZIP.EXE | less`, or `--term` in the profile) it runs in the
  terminal, and its DOS output goes to the pipe or file;
- its exit code, returned to the shell.

Arguments written the Linux way are translated into DOS paths, under a narrow
rule: an argument that names an existing file or directory (or a file to
create in an existing directory) and contains a slash other than a leading
one (`./out.zip`, `"Mes Archives/notes.txt"`, `/home/me/games/doom/x.wad`),
and only if it lies on drive C: (the program's directory, or the profile's
`mount`). It becomes `C:\…` with the short names the DOS sees. DOS switches
(`/W`, `-timedemo`) and plain names are left alone. A path outside drive C:
is passed unchanged, with a warning: a DOS program gets no access to the rest
of the machine. To remove the rules: `sudo rm /etc/binfmt.d/zz-bare86.conf`, then
`echo -1 | sudo tee /proc/sys/fs/binfmt_misc/bare86-{exe,com,COM}`.

## Launcher

`bare86 --launcher ~/games` opens a window listing the games of `~/games`: each
subdirectory that holds a `bare86.conf` is a game, shown with its program, CD
or real DOS boot. The launcher only opens when asked: `bare86` alone starts the
built-in DOS directly.

| Keys | Action |
|---|---|
| Enter | Run the game, in a window, with its configuration file (other command-line options still apply); when it ends, the launcher comes back, with the game's error if it failed |
| E or F2 | Edit the game's profile |
| Enter on "+ New game..." | Choose the game's program (.EXE, .COM, .BAT) in a file browser; the profile goes into the game's directory (the subdirectory of the games directory that holds the program), then the editor opens |
| Escape | Quit |

The profile editor shows a one-line help for the selected setting, and sets,
without typing: the program (chosen in a file
browser), real DOS boot and hard disk image, CD and floppy images, sound card,
FM chip, General MIDI synthesizer and its timbres (a short list: Windows
95/98, Windows 3.x, Doom, Miles, HMI, Apogee), speed, memory, network and full
screen. Left and right change a value, Enter opens the file browser or toggles,
Delete clears a setting, Escape leaves without saving. Saving rewrites
`bare86.conf` in place: comments and options the editor does not handle are
kept, and paths inside the game's directory are written relative to it.

## The built-in DOS

Without `--boot`, Bare86 runs its own DOS. A host directory is drive C: (the
program's directory, or `--mount DIR`). Host file names that are valid 8.3
names appear in upper case; longer names get a short name (`LONGNA~1.TXT`).
The shell, started without a program, knows `DIR`, `CD`, `MD`, `RD`, `DEL`,
`REN`, `TYPE`, `ECHO`, `SET`, `CLS`, `VER` and `EXIT`, and runs programs. Its
messages are in French for now.

Memory: 640 KB of conventional memory, XMS through the built-in HIMEM-like
driver, EMS in a page frame at E000h. The environment holds
`BLASTER=A220 I5 D1 H5 P330 T6` (`A220 I5 D1 T4` with a Sound Blaster Pro), so that
games find the Sound Blaster and the MPU-401.

Programs bound to the DOS/4GW stub that do not ship `DOS4GW.EXE` (it was
expected in the user's `PATH`, and is not freely redistributable) get
[DOS/32A](https://sourceforge.net/projects/dos32a/), a compatible extender,
under that name: Bare86 downloads it from the FreeDOS 1.3 package repository
the first time it is needed (fixed version, checked SHA-256) into
`~/.local/share/bare86/DOS32A`.

Programs built with Turbo Pascal 7 or Borland Pascal 7 and its CRT unit stop
with "Runtime error 200" on any modern processor: the `Delay` calibration
overflows. The built-in DOS fixes it in memory when such a program starts, as
the TPPATCH tool did on disk. Tested with Second Reality (Future Crew, 1993).

## Booting a real DOS

With `--boot`, Bare86 starts the boot sector of A: (or of C: without a floppy)
like a PC BIOS. The DOS then uses its own drivers: XMS and EMS come from its
memory managers, the mouse from its driver (PS/2 mouse), the CD from its CD
drivers. FreeDOS 1.3 installs from its own installation floppies, and runs
with JEMMEX; DOS/4GW games run under it at the same speed.

```sh
cp freedos.img work.img                  # Bare86 writes to the image
bare86 --window --hda work.img --boot
```

## CD-ROM

`--cdrom IMAGE` takes an `.iso` file, or a `.cue` sheet with its `.bin` files
(one or several, with data and audio tracks, `PREGAP` and `INDEX` lines). File
names in the sheet are also found when their case differs.

- **Built-in DOS:** the CD is drive D:, with MSCDEX 2.23 built in: games find
  it, read files through DOS, and play the audio tracks.
- **Real DOS (`--boot`):** the CD is an ATAPI drive on the secondary IDE
  channel (ports 170h–177h and 376h, IRQ 15). FreeDOS loads its UDVD2 and
  SHSUCDX drivers at boot and mounts it as D:.

The audio tracks are mixed with the Sound Blaster. The menu (Ctrl+Alt+M)
changes or ejects the CD while a program runs.

## Network

`--net` adds an NE2000 card at port 300h, IRQ 3, behind a user-mode NAT: the
guest is 10.0.2.15 (by DHCP), the gateway 10.0.2.2 (it also reaches the host),
the DNS server 10.0.2.3. Outgoing TCP and UDP connections go through the host;
no root rights are needed. `--net-fwd tcp:2323:23` makes a guest server
reachable on the host's port 2323.

Under a real DOS, load a packet driver for the card, for example the free
Crynwr driver, then the network programs:

```bat
NE2000 0x60 3 0x300
SET MTCPCFG=C:\NET\MTCP.CFG
DHCP
HTGET -o PAGE.HTM http://example.com/
```

## Scripted and windowless sessions

Without a window, Bare86 can run a program from a script and keep the
results: useful for tests and for checking a program quickly.

```sh
bare86 --keys "{until:C:\>}DIR{enter}" --screen
bare86 --hda work.img --boot --keys "{until:C:\>}cd \DOOM{enter}doom -timedemo demo1{enter}{until:timed}" --wav /dev/null --screen
```

The `--keys` script types its text as is, and understands:

| Item | Meaning |
|---|---|
| `{enter}`, `{esc}`, `{tab}`, `{f1}`, `{up}`, `{alt+f}`, `{ctrl+c}`… | Named keys and combinations |
| `{press:KEY}`, `{release:KEY}` | Hold a key down (through the `{wait:…}` that follow), then release it |
| `{wait:MS}` | Pause for MS milliseconds |
| `{until:TEXT}` | Wait until TEXT appears on the text screen |
| `{speed:MIPS}`, `{speed:max}` | Change the speed |
| `{disk}`, `{fda:IMG}` | Next floppy in A:, or a given image |
| `{cd:IMG}`, `{cd:}` | Change the CD, or eject it |
| `{mouse:DX,DY,BUTTONS}` | Mouse motion and buttons |
| `{{` | A literal `{` |

The session ends once the script is typed and the program is waiting for a
key. `--limit` stops a run that hangs.

## Developer corner

Notes for those who work on Bare86 itself. The design journal
([docs/JOURNAL.md](docs/JOURNAL.md), in French) holds the reasoning,
measurements and pitfalls behind each step. [docs/PERFORMANCE.md](docs/PERFORMANCE.md) is
the overview of speed: VM exits, interpreter bursts, the dynamic translator,
busy waits, `--mips`, and how to measure.

**Source map.** The guest code runs natively under KVM; everything else is
software in `src/`:

| Where | What |
|---|---|
| `kvm.rs`, `backend.rs`, `mem.rs`, `machine.rs` | Virtual machine, guest memory, main loop and VM exits |
| `interp/` | Host x86 interpreter (iced-x86) that runs bursts of emulated video memory and port accesses without leaving the host |
| `jit/` | Dynamic translator: flat 32-bit guest blocks re-encoded for x86-64, run on the host; `policy.rs` decides when it takes over |
| `bios.rs`, `asm/bios.S`, `hle.rs` | BIOS ROM (GNU `as`, Intel syntax) and the common helpers of the services run by the host (BIOS, DOS) |
| `dos/` | Built-in DOS: files, processes, long names, MSCDEX, shell, Win32 loading through HX |
| `vga.rs`, `render.rs`, `sdl.rs`, `menu.rs`, `launcher.rs` | Video card, picture, window, in-game menu and launcher |
| `sb16.rs`, `opl.rs`, `mpu401.rs`, `midisynth.rs`, `audio.rs` | Sound cards, FM, MIDI and mixing |
| `pic.rs`, `pit.rs`, `dma.rs`, `kbc.rs`, `rtc.rs`, `ide.rs`, `ne2000.rs`, `net/`… | The rest of the PC |
| `glide/` | Glide replayed with the host's OpenGL; its guest side is `guest/glide2x/` |
| `opengl/` | OpenGL 1.1 replayed with the host's OpenGL; its guest side is `guest/opengl32/` |

**Tests.** `cargo test --release` (needs `/dev/kvm`; the OpenGL tests are
skipped without an offscreen context, the CD tests without `xorriso`). The
interpreter has a differential test: each random instruction is run both by
the processor (KVM single step) and by the interpreter, then registers, flags
and memory are compared:

```sh
DIFFTEST_CASES=100000 DIFFTEST_SEED=7 cargo test --release interpreter_matches_cpu -- --nocapture
```

`DIFFTEST_TRACE=1` prints every instruction compared.

The dynamic translator is checked the same way against the interpreter, on
random instruction sequences (`JIT_TRACE=1` replays a failing case one
instruction at a time); `BARE86_JIT=count` prints at the end what it left
to the interpreter. Translated code reaches guest memory through a second
mapping of it where the video window, the ROM and translated code pages are
protected: an access that faults there is taken back by a `SIGSEGV`
handler and that instruction is translated with explicit checks from then
on (`BARE86_JIT_NO_FASTMEM=1` checks every access, `BARE86_JIT_NO_BMI2=1`
without the BMI2 fast checks; `BARE86_PERFMAP=1` writes a symbol map of the
translated code for `perf`):

```sh
JIT_CASES=100000 JIT_SEED=7 cargo test --release jit_matches_interpreter -- --nocapture
```

**Debugging a Win32 program.** `--trace-int` logs the DOS calls (with file
names) that HX makes for the program. `BARE86_HWBP=ADDR[,ADDR…]` sets up to
four hardware execution breakpoints (linear addresses in hexadecimal, e.g.
in the game's EXE, loaded at its base address): at each hit, the registers
and the top of the stack go to the terminal, and the program goes on.
With `BARE86_HWBP_W=1` as well, the addresses become write watchpoints
(4 bytes; the stop comes after the writing instruction); use `--no-jit`,
since translated code and interpreter bursts write from the host.
`BARE86_KEYLOG=1` traces the keys received by the window and the bytes
handed to the keyboard controller.

**Measuring.** Pin the process to the same cores (`taskset -c 0-3`): on hybrid
processors, the small cores make random runs slower. Game benchmarks are
noisy (±3 %): take the median of five runs. `--stats` gives the VM exit
counters, `--no-interp` turns the interpreter off for comparison, and three
micro-benchmarks from the feasibility study remain, each run alone without
other options:

| Option | Measures |
|---|---|
| `bare86 --bench` | Cost of a VM exit: I/O port, video memory access, and the same accesses coalesced or run by the interpreter |
| `bare86 --throttle` | Accuracy of `--mips` from 1 to 500 MIPS, and the unthrottled speed |
| `bare86 --remap` | Cost of mapping another VGA plane at A0000h (KVM slot recreated, or host view remapped) |

**Glide.** The guest DLLs (`guest/glide2x/glide2x.dll` for Win32,
`glide2x.ovl` for DOS/4GW) are built by `guest/glide2x/build.sh` (clang,
lld, Python) and committed. The commands they send to the host are numbered
in `src/glide/proto.rs` and `guest/glide2x/glide2x.c`, which must agree. A
DOS game calling a Glide function that Bare86 lacks gets a terminal line
`glide2x.ovl: _NAME@n is not implemented (ignored)`.

**OpenGL.** `guest/opengl32/build.sh` runs `gen.py`, which reads the 336
OpenGL 1.1 prototypes of `gl11.txt` and writes the DLL's functions
(`gl_gen.c`), its export table (`opengl32.def`) and the host's replay table
(`src/opengl/api.rs`); then clang and lld build `opengl32.dll` (committed).
Calls that return a value or pass a pointer of variable size (images,
vertex arrays, `glGet*`) send the buffer at once and the host reads or
writes the game's memory; `src/opengl/mod.rs` handles them by hand.

**Tools** (`tools/`):

| Tool | Use |
|---|---|
| `netbench.sh IMAGE MB [RUNS]` | Network throughput: a FreeDOS image downloads a file from a host HTTP server, timed from the `BARE86_NETLOG=1` TCP trace |
| `is5x.py CABDIR DEST` | Extract InstallShield 5 cabinets that `unshield` cannot (some game demos) |
| `compare.py --ref REF --video OURS --out DIR` | Compare a session recorded with `--video` with a reference video of the same demo or game (a real PC, DOSBox, a downloaded video): each 20-second window is found in the reference by its pictures, giving the drift (the program running slower or faster than in the reference), distinct frames per second on both sides, and side-by-side sheets at the same moments |
| `terminus_cp437.py` | Rebuild the built-in font `fonts/terminus-cp437-8x16.bin` from Terminus |

### Writing DOS programs in assembly (GNU as)

Small DOS programs can be written with the GNU assembler of any Linux
distribution (binutils), in Intel syntax, and run at once by Bare86: no DOS
toolchain to install. A `.COM` program is the simplest format: raw code and
data, loaded at offset 100h of a single 64 KB segment, with CS = DS = ES =
SS. `.code16` makes the assembler produce 16-bit real-mode code (32-bit
registers and instructions remain usable, the guest is a 386 and up).

`hello.S`, a string printed through DOS:

```asm
# hello.S: a DOS .COM program in GNU as (Intel syntax, 16-bit real mode).
    .code16
    .intel_syntax noprefix
    .text
    mov dx, offset msg      # DS:DX = "$"-terminated string
    mov ah, 0x09            # DOS: print string
    int 0x21
    mov ax, 0x4C00          # DOS: exit with code 0
    int 0x21
msg: .ascii "Hello, DOS!\r\n$"
```

`bars.S`, mode 13h graphics: the palette is loaded through the VGA DAC
ports and the 256 colours are drawn straight into video memory at A000h:

```asm
# bars.S: mode 13h (320x200, 256 colours), the 256 colours side by side
# with a blue gradient loaded into the palette; any key returns to DOS.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0013          # BIOS: set video mode 13h
    int 0x10

    mov dx, 0x3C8           # VGA DAC: start writing at colour 0
    xor al, al
    out dx, al
    inc dx                  # 3C9h: red, green, blue (0-63) per colour
    xor cx, cx
palette:
    mov al, cl
    shr al, 3               # red and green: 0-31
    out dx, al
    out dx, al
    mov al, cl
    shr al, 2               # blue: 0-63
    out dx, al
    inc cl
    jnz palette             # 256 colours

    mov ax, 0xA000          # ES:DI = video memory, row after row
    mov es, ax
    xor di, di
    cld
    mov bx, 200             # rows
row:
    xor si, si              # x
column:
    imul ax, si, 205        # colour = x * 205 / 256: 0-255 across the row
    mov al, ah
    stosb                   # ES:[DI++] = AL
    inc si
    cmp si, 320
    jb column
    dec bx
    jnz row

    xor ah, ah              # BIOS: wait for a key
    int 0x16
    mov ax, 0x0003          # back to 80x25 text mode
    int 0x10
    mov ax, 0x4C00
    int 0x21
```

Assemble, link at 100h into a flat binary, run:

```sh
as --32 -mx86-used-note=no -o hello.o hello.S
ld -m elf_i386 -Ttext=0x100 -e 0x100 --oformat=binary -o HELLO.COM hello.o
bare86 HELLO.COM

as --32 -mx86-used-note=no -o bars.o bars.S
ld -m elf_i386 -Ttext=0x100 -e 0x100 --oformat=binary -o BARS.COM bars.o
bare86 --window BARS.COM
```

`-mx86-used-note=no` keeps out a property note that some distributions'
assemblers add by default, which would otherwise end up in the binary.
With the [binfmt rules](#running-programs-directly) installed,
`./HELLO.COM` works too. Drive C: is the program's directory, so it can
open the files next to it.

### Writing DOS programs in C (gcc)

The host's gcc can also produce real-mode code: `-m16` emits 32-bit code
with the prefixes that make it run in a 16-bit segment (a 386 or later is
required, which the guest is). There is no C library: the program calls DOS
and the BIOS through inline assembly. Four small files make a `.COM`
program.

`crt0.S`, the entry point:

```asm
# crt0.S: entry point of a .COM program written in C (gcc -m16).
# DOS loads the program at CS:0100h with CS = DS = ES = SS; clear the
# upper half of ESP (gcc's code uses 32-bit addresses), zero the .bss,
# call main(), then exit with its return value as the exit code.
    .code16
    .intel_syntax noprefix
    .section .startup, "ax"
    .globl _start
_start:
    movzx esp, sp
    cld
    mov di, offset __bss_start
    mov cx, offset __bss_end
    sub cx, di
    xor al, al
    rep stosb
    call main
    mov ah, 0x4C
    int 0x21
```

`com.ld`, the linker script (one segment from 100h, flat binary):

```ld
/* com.ld: link a .COM program (one 64 KB segment, loaded at 100h). */
OUTPUT_FORMAT(binary)
ENTRY(_start)
SECTIONS
{
    . = 0x100;
    .text   : { *(.startup) *(.text*) }
    .rodata : { *(.rodata*) }
    .data   : { *(.data*) }
    .bss    : { __bss_start = .; *(.bss*) *(COMMON) __bss_end = .; }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
```

`dos.h`, a few services:

```c
/* dos.h: a few DOS and BIOS services for gcc -m16 programs. */
#pragma once

static inline void dos_putc(char c)
{
    __asm__ volatile("int $0x21" : : "a"(0x0200), "d"(c));
}

static inline void dos_puts(const char *s)
{
    while (*s)
        dos_putc(*s++);
}

/* BIOS: wait for a key; returns its ASCII code. */
static inline unsigned char bios_getkey(void)
{
    unsigned short ax = 0;
    __asm__ volatile("int $0x16" : "+a"(ax));
    return ax & 0xFF;
}

static inline void bios_set_mode(unsigned char mode)
{
    __asm__ volatile("int $0x10" : : "a"((unsigned short)mode) : "memory"); /* AH = 0 */
}

static inline void outb(unsigned short port, unsigned char v)
{
    __asm__ volatile("outb %0, %1" : : "a"(v), "Nd"(port));
}
```

`hello.c`:

```c
/* hello.c: a DOS .COM program in C. */
#include "dos.h"

int main(void)
{
    dos_puts("Hello from C! Press a key...\r\n");
    unsigned char k = bios_getkey();
    dos_puts("You pressed: ");
    dos_putc(k);
    dos_puts("\r\n");
    return 0;
}
```

`xor.c`, mode 13h graphics. gcc has no far pointers, but its `__seg_gs`
address space puts a `GS:` prefix on every access through such a pointer:
with GS = A000h, the pointer addresses video memory:

```c
/* xor.c: mode 13h, an XOR pattern drawn in video memory (A000h), any key
   returns to DOS. Video memory is reached through GS: gcc's __seg_gs
   address space adds the GS: prefix to every access through the pointer. */
#include "dos.h"

static unsigned char __seg_gs *const vga = 0;

int main(void)
{
    bios_set_mode(0x13);
    __asm__ volatile("mov %0, %%gs" : : "r"((unsigned short)0xA000));

    outb(0x3C8, 0);                     /* palette: colour 0 onwards */
    for (int i = 0; i < 256; i++) {
        outb(0x3C9, i >> 2);            /* red   0-63 */
        outb(0x3C9, (i >> 3) ^ 0x1F);   /* green 31-0 */
        outb(0x3C9, 63 - (i >> 2));     /* blue  63-0 */
    }

    for (int y = 0; y < 200; y++)
        for (int x = 0; x < 320; x++)
            vga[y * 320 + x] = (x * 4 / 5) ^ y; /* 0-255 across */

    bios_getkey();
    bios_set_mode(0x03);
    return 0;
}
```

Build and run:

```sh
gcc -m16 -march=i386 -Os -Wall -ffreestanding -fno-pic -fno-stack-protector \
    -fno-asynchronous-unwind-tables -nostdlib -static -Wl,--build-id=none \
    -Wa,-mx86-used-note=no -T com.ld -o HELLO.COM crt0.S hello.c
bare86 HELLO.COM

gcc -m16 -march=i386 -Os -Wall -ffreestanding -fno-pic -fno-stack-protector \
    -fno-asynchronous-unwind-tables -nostdlib -static -Wl,--build-id=none \
    -Wa,-mx86-used-note=no -T com.ld -o XOR.COM crt0.S xor.c
bare86 --window XOR.COM
```

Limits of this setup: code, data and stack share 64 KB, and gcc may call
`memcpy`, `memset` or `memcmp` for structure copies or large
initializations, which then have to be written too. For bigger programs, a
DOS toolchain with a C library is the usual way: DJGPP (32-bit, DPMI) or
Open Watcom (16 and 32-bit), whose programs Bare86 runs. `--trace-int` shows
the DOS and BIOS calls a program makes, and `--debug` steps through it.

## Status and limits

Bare86 is young and under active development. What works is listed above;
among what does not, yet:

- Linux x86-64 hosts only. Windows and Intel macOS hosts are not planned.
- No IPX network for multiplayer games yet.
- Under FreeDOS, a CD changed while running is not seen by the UDVD2 and
  SHSUCDX drivers (the built-in DOS sees it).
- Fonts loaded by programs (code pages, text-mode games that redefine
  characters) are not shown yet in text mode.
- The dynamic translator is new: it only serves flat 32-bit code without
  paging, and has been tried on Doom and Rise of the Triad. If a game
  misbehaves where it did not before, try `--no-jit` (`jit = off`).
- OpenGL is version 1.1 with a few extensions (multitexture ARB and SGIS,
  point parameters, compiled vertex arrays); paletted textures, hardware
  gamma, `wglUseFontBitmaps` and `wglUseFontOutlines` are missing.

The design journal, with the decisions, measurements and the progress of
each step, is in [docs/JOURNAL.md](docs/JOURNAL.md) (in French).

## License

Bare86 is free software, under the GNU General Public License version 3 or
later, see `LICENSE`. It includes in `third_party/` Nuked-OPL3 (LGPL 2.1 or
later) and a subset of libADLMIDI (LGPL 2.1 or later, GPL 3, MIT: see its
license files; its embedded FM banks carry their own terms), and the Terminus
font (SIL Open Font License 1.1) in `fonts/`.

When needed, Bare86 downloads third-party software that it does not include:
HX DOS Extender (freeware) and DOS/32A. This product uses DOS/32 Advanced DOS
Extender technology.

Bare86 is by Nicolas BAUW, developed with [Claude Code](https://claude.com/claude-code).
