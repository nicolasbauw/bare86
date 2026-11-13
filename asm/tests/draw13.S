# Mode 13h : pixel (10, 5) couleur 4 ; DAC 200 = (63, 32, 0), pixel (11, 5).
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0013
    int 0x10
    mov dx, 0x3C8
    mov al, 200
    out dx, al
    inc dx
    mov al, 63
    out dx, al
    mov al, 32
    out dx, al
    xor al, al
    out dx, al
    mov ax, 0xA000
    mov es, ax
    mov byte ptr es:[5 * 320 + 10], 4
    mov byte ptr es:[5 * 320 + 11], 200
    mov ax, 0x4C00
    int 0x21
