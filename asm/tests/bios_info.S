# INT 11h, 12h, 15h (88h, E801h, C0h, 87h, 2402h), 1Ah/02h, BDA.
# Code de sortie : 0, ou n° du contrôle en échec. Mémoire étendue : 15 Mo.
    .code16
    .intel_syntax noprefix
    .text
    int 0x12                    # 1. 640 Ko conventionnels
    cmp ax, 640
    mov di, 1
    jne fail
    int 0x11                    # 2. 80×25 couleur
    and ax, 0x30
    cmp ax, 0x20
    mov di, 2
    jne fail
    mov ah, 0x88                # 3. mémoire étendue en Ko
    int 0x15
    mov di, 3
    jc fail
    cmp ax, 15360
    jne fail
    mov ax, 0xE801              # 4. E801h : 15 Mo sous 16 Mo, rien au-delà
    int 0x15
    mov di, 4
    jc fail
    cmp ax, 15360
    jne fail
    test bx, bx
    jnz fail
    mov ah, 0xC0                # 5. table de configuration : modèle AT
    int 0x15
    mov di, 5
    jc fail
    cmp byte ptr es:[bx+2], 0xFC
    jne fail
    mov ax, 0x2402              # 6. A20 ouverte
    int 0x15
    mov di, 6
    jc fail
    cmp al, 1
    jne fail
    mov ah, 0x02                # 7. heure de la RTC, CF=0
    stc
    int 0x1A
    mov di, 7
    jc fail
    xor ax, ax                  # 8. BDA : mode vidéo 3
    mov es, ax
    cmp byte ptr es:[0x449], 3
    mov di, 8
    jne fail
    mov ax, 0xFFFF              # 9. 87h : copie de 100000h vers 2000:0000
    mov es, ax
    mov dword ptr es:[0x10], 0x44434241
    push cs
    pop es
    mov si, offset gdt
    mov ah, 0x87
    mov cx, 2
    int 0x15
    mov di, 9
    jc fail
    mov ax, 0x2000
    mov es, ax
    cmp dword ptr es:[0], 0x44434241
    jne fail
    mov ax, 0x4C00
    int 0x21
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
gdt:
    .fill 16, 1, 0
    .word 0xFFFF, 0x0000        # source : 100000h
    .byte 0x10, 0x93, 0, 0
    .word 0xFFFF, 0x0000        # destination : 20000h
    .byte 0x02, 0x93, 0, 0
    .fill 16, 1, 0
