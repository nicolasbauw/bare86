# INT 10h AX=1B00h BX=0 (informations de fonctionnalité/état VGA) : AL = 1Bh au retour, mode courant (13h), 256 couleurs,
# colonnes (40), pointeur vers une table de fonctionnalités dans la ROM. Code de sortie = n° du contrôle en échec (0 = bon).
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0013
    int 0x10
    push cs
    pop es
    mov di, offset buf
    xor bx, bx
    mov ax, 0x1B00
    int 0x10
    cmp al, 0x1B
    mov bl, 1
    jne fail
    cmp byte ptr [buf + 4], 0x13
    mov bl, 2
    jne fail
    cmp word ptr [buf + 0x27], 256
    mov bl, 3
    jne fail
    cmp word ptr [buf + 5], 40
    mov bl, 4
    jne fail
    cmp word ptr [buf + 2], 0xF000
    mov bl, 5
    jne fail
    xor bl, bl
fail:
    mov al, bl
    mov ah, 0x4C
    int 0x21
buf:
    .space 64
