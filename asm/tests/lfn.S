# Noms longs (INT 21h AX=71xxh) : informations du volume, création,
# recherche, nom court, répertoires, renommage, attributs, suppression,
# fonction absente. Code de sortie : 0, ou n° du contrôle en échec (DI).
    .code16
    .intel_syntax noprefix
    .text
    mov dx, offset root         # 1. volume : noms longs annoncés (BX bit 14)
    mov di, offset fsname
    mov cx, 16
    mov ax, 0x71A0
    stc
    int 0x21
    mov di, 1
    jc fail
    test bx, 0x4000
    jz fail
    cmp cx, 255
    jne fail
    mov si, offset longname     # 2. création (DX=10h), écriture, fermeture
    mov bx, 2
    xor cx, cx
    mov dx, 0x10
    mov ax, 0x716C
    stc
    int 0x21
    mov di, 2
    jc fail
    cmp cx, 2                   # action : créé
    jne fail
    mov bx, ax
    mov dx, offset text
    mov cx, 5
    mov ah, 0x40
    int 0x21
    jc fail
    mov ah, 0x3E
    int 0x21
    jc fail
    mov dx, offset pattern      # 3. recherche « long*.TXT » : un seul résultat
    mov di, offset find
    mov cx, 0x0000              # fichiers seulement
    mov si, 1                   # heures au format DOS
    mov ax, 0x714E
    stc
    int 0x21
    mov di, 3
    jc fail
    mov [handle], ax
    cmp dword ptr [find + 0x20], 5      # taille
    jne fail
    mov si, offset find + 0x2C          # nom long rendu tel quel
    mov bx, offset longname
    call strcmp
    mov di, 3
    jne fail
    mov bx, [handle]            # 4. suivant : plus rien ; fermeture
    mov di, offset find
    mov si, 1
    mov ax, 0x714F
    int 0x21
    mov di, 4
    jnc fail
    mov bx, [handle]
    mov ax, 0x71A1
    int 0x21
    jc fail
    mov si, offset longname     # 5. nom court (CL=1)
    mov di, offset path
    mov cx, 1
    mov ax, 0x7160
    stc
    int 0x21
    mov di, 5
    jc fail
    mov si, offset path
    mov bx, offset shortname
    call strcmp
    mov di, 5
    jne fail
    mov dx, offset dirname      # 6. répertoire long : création, entrée,
    mov ax, 0x7139              #    répertoire courant (7147h), sortie
    stc
    int 0x21
    mov di, 6
    jc fail
    mov dx, offset dirname
    mov ax, 0x713B
    int 0x21
    jc fail
    xor dl, dl
    mov si, offset path
    mov ax, 0x7147
    int 0x21
    jc fail
    mov si, offset path
    mov bx, offset dirname
    call strcmp
    mov di, 6
    jne fail
    mov dx, offset dotdot
    mov ax, 0x713B
    int 0x21
    jc fail
    mov dx, offset longname     # 7. renommage dans le répertoire long
    mov di, offset newname
    mov ax, 0x7156
    stc
    int 0x21
    mov di, 7
    jc fail
    mov dx, offset newname      # 8. attributs (BL=0) du fichier renommé
    xor bl, bl
    mov ax, 0x7143
    stc
    int 0x21
    mov di, 8
    jc fail
    test cl, 0x10
    jnz fail
    mov dx, offset newname      # 9. suppression, puis du répertoire
    xor si, si
    mov ax, 0x7141
    stc
    int 0x21
    mov di, 9
    jc fail
    mov dx, offset dirname
    mov ax, 0x713A
    int 0x21
    jc fail
    mov ax, 0x71FF              # 10. fonction absente : AX=7100h, CF
    clc
    int 0x21
    mov di, 10
    jnc fail
    cmp ax, 0x7100
    jne fail
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21

# Compare les chaînes ASCIIZ DS:SI et DS:BX ; ZF si égales.
strcmp:
    mov al, [si]
    cmp al, [bx]
    jne 1f
    inc si
    inc bx
    test al, al
    jnz strcmp
1:  ret

root: .asciz "C:\\"
longname: .asciz "Long File Name.txt"
pattern: .asciz "long*.TXT"
shortname: .asciz "C:\\LONGFI~1.TXT"
dirname: .asciz "Sub Dir"
dotdot: .asciz ".."
newname: .asciz "Sub Dir\\Renamed File.txt"
text: .ascii "hello"
handle: .word 0
fsname: .fill 16, 1, 0
path: .fill 264, 1, 0
find: .fill 0x140, 1, 0
