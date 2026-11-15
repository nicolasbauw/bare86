/* Image de glide2x.ovl : un seul bloc à l'adresse 0 (mkovl.py le relocalise). */
SECTIONS
{
    . = 0;
    .text : { *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) }
    .bss : { *(.bss .bss.* COMMON) }
    /DISCARD/ : { *(.comment) *(.note*) *(.eh_frame*) }
}
