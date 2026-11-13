# Banc de la mémoire vidéo : cinq motifs d'accès typiques, chronométrés par
# le compteur de cycles (RDTSC) calibré sur 18 tics du BIOS. Affiche pour
# chacun la durée d'une répétition en µs.
#   1. EGA 0Dh : copie écran → écran de 8000 octets par les latches (write
#      mode 1), le défilement de Commander Keen ; émulé.
#   2. EGA 0Dh : remplissage de 8000 octets, 4 plans, set/reset ; émulé.
#   3. Mode X : 320 colonnes de 200 points, un plan par colonne (Doom,
#      Wolfenstein) ; plan mappé.
#   4. Mode X : remplissage des 4 plans à la fois, 16000 octets ; émulé.
#   5. Mode 13h : remplissage de 64000 octets ; linéaire mappé.
    .code16
    .intel_syntax noprefix
    .text
    cld
    call calibrate
    mov ax, 0x000D              # --- 1 et 2 : EGA 320×200×16
    int 0x10
    mov bp, 20
    mov si, offset name1
    mov word ptr [test_fn], offset t1
    call measure
    mov bp, 20
    mov si, offset name2
    mov word ptr [test_fn], offset t2
    call measure
    mov ax, 0x0013              # --- 3 et 4 : mode X
    int 0x10
    mov dx, 0x3C4
    mov ax, 0x0604              # chain4 coupé
    out dx, ax
    mov dx, 0x3D4
    mov ax, 0x0014
    out dx, ax
    mov ax, 0xE317
    out dx, ax
    mov bp, 5
    mov si, offset name3
    mov word ptr [test_fn], offset t3
    call measure
    mov bp, 20
    mov si, offset name4
    mov word ptr [test_fn], offset t4
    call measure
    mov ax, 0x0013              # --- 5 : mode 13h
    int 0x10
    mov bp, 20
    mov si, offset name5
    mov word ptr [test_fn], offset t5
    call measure
    mov ax, 0x4C00
    int 0x21

# Cycles par µs : RDTSC sur 18 tics (18 × 54 925,4 µs = 988 657 µs).
calibrate:
    push ds
    xor ax, ax
    mov ds, ax
    mov bx, ds:[0x46C]
1:  cmp bx, ds:[0x46C]
    je 1b
    rdtsc
    mov cs:[t0], eax
    mov cs:[t0 + 4], edx
    mov bx, ds:[0x46C]
    add bx, 18
2:  cmp bx, ds:[0x46C]
    jne 2b
    rdtsc
    pop ds
    sub eax, [t0]
    sbb edx, [t0 + 4]
    mov ecx, 988657
    div ecx
    mov [cyc_us], eax
    ret

# BP répétitions de [test_fn] ; affiche le nom (SI) et les µs par répétition.
measure:
    push bp
    rdtsc
    mov [t0], eax
    mov [t0 + 4], edx
1:  call word ptr [test_fn]
    dec bp
    jnz 1b
    rdtsc
    sub eax, [t0]
    sbb edx, [t0 + 4]
    div dword ptr [cyc_us]      # µs au total
    pop bp
    movzx ecx, bp
    xor edx, edx
    div ecx                     # µs par répétition
    push eax
    mov dx, si
    mov ah, 0x09
    int 0x21
    pop eax
    call print_dec
    mov dx, offset unit
    mov ah, 0x09
    int 0x21
    ret

print_dec:
    mov ecx, 10
    xor bx, bx
1:  xor edx, edx
    div ecx
    push dx
    inc bx
    test eax, eax
    jnz 1b
2:  pop dx
    add dl, '0'
    mov ah, 0x02
    int 0x21
    dec bx
    jnz 2b
    ret

t1: push ds
    push es
    mov ax, 0xA000
    mov ds, ax
    mov es, ax
    mov dx, 0x3CE
    mov ax, 0x0105              # write mode 1
    out dx, ax
    xor si, si
    mov di, 0x2000
    mov cx, 8000
    rep movsb
    mov ax, 0x0005
    out dx, ax
    pop es
    pop ds
    ret

t2: push es
    mov ax, 0xA000
    mov es, ax
    mov dx, 0x3C4
    mov ax, 0x0F02
    out dx, ax
    mov dx, 0x3CE
    mov ax, 0x0F01              # set/reset sur les 4 plans
    out dx, ax
    mov ax, 0x0500              # couleur 5
    out dx, ax
    xor di, di
    mov cx, 8000
    rep stosb
    mov ax, 0x0001
    out dx, ax
    pop es
    ret

t3: push es
    mov ax, 0xA000
    mov es, ax
    xor bx, bx                  # colonne
1:  mov cl, bl
    and cl, 3
    mov ah, 1
    shl ah, cl
    mov al, 2
    mov dx, 0x3C4
    out dx, ax
    mov di, bx
    shr di, 2
    mov cx, 200
    mov al, bl
2:  mov es:[di], al
    add di, 80
    loop 2b
    inc bx
    cmp bx, 320
    jb 1b
    pop es
    ret

t4: push es
    mov ax, 0xA000
    mov es, ax
    mov dx, 0x3C4
    mov ax, 0x0F02
    out dx, ax
    xor di, di
    mov ax, 0x0707
    mov cx, 8000
    rep stosw
    pop es
    ret

t5: push es
    mov ax, 0xA000
    mov es, ax
    xor di, di
    mov ax, 0x0909
    mov cx, 32000
    rep stosw
    pop es
    ret

name1:  .ascii "1 EGA, copie par latches (8000 o)      : $"
name2:  .ascii "2 EGA, remplissage 4 plans (8000 o)    : $"
name3:  .ascii "3 mode X, colonnes un plan (64000 pts) : $"
name4:  .ascii "4 mode X, remplissage 4 plans (16000 o): $"
name5:  .ascii "5 mode 13h, remplissage (64000 o)      : $"
unit:   .ascii " us\r\n$"
        .balign 4
t0:     .long 0, 0
cyc_us: .long 0
test_fn: .word 0
