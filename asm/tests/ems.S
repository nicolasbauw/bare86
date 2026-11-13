# Pilote EMS : détection par le nom EMMXXXX0 au segment du vecteur INT 67h,
# état, page frame, allocation, mappage et échange de pages.
# Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    xor ax, ax                  # 1. nom du périphérique en vecteur:000Ah
    mov es, ax
    mov es, es:[0x67 * 4 + 2]
    mov di, 0x0A
    mov si, offset name
    mov cx, 8
    cld
    repe cmpsb
    mov di, 1
    jne fail
    mov ah, 0x40                # 2. état
    int 0x67
    mov di, 2
    test ah, ah
    jnz fail
    mov ah, 0x41                # 3. page frame en E000h
    int 0x67
    mov di, 3
    cmp bx, 0xE000
    jne fail
    mov es, bx
    mov ah, 0x43                # 4. deux pages
    mov bx, 2
    int 0x67
    mov di, 4
    test ah, ah
    jnz fail
    mov bp, dx
    mov ax, 0x4400              # 5. page logique 1 en page physique 0
    mov bx, 1
    int 0x67
    mov di, 5
    test ah, ah
    jnz fail
    mov word ptr es:[0], 0x1234
    mov ax, 0x4400              # 6. page logique 0 à sa place : vierge
    xor bx, bx
    mov dx, bp
    int 0x67
    mov di, 6
    cmp word ptr es:[0], 0
    jne fail
    mov ax, 0x4402              # 7. page logique 1 en page physique 2
    mov bx, 1
    mov dx, bp
    int 0x67
    mov di, 7
    cmp word ptr es:[0x8000], 0x1234
    jne fail
    mov ah, 0x45                # 8. libération
    mov dx, bp
    int 0x67
    mov di, 8
    test ah, ah
    jnz fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
name:   .ascii "EMMXXXX0"
