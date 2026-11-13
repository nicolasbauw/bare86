# Pilote XMS : détection (INT 2Fh), appel lointain du point d'entrée,
# allocation, copie vers un bloc et retour, verrouillage.
# Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x4300              # 1. pilote présent
    int 0x2F
    mov di, 1
    cmp al, 0x80
    jne fail
    mov ax, 0x4310              # 2. point d'entrée
    int 0x2F
    mov [entry], bx
    mov [entry + 2], es
    mov ah, 0x00                # 3. version 3.00
    call dword ptr [entry]
    mov di, 3
    cmp ax, 0x0300
    jne fail
    mov ah, 0x09                # 4. allocation de 64 Ko
    mov dx, 64
    call dword ptr [entry]
    mov di, 4
    cmp ax, 1
    jne fail
    mov [handle], dx
    mov [mv_dst_h], dx          # 5. copie de « XMS! » vers le bloc…
    mov [mv_src_off + 2], ds
    mov si, offset mv
    mov ah, 0x0B
    call dword ptr [entry]
    mov di, 5
    cmp ax, 1
    jne fail
    mov ax, [handle]            # 6. …et retour vers « back »
    mov [mv_src_h], ax
    mov word ptr [mv_src_off], 0
    mov word ptr [mv_src_off + 2], 0
    mov word ptr [mv_dst_h], 0
    mov word ptr [mv_dst_off], offset back
    mov [mv_dst_off + 2], ds
    mov si, offset mv
    mov ah, 0x0B
    call dword ptr [entry]
    mov di, 6
    cmp ax, 1
    jne fail
    cmp dword ptr [back], 0x21534D58
    jne fail
    mov ah, 0x0C                # 7. verrouillage : adresse ≥ 1 Mo + 64 Ko
    mov dx, [handle]
    call dword ptr [entry]
    mov di, 7
    cmp ax, 1
    jne fail
    cmp dx, 0x11
    jb fail
    mov ah, 0x0A                # 8. libération refusée tant que verrouillé
    mov dx, [handle]
    call dword ptr [entry]
    mov di, 8
    cmp ax, 0
    jne fail
    cmp bl, 0xAB
    jne fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
entry:  .word 0, 0
handle: .word 0
mv:     .long 4
mv_src_h:   .word 0
mv_src_off: .word offset text, 0
mv_dst_h:   .word 0
mv_dst_off: .long 0
text:   .ascii "XMS!"
back:   .long 0
