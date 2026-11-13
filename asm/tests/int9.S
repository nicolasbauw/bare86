# Lecture du clavier façon jeu : INT 09h détournée, scancodes lus sur le
# port 60h, fin d'interruption au PIC. Une première touche est lue par
# INT 16h (les suivantes attendent qu'elle le soit) ; puis trois appuis
# attendus sur HLT ; vecteur rétabli, sortie avec le dernier scancode.
    .code16
    .intel_syntax noprefix
    .text
    xor ah, ah
    int 0x16
    xor ax, ax
    mov es, ax
    cli
    mov eax, es:[0x09 * 4]
    mov [old09], eax
    mov word ptr es:[0x09 * 4], offset kbd
    mov word ptr es:[0x09 * 4 + 2], cs
    sti
1:  hlt
    cmp byte ptr [presses], 3
    jb 1b
    cli
    mov eax, [old09]
    mov es:[0x09 * 4], eax
    sti
    mov al, [last]
    mov ah, 0x4C
    int 0x21

kbd:
    push ax
    in al, 0x60
    test al, 0x80
    jnz 2f
    mov cs:[last], al
    inc byte ptr cs:[presses]
2:  mov al, 0x20
    out 0x20, al
    pop ax
    iret

presses: .byte 0
last: .byte 0
old09: .long 0
