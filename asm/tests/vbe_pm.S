# VBE 2.0 : interface en mode protégé 32 bits (INT 10h AX=4F0Ah). Mode 101h, table
# demandée en mode réel, puis en mode protégé : fenêtre (réglage et lecture),
# début d'affichage (x=10, y=2) et palette (entrées 7 et 8 : B,G,R = 1,2,3 et 4,5,6).
# Code de sortie : 0, ou n° du contrôle en échec ; le test Rust relit l'état de la carte.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x4F02              # mode 101h
    mov bx, 0x101
    int 0x10
    mov dl, 1
    cmp ax, 0x004F
    jne fail16
    mov ax, 0x4F0A              # interface protégée : ES:DI = table, CX = longueur
    xor bx, bx
    int 0x10
    mov dl, 2
    cmp ax, 0x004F
    jne fail16
    mov dl, 3
    cmp cx, 0x20
    jb fail16
    mov ax, es
    movzx esi, ax
    shl esi, 4
    movzx edi, di
    add esi, edi                # esi : adresse linéaire de la table
    mov ax, cs
    mov word ptr [rm_seg], ax
    movzx ebp, ax
    shl ebp, 4                  # ebp : base linéaire du programme
    mov ebx, ebp
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
    .byte 0x66, 0xEA            # jmp far 08h:base+pm32
pm32_off:
    .long pm32
    .word 0x08

    .code32
pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    cld
    movzx eax, word ptr [esi]   # fonction de fenêtre
    add eax, esi
    mov edi, eax
    mov byte ptr [0xA0000], 0x11    # banque 0
    xor ebx, ebx
    mov dx, 1
    call edi
    mov byte ptr [0xA0000], 0x22    # banque 1
    xor ebx, ebx
    xor edx, edx
    call edi
    mov bl, 10
    cmp byte ptr [0xA0000], 0x11    # retour en banque 0
    jne leave_pm
    mov dx, 0xFFFF              # lecture de la banque : 0
    mov bx, 0x0100
    call edi
    mov bl, 11
    test dx, dx
    jne leave_pm
    xor ebx, ebx
    mov dx, 1                   # banque 1 pour le test Rust
    call edi
    movzx eax, word ptr [esi + 2]   # début d'affichage
    add eax, esi
    mov cx, 10
    mov dx, 2
    xor ebx, ebx
    call eax
    movzx eax, word ptr [esi + 4]   # palette : 2 entrées à partir de 7
    add eax, esi
    lea edi, [ebp + pal]
    mov cx, 2
    mov dx, 7
    xor ebx, ebx
    call eax
    xor ebx, ebx
leave_pm:
    .byte 0xEA                  # jmp far 18h:pm16
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
    .byte 0xEA
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
fail16:
    mov al, dl
    mov ah, 0x4C
    int 0x21

    .balign 4
pal:
    .byte 1, 2, 3, 0, 4, 5, 6, 0
    .balign 8
gdt:
    .quad 0
    .quad 0x00CF9A000000FFFF
    .quad 0x00CF92000000FFFF
    .quad 0x00009A000000FFFF
    .quad 0x000092000000FFFF
gdt_end:
gdtr:
    .word gdt_end - gdt - 1
    .long gdt
