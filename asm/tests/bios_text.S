# INT 10h en mode texte : télétype, curseur, 09h, 13h, défilement, 0Fh.
# Code de sortie : 0, ou n° du contrôle en échec ; l'écran est vérifié
# par le test Rust.
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x0003              # mode 3, écran effacé
    int 0x10
    mov si, offset lines        # « L1\r\nL2 » en télétype
1:  lodsb
    test al, al
    jz 2f
    mov ah, 0x0E
    int 0x10
    jmp 1b
2:  mov ah, 0x02                # curseur en ligne 10, colonne 5
    xor bh, bh
    mov dx, 0x0A05
    int 0x10
    mov ax, 0x0958              # 3 × « X », attribut 1Eh
    mov bx, 0x001E
    mov cx, 3
    int 0x10
    mov ah, 0x03                # 09h ne déplace pas le curseur
    xor bh, bh
    int 0x10
    cmp dx, 0x0A05
    mov al, 1
    jne fail
    push cs                     # « Hi » en ligne 12, attribut 70h, curseur mis à jour
    pop es
    mov bp, offset hi
    mov ax, 0x1301
    mov bx, 0x0070
    mov cx, 2
    mov dx, 0x0C00
    int 0x10
    mov ax, 0x0601              # défilement d'une ligne, tout l'écran
    mov bh, 0x07
    xor cx, cx
    mov dx, 0x184F
    int 0x10
    mov ah, 0x0F
    int 0x10
    cmp ax, 0x5003              # 80 colonnes, mode 3
    mov al, 2
    jne fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ah, 0x4C
    int 0x21
lines: .asciz "L1\r\nL2"
hi: .ascii "Hi"
