# INT 10h en mode graphique : AH=0Ch/0Dh (pixels, mode 13h puis 12h), AH=0Eh (caractère dessiné), AH=09h, défilement AH=06h,
# police 8×8 (AX=1130h BH=3). Code de sortie = n° du contrôle en échec (0 = bon).
    .code16
    .intel_syntax noprefix
    .text
    # --- mode 13h : pixel (10,20) = 5, relu par AH=0Dh et dans la mémoire vidéo A000 ---
    mov ax, 0x0013
    int 0x10
    mov ax, 0x0C05
    xor bh, bh
    mov cx, 10
    mov dx, 20
    int 0x10
    mov ah, 0x0D
    xor bh, bh
    mov cx, 10
    mov dx, 20
    int 0x10
    cmp al, 5
    mov bl, 1
    jne fail
    mov ax, 0xA000
    mov es, ax
    cmp byte ptr es:[20 * 320 + 10], 5
    mov bl, 2
    jne fail
    # OU exclusif hors mode 13h n'existe pas ici : en 13h, AL = couleur sur 8 bits (200)
    mov ax, 0x0CC8
    xor bh, bh
    mov cx, 11
    mov dx, 20
    int 0x10
    cmp byte ptr es:[20 * 320 + 11], 200
    mov bl, 3
    jne fail
    # --- caractère 'A' (0x41) par AH=0Eh en (0,0), couleur 15 : au moins 8 points allumés dans la cellule 8×8 ---
    mov ax, 0x0200
    xor bh, bh
    xor dx, dx
    int 0x10
    mov ax, 0x0E41
    mov bx, 0x000F
    int 0x10
    xor di, di
    mov al, 15
    call count
    cmp si, 8
    mov bl, 4
    jb fail
    # le curseur a avancé d'une colonne
    mov ah, 0x03
    xor bh, bh
    int 0x10
    cmp dl, 1
    mov bl, 5
    jne fail
    # --- AH=09h : 'B' en (1,0) avec couleur 7, deux caractères ---
    mov ax, 0x0200
    xor bh, bh
    mov dx, 0x0100
    int 0x10
    mov ax, 0x0942
    mov bx, 0x0007
    mov cx, 2
    int 0x10
    mov di, 8 * 320
    mov al, 7
    call count          # cellule (1,0) : points de couleur 7
    cmp si, 8
    mov bl, 6
    jb fail
    # --- défilement d'une ligne de caractères vers le haut : la cellule (1,0) monte en (0,0) ---
    mov ax, 0x0601
    mov bh, 0
    mov cx, 0
    mov dx, 0x1827
    int 0x10
    xor di, di
    mov al, 7
    call count          # cellule (0,0) : le 'B' est monté
    cmp si, 8
    mov bl, 7
    jb fail
    # --- police 8×8 : AX=1130h BH=3 -> ES:BP dans la ROM (F000), CX = hauteur 8 ---
    mov ax, 0x1130
    mov bh, 3
    int 0x10
    mov ax, es
    cmp ax, 0xF000
    mov bl, 8
    jne fail
    cmp cx, 8
    mov bl, 9
    jne fail
    # --- mode 12h : pixel couleur 9 en (100,50), relu ---
    mov ax, 0x0012
    int 0x10
    mov ax, 0x0C09
    xor bh, bh
    mov cx, 100
    mov dx, 50
    int 0x10
    mov ah, 0x0D
    xor bh, bh
    mov cx, 100
    mov dx, 50
    int 0x10
    cmp al, 9
    mov bl, 10
    jne fail
    mov ah, 0x0D
    xor bh, bh
    mov cx, 101
    mov dx, 50
    int 0x10
    cmp al, 0
    mov bl, 11
    jne fail
    xor bl, bl
fail:
    mov al, bl
    mov ah, 0x4C
    int 0x21

# SI = nombre de pixels de couleur AL dans la cellule 8×8 dont le coin haut gauche est à l'offset DI de A000 (ES = A000)
count:
    push ax
    push es
    push di
    mov bx, 0xA000
    mov es, bx
    xor si, si
    mov bh, 8
rowloop:
    mov cx, 8
    push di
colloop:
    cmp es:[di], al
    jne 1f
    inc si
1:
    inc di
    loop colloop
    pop di
    add di, 320
    dec bh
    jnz rowloop
    pop di
    pop es
    pop ax
    ret
