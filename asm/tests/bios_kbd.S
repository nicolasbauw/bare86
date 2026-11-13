# INT 16h : lit deux touches (entrée scriptée), les affiche, puis vérifie
# que le tampon est vide, et la touche déposée par 05h.
    .code16
    .intel_syntax noprefix
    .text
    mov cx, 2
1:  xor ah, ah
    int 0x16
    mov ah, 0x0E
    int 0x10
    loop 1b
    mov ah, 0x01                # tampon vide : ZF=1
    int 0x16
    mov al, 1
    jnz fail
    mov ah, 0x05                # dépose « ! »
    mov cx, 0x0221
    int 0x16
    mov ah, 0x11
    int 0x16
    mov bl, al
    mov al, 2
    jz fail
    cmp bl, '!'
    jne fail
    xor ah, ah
    int 0x16
    cmp ax, 0x0221
    mov al, 3
    jne fail
    xor al, al
fail:
    mov ah, 0x4C
    int 0x21
