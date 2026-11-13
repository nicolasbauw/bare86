# VBE 2.0 : bloc d'information, informations du mode 101h, passage en
# 640×480×8, banques par INT 10h et par la fonction de fenêtre (appel
# lointain), relecture. Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    mov dword ptr [info], 0x32454256  # 1. « VBE2 » → « VESA », version 2.0
    mov ax, 0x4F00
    mov di, offset info
    int 0x10
    mov di, 1
    cmp ax, 0x004F
    jne fail
    cmp dword ptr [info], 0x41534556
    jne fail
    cmp word ptr [info + 4], 0x0200
    jne fail
    mov ax, 0x4F01                # 2. mode 101h : 640×480, 8 bits, LFB
    mov cx, 0x101
    mov di, offset info
    int 0x10
    mov di, 2
    cmp ax, 0x004F
    jne fail
    cmp word ptr [info + 0x12], 640
    jne fail
    cmp byte ptr [info + 0x19], 8
    jne fail
    cmp dword ptr [info + 0x28], 0xE0000000
    jne fail
    mov eax, [info + 0x0C]        # fonction de fenêtre
    mov [winfunc], eax
    mov ax, 0x4F02                # 3. passage en 101h
    mov bx, 0x101
    int 0x10
    mov di, 3
    cmp ax, 0x004F
    jne fail
    mov ax, 0x4F03
    int 0x10
    cmp bx, 0x101
    jne fail
    push es
    mov ax, 0xA000
    mov es, ax
    mov byte ptr es:[0], 0x11     # 4. banque 0, puis banque 1 par 4F05h
    mov ax, 0x4F05
    xor bx, bx
    mov dx, 1
    int 0x10
    mov byte ptr es:[0], 0x22
    xor bx, bx                    # 5. banque 2 par la fonction de fenêtre
    mov dx, 2
    call dword ptr [winfunc]
    mov byte ptr es:[0], 0x33
    xor bx, bx                    # 6. retour en banque 0 : relecture
    xor dx, dx
    call dword ptr [winfunc]
    mov di, 6
    cmp byte ptr es:[0], 0x11
    jne fail_es
    mov ax, 0x4F05                # 7. lecture de la banque
    mov bx, 0x0100
    int 0x10
    mov di, 7
    test dx, dx
    jne fail_es
    pop es
    mov ax, 0x4C00
    int 0x21
fail_es:
    pop es
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
    .balign 4
winfunc: .long 0
info:   .fill 512, 1, 0
