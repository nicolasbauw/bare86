# Third-party components

Bare86 is distributed under the GNU General Public License, version 3 or later
(`LICENSE`). It includes or links the following components, under licenses
compatible with it.

## Included in the source tree

| Component | Where | License |
|---|---|---|
| Nuked OPL3 (Nuke.YKT), OPL3 emulation | `third_party/nuked-opl3/` | LGPL-2.1-or-later |
| libADLMIDI (Vitaly Novichkov), MIDI synthesis on OPL chips | `third_party/libadlmidi/` | GPL-3.0 and LGPL-2.1 parts (see its `LICENSE*` files) |
| ESFMu, ESS ESFM emulation (part of libADLMIDI's chips) | `third_party/libadlmidi/src/chips/esfmu/` | LGPL-2.1-or-later |
| Terminus font (Dimitar Zhekov), 8×16 CP437 rendering | `fonts/` | SIL Open Font License 1.1 (`fonts/OFL-Terminus.txt`) |

The guest libraries in `guest/` (`glide2x.dll`, `glide2x.ovl`, `opengl32.dll`)
are Bare86's own code, shipped prebuilt with their sources.

## Linked into the binary

SDL 3 (zlib license) is linked statically into the release binary
(`--features static-sdl`); other builds use the system's SDL 3. libslirp
(BSD-3-Clause and MIT parts) is linked dynamically from the system.

The Rust crates used (see `Cargo.lock`) are under permissive licenses: MIT,
Apache-2.0, MIT OR Apache-2.0, Zlib, ISC, BSD-3-Clause, 0BSD, and
Unicode-3.0 for `unicode-ident` data.

## Downloaded at run time

HX DOS Extender (Japheth, freeware), used to run Win32 programs and for the
`--dpmi` option, is not shipped with Bare86: it is downloaded from its
official releases on first use.
