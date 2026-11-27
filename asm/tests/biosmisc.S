# Services BIOS/DOS ajoutés en complétion : INT 14h (aucun port série), INT 17h (imprimante prête), INT 16h AH=0Ah/09h,
# INT 15h AH=83h (attente d'événement) et AH=84h (manette de la machine), INT 21h AH=5Ch (verrouillage) et 5Eh (réseau).
# Code de sortie = n° du contrôle en échec (0 = bon).
    .code16
    .intel_syntax noprefix
    .text
    xor ax, ax
    xor dx, dx
    int 0x14
    cmp ax, 0x8000
    mov bl, 1
    jne fail
    mov ax, 0x0041
    xor dx, dx
    int 0x17
    cmp ah, 0x90
    mov bl, 2
    jne fail
    mov ah, 0x0A
    int 0x16
    cmp bx, 0xAB41
    mov bl, 3
    jne fail
    push cs
    pop es
    mov bx, offset flag
    mov ax, 0x8300
    xor cx, cx
    mov dx, 100
    int 0x15
    test byte ptr [flag], 0x80
    mov bl, 4
    jz fail
    mov ah, 0x84
    xor dx, dx
    int 0x15
    mov bl, 5
    jc fail           # manette émulée par la machine : CF = 0
    mov ax, 0x5C00
    int 0x21
    mov bl, 7
    jc fail
    mov ax, 0x5E00
    int 0x21
    mov bl, 8
    jnc fail
    xor bl, bl
fail:
    mov al, bl
    mov ah, 0x4C
    int 0x21
flag:
    .byte 0
