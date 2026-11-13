# Mode X (13h non chaîné) : pixel (7, 3) couleur 4, plan 3, décalage 3*80+1.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0013
    int 0x10
    mov dx, 0x3C4
    mov ax, 0x0604              # chain4 coupé
    out dx, ax
    mov dx, 0x3D4
    mov ax, 0x0014              # pas d'adressage en doubles mots
    out dx, ax
    mov ax, 0xE317              # adressage par octets
    out dx, ax
    mov dx, 0x3C4
    mov ax, 0x0802              # plan 3
    out dx, ax
    mov ax, 0xA000
    mov es, ax
    mov byte ptr es:[3 * 80 + 1], 4
    mov ax, 0x4C00
    int 0x21
