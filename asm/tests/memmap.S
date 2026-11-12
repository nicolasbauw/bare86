# Vérifie la carte mémoire ; code de sortie 0 si tout va bien, sinon n° d'échec.
    .code16
    .intel_syntax noprefix
    .text
    # 1. RAM conventionnelle : écriture puis relecture en 9000:0000.
    mov ax, 0x9000
    mov es, ax
    mov byte ptr es:[0], 0x5A
    mov al, 1
    cmp byte ptr es:[0], 0x5A
    jne fail
    # 2. Modèle de machine en F000:FFFE = FCh (AT).
    mov ax, 0xF000
    mov es, ax
    mov al, 2
    cmp byte ptr es:[0xFFFE], 0xFC
    jne fail
    # 3. La ROM est en lecture seule.
    mov byte ptr es:[0xFFFE], 0x00
    mov al, 3
    cmp byte ptr es:[0xFFFE], 0xFC
    jne fail
    # 4. Mémoire texte B800:0000 : RAM ordinaire.
    mov ax, 0xB800
    mov es, ax
    mov word ptr es:[0], 0x1F41
    mov al, 4
    cmp word ptr es:[0], 0x1F41
    jne fail
    # 5. Mémoire étendue (A20 active) : FFFF:0010 = 100000h.
    mov ax, 0xFFFF
    mov es, ax
    mov byte ptr es:[0x10], 0xA5
    xor ax, ax
    mov ds, ax
    mov al, 5
    cmp byte ptr ds:[0], 0xA5      # pas de repliement sur 0
    je fail
    cmp byte ptr es:[0x10], 0xA5
    jne fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ah, 0x4C
    int 0x21
