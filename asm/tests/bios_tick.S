# INT 1Ah/00h autour d'une attente de 110 ms (INT 15h/86h) : le code de
# sortie est l'écart en tics (2 attendus à 18,2 Hz).
    .code16
    .intel_syntax noprefix
    .text
    xor ah, ah
    int 0x1A
    mov si, dx
    mov ah, 0x86
    mov cx, 0x0001
    mov dx, 0xADB0              # 110 000 µs
    int 0x15
    xor ah, ah
    int 0x1A
    mov ax, dx
    sub ax, si
    mov ah, 0x4C
    int 0x21
