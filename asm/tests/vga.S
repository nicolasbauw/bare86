# 1000 écritures puis lectures dans la fenêtre VGA (A000:0000) ; code de
# sortie = dernier octet relu (doit valoir 0E7h).
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0xA000
    mov es, ax
    xor di, di
    mov cx, 1000
    mov al, 0xE7
    rep stosb
    mov al, es:[999]
    mov ah, 0x4C
    int 0x21
