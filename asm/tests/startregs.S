# Registres au départ d'un .COM (comme MS-DOS) et palette par l'INT 10h
# (AH=10h). Code de sortie : n° du contrôle en échec, 0 si tout va bien.
    .code16
    .intel_syntax noprefix
    .text
    mov al, 1
    cmp cx, 0x00FF
    jne fail
    inc ax
    cmp si, 0x0100
    jne fail
    inc ax
    cmp di, sp
    jne fail
    inc ax
    cmp bp, 0x091C
    jne fail
    inc ax
    mov bx, cs
    cmp dx, bx
    jne fail

    # 5 : AX=1012h, BX=0100h : seul l'octet bas compte (registre 0).
    mov ax, 0x1012
    mov bx, 0x0100
    mov cx, 1
    mov dx, offset rgb
    int 0x10
    mov ax, 0x1015
    xor bx, bx
    int 0x10
    mov al, 6
    cmp dh, 0x11
    jne fail
    inc ax
    cmp cx, 0x2233
    jne fail

    # 8 : AX=1010h puis lecture d'un bloc (AX=1017h).
    mov ax, 0x1010
    mov bx, 200
    mov dh, 0x3F
    mov cx, 0x0102
    int 0x10
    mov ax, 0x1017
    mov bx, 200
    mov cx, 1
    mov dx, offset back
    int 0x10
    mov al, 8
    cmp word ptr [back], 0x013F
    jne fail
    inc ax
    cmp byte ptr [back + 2], 0x02
    jne fail

    # 10 : registre de palette (AX=1000h, relu par AX=1007h).
    mov ax, 0x1000
    mov bx, 0x2A05
    int 0x10
    mov ax, 0x1007
    mov bx, 0x0005
    int 0x10
    mov al, 10
    cmp bh, 0x2A
    jne fail

    xor al, al
fail:
    mov ah, 0x4C
    int 0x21
rgb: .byte 0x11, 0x22, 0x33
back: .byte 0, 0, 0
