# INT 10h AX=4F00h : les pointeurs du bloc (liste des modes) sont normalisés (décalage < 16) et la liste commence par 100h, finit par
# FFFFh et contient 400×300×16 (139h) ; AX=4F01h de ce mode : 400 × 300, 16 bits. Code de sortie = n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    push cs
    pop es
    mov di, offset buf
    mov ax, 0x4F00
    int 0x10
    cmp ax, 0x004F
    mov bl, 1
    jne fail
    cmp word ptr [buf + 14], 16
    mov bl, 2
    jae fail
    les si, [buf + 14]
    cmp word ptr es:[si], 0x100
    mov bl, 3
    jne fail
    xor dx, dx
scan:
    mov ax, es:[si]
    cmp ax, 0xFFFF
    je done
    cmp ax, 0x139
    jne 1f
    mov dx, 1
1:
    add si, 2
    jmp scan
done:
    test dx, dx
    mov bl, 4
    jz fail
    push cs
    pop es
    mov di, offset buf
    mov cx, 0x139
    mov ax, 0x4F01
    int 0x10
    cmp word ptr [buf + 0x12], 400
    mov bl, 5
    jne fail
    cmp word ptr [buf + 0x14], 300
    mov bl, 6
    jne fail
    cmp byte ptr [buf + 0x19], 16
    mov bl, 7
    jne fail
    xor bl, bl
fail:
    mov al, bl
    mov ah, 0x4C
    int 0x21
buf:
    .space 512
