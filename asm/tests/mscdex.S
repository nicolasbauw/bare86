# Extensions MSCDEX du DOS intégré, sur une ISO montée en D: (étiquette
# « ESSAI », fichier LISEZ.TXT = « Bonjour du CD »).
# Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x1500              # 1. un lecteur, D:
    xor bx, bx
    int 0x2F
    mov di, 1
    cmp bx, 1
    jne fail
    cmp cx, 3
    jne fail
    mov ax, 0x150B              # 2. D: est un CD
    mov cx, 3
    int 0x2F
    mov di, 2
    cmp bx, 0xADAD
    jne fail
    test ax, ax
    jz fail
    mov ax, 0x150C              # 3. version 2.23
    int 0x2F
    mov di, 3
    cmp bx, 0x0217
    jne fail
    mov ax, 0x1505              # 4. VTOC : descripteur primaire
    mov cx, 3
    xor dx, dx
    mov bx, offset buf
    int 0x2F
    mov di, 4
    jc fail
    cmp ax, 1
    jne fail
    cmp dword ptr [buf + 1], 0x30304443     # « CD00 »
    jne fail
    mov word ptr [buf + 1], 0   # 5. lecture absolue du secteur 16
    mov ax, 0x1508
    mov cx, 3
    xor si, si
    mov di, 16
    mov dx, 1
    mov bx, offset buf
    int 0x2F
    mov di, 5
    jc fail
    cmp dword ptr [buf + 1], 0x30304443
    jne fail
    mov byte ptr [req + 2], 3   # 6. IOCTL 8 : taille du volume
    mov word ptr [req + 14], offset cb
    mov [req + 16], ds
    mov byte ptr [cb], 8
    mov ax, 0x1510
    mov cx, 3
    mov bx, offset req
    int 0x2F
    mov di, 6
    cmp word ptr [req + 3], 0x0100
    jne fail
    cmp dword ptr [cb + 1], 17
    jb fail
    mov word ptr [buf + 1], 0   # 7. READ LONG du secteur 16 (HSG)
    mov byte ptr [req + 2], 128
    mov byte ptr [req + 13], 0
    mov word ptr [req + 14], offset buf
    mov word ptr [req + 18], 1
    mov dword ptr [req + 20], 16
    mov byte ptr [req + 24], 0
    mov ax, 0x1510
    mov cx, 3
    mov bx, offset req
    int 0x2F
    mov di, 7
    cmp word ptr [req + 3], 0x0100
    jne fail
    cmp dword ptr [buf + 1], 0x30304443
    jne fail
    mov ax, 0x3D00              # 8. fichier du CD par le DOS
    mov dx, offset name
    int 0x21
    mov di, 8
    jc fail
    mov bx, ax
    mov ah, 0x3F
    mov cx, 7
    mov dx, offset buf
    int 0x21
    jc fail
    cmp ax, 7
    jne fail
    cmp dword ptr [buf], 0x6A6E6F42         # « Bonj »
    jne fail
    mov ax, 0x3D01              # 9. écriture refusée
    mov dx, offset name
    int 0x21
    mov di, 9
    jnc fail
    mov ah, 0x4E                # 10. étiquette de volume
    mov cx, 0x08
    mov dx, offset all
    int 0x21
    mov di, 10
    jc fail
    cmp dword ptr [0x80 + 0x1E], 0x41535345 # « ESSA »
    jne fail
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21

name:
    .asciz "D:\\LISEZ.TXT"
all:
    .asciz "D:\\*.*"
req:
    .byte 26
    .space 25
cb:
    .space 16
buf:
    .space 2048
