# Mode 12h, mode d'écriture 2 : pixel (100, 50) couleur 0Eh (jaune).
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0012
    int 0x10
    mov dx, 0x3CE
    mov ax, 0x0205              # mode d'écriture 2
    out dx, ax
    mov ax, 0x0808              # masque : bit 3 (x = 100 → 7 - 4)
    out dx, ax
    mov ax, 0xA000
    mov es, ax
    mov di, 50 * 80 + 12
    mov al, es:[di]             # charge les latches
    mov byte ptr es:[di], 0x0E
    mov ax, 0x4C00
    int 0x21
