# Passe en mode protégé 32 bits, écrit 1000 octets en A0000h (adresse plate),
# relit le dernier, revient en mode réel et quitte avec cet octet (5Ah).
# Indépendant de son adresse de chargement : la base (CS × 16) est reportée
# dans la GDT et les sauts lointains au démarrage.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, cs
    mov word ptr [rm_seg], ax
    movzx ebx, ax
    shl ebx, 4                  # base linéaire du programme
    add dword ptr [gdtr + 2], ebx
    add dword ptr [pm32_off], ebx
    mov word ptr [gdt + 0x1A], bx
    mov word ptr [gdt + 0x22], bx
    shr ebx, 16
    mov byte ptr [gdt + 0x1C], bl
    mov byte ptr [gdt + 0x24], bl

    cli
    lgdt [gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    .byte 0x66, 0xEA            # jmp far 08h:base+pm32 (offset 32 bits)
pm32_off:
    .long pm32
    .word 0x08

    .code32
pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    cld
    mov edi, 0xA0000
    mov ecx, 1000
    mov al, 0x5A
    rep stosb
    mov bl, byte ptr ds:[0xA0000 + 999]
    .byte 0xEA                  # jmp far 18h:pm16 (code 16 bits, base du programme)
    .long pm16
    .word 0x18

    .code16
pm16:
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov eax, cr0
    and al, 0xFE
    mov cr0, eax
    .byte 0xEA                  # jmp far CS:rm
    .word rm
rm_seg:
    .word 0
rm:
    mov ax, cs
    mov ds, ax
    mov es, ax
    sti
    mov al, bl
    mov ah, 0x4C
    int 0x21

    .balign 8
gdt:
    .quad 0
    .quad 0x00CF9A000000FFFF    # 08h : code 32 bits plat
    .quad 0x00CF92000000FFFF    # 10h : données 32 bits plates
    .quad 0x00009A000000FFFF    # 18h : code 16 bits, base du programme
    .quad 0x000092000000FFFF    # 20h : données 16 bits, base du programme
gdt_end:
gdtr:
    .word gdt_end - gdt - 1
    .long gdt
