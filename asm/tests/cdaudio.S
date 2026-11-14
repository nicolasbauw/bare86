# Pistes audio par MSCDEX (DOS intégré) : disque de deux pistes, la
# seconde audio, montée en D:.
# Code de sortie : 0, ou n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    mov byte ptr [cb], 10       # 1. disque audio : pistes 1 à 2
    call ioctl
    mov di, 1
    cmp word ptr [cb + 1], 0x0201
    jne fail
    mov byte ptr [cb], 11       # 2. piste 2 : audio, début lu
    mov byte ptr [cb + 1], 2
    call ioctl
    mov di, 2
    test byte ptr [cb + 6], 0x40
    jnz fail
    mov eax, [cb + 2]
    mov [start], eax
    mov byte ptr [req + 2], 132 # 3. lecture de 300 secteurs (4 s), Red Book
    mov byte ptr [req + 13], 1
    mov eax, [start]
    mov [req + 14], eax
    mov dword ptr [req + 18], 300
    call request
    mov di, 3
    cmp word ptr [req + 3], 0x0300          # fait + occupé
    jne fail
    mov ah, 0                   # 4. ≈ 4 tics plus tard, la position a avancé
    int 0x1A
    mov bx, dx
wait:
    mov ah, 0
    int 0x1A
    sub dx, bx
    cmp dx, 4
    jb wait
    mov byte ptr [cb], 12
    call ioctl
    mov di, 4
    cmp byte ptr [cb + 2], 0x02             # piste 2 (BCD)
    jne fail
    mov ax, [cb + 4]            # secondes:minutes relatives (octets 4-5 = min, s)
    mov dl, [cb + 6]            # trames
    or al, ah
    or al, dl
    jz fail                     # toujours au tout début : rien ne joue
    mov byte ptr [req + 2], 133 # 5. arrêt → pause
    call request
    mov byte ptr [cb], 15
    call ioctl
    mov di, 5
    test byte ptr [cb + 1], 1
    jz fail
    mov eax, [cb + 3]
    cmp eax, [start]
    jne fail
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21

# IOCTL d'entrée sur le bloc de contrôle `cb`.
ioctl:
    mov byte ptr [req + 2], 3
    mov word ptr [req + 14], offset cb
    mov [req + 16], ds
request:
    mov ax, 0x1510
    mov cx, 3
    mov bx, offset req
    int 0x2F
    ret

start:
    .long 0
req:
    .byte 26
    .space 25
cb:
    .space 16
