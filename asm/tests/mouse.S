# INT 33h : réinitialisation (souris, 2 boutons), position, plage ; CMOS :
# mémoire étendue (17h–18h). Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    xor ax, ax                  # 1. souris présente, deux boutons
    int 0x33
    mov di, 1
    cmp ax, 0xFFFF
    jne fail
    cmp bx, 2
    jne fail
    mov ax, 4                   # 2. position puis lecture
    mov cx, 100
    mov dx, 50
    int 0x33
    mov ax, 3
    int 0x33
    mov di, 2
    cmp cx, 100
    jne fail
    cmp dx, 50
    jne fail
    mov ax, 7                   # 3. plage horizontale 200–300 : position ramenée
    mov cx, 200
    mov dx, 300
    int 0x33
    mov ax, 3
    int 0x33
    mov di, 3
    cmp cx, 200
    jne fail
    mov al, 0x17                # 4. CMOS : 15 Mo de mémoire étendue
    out 0x70, al
    in al, 0x71
    mov bl, al
    mov al, 0x18
    out 0x70, al
    in al, 0x71
    mov bh, al
    mov di, 4
    cmp bx, 15360
    jne fail
    mov al, 0x40                # 5. CMOS : écriture (mise en file par KVM) relue
    out 0x70, al
    mov al, 0x5A
    out 0x71, al
    mov al, 0x0F
    out 0x70, al
    mov al, 0x40
    out 0x70, al
    in al, 0x71
    mov di, 5
    cmp al, 0x5A
    jne fail
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
