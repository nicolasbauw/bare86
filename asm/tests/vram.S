# Mémoire vidéo : mode 13h (tampon linéaire mappé), mode X (plan unique
# mappé après le seuil, puis simple remappage), copie par les latches
# (logique des plans émulée). Le test Rust vérifie contenus et compteurs.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0013              # 1. mode 13h, 64 000 octets à 21h
    int 0x10
    mov ax, 0xA000
    mov es, ax
    xor di, di
    mov cx, 64000
    mov al, 0x21
    cld
    rep stosb
    mov dx, 0x3C4               # 2. mode X : chain4 coupé
    mov ax, 0x0604
    out dx, ax
    mov ax, 0x0202              # écriture vers le plan 1, lecture du plan 1
    out dx, ax
    mov dx, 0x3CE
    mov ax, 0x0104
    out dx, ax
    xor di, di
    mov cx, 16000
    mov al, 0x55
    rep stosb
    mov dx, 0x3C4               # 3. plan 2 (lecture plan 2)
    mov ax, 0x0402
    out dx, ax
    mov dx, 0x3CE
    mov ax, 0x0204
    out dx, ax
    xor di, di
    mov cx, 16000
    mov al, 0x66
    rep stosb
    mov dx, 0x3C4               # 4. copie par les latches, tous les plans
    mov ax, 0x0F02
    out dx, ax
    mov dx, 0x3CE
    mov ax, 0x4105
    out dx, ax
    push es
    pop ds
    xor si, si
    mov di, 0x8000
    mov cx, 100
    rep movsb
    push cs
    pop ds
    mov ax, 0x4C00
    int 0x21
