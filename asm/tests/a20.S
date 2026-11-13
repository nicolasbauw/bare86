# Porte A20 : fermée au démarrage (FFFF:0010 = 0000:0000), ouverte par le
# port 92h puis par XMS (HMA distincte), refermée par le 8042.
# Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    xor ax, ax
    mov ds, ax
    mov ax, 0xFFFF
    mov es, ax
    mov byte ptr ds:[0x500], 0x5A   # 1. fermée : repliement
    mov di, 1
    cmp byte ptr es:[0x510], 0x5A
    jne fail
    in al, 0x92                 # 2. ouverte par le port 92h : la HMA est à part
    or al, 2
    out 0x92, al
    mov byte ptr es:[0x510], 0xA5
    mov di, 2
    cmp byte ptr ds:[0x500], 0x5A
    jne fail
    in al, 0x92                 # 3. fermée par le port 92h
    and al, 0xFD
    out 0x92, al
    mov di, 3
    cmp byte ptr es:[0x510], 0x5A
    jne fail
    mov ax, 0x4310              # 4. ouverte par XMS 03h, état lu par 07h
    int 0x2F
    mov ds:[0x600], bx
    mov ds:[0x602], es
    mov ah, 0x03
    call dword ptr ds:[0x600]
    mov ah, 0x07
    call dword ptr ds:[0x600]
    mov di, 4
    cmp ax, 1
    jne fail
    mov ax, 0xFFFF
    mov es, ax
    cmp byte ptr es:[0x510], 0xA5
    jne fail
    mov al, 0xD1                # 5. fermée par le 8042 (D1h, port de sortie)
    out 0x64, al
    mov al, 0xCD
    out 0x60, al
    mov di, 5
    cmp byte ptr es:[0x510], 0x5A
    jne fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
