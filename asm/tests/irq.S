# IRQ 0 réelle : INT 1Ch détournée, attente sur HLT de 5 tics (18,2 Hz),
# puis PIT reprogrammé à 1 kHz et 100 IRQ 0 comptées par un INT 08h chaîné
# sur celui du BIOS ; verrouillage du compteur. Code de sortie : 0, ou
# n° du contrôle en échec.
    .code16
    .intel_syntax noprefix
    .text
    xor ax, ax
    mov es, ax
    cli
    mov word ptr es:[0x1C * 4], offset tick1c
    mov word ptr es:[0x1C * 4 + 2], cs
    sti
1:  hlt                         # 1. cinq tics
    cmp word ptr [count1c], 5
    jb 1b
    mov eax, es:[0x6C * 1 + 0x400]
    mov di, 1
    test eax, eax
    jz fail
    cli                         # 2. INT 08h chaîné, PIT à 1 kHz
    mov eax, es:[0x08 * 4]
    mov [old08], eax
    mov word ptr es:[0x08 * 4], offset tick08
    mov word ptr es:[0x08 * 4 + 2], cs
    mov al, 0x34
    out 0x43, al
    mov ax, 1193
    out 0x40, al
    mov al, ah
    out 0x40, al
    sti
2:  hlt
    cmp word ptr [count08], 100
    jb 2b
    mov al, 0x00                # 3. compteur verrouillé : sous 1193
    out 0x43, al
    in al, 0x40
    mov ah, al
    in al, 0x40
    xchg al, ah
    mov di, 3
    cmp ax, 1193
    ja fail
    cli                         # remise en état : 18,2 Hz, vecteurs d'origine
    mov al, 0x36
    out 0x43, al
    xor al, al
    out 0x40, al
    out 0x40, al
    mov eax, [old08]
    mov es:[0x08 * 4], eax
    sti
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21

tick1c:
    inc word ptr cs:[count1c]
    iret
tick08:
    inc word ptr cs:[count08]
    jmp far ptr cs:[old08]

count1c: .word 0
count08: .word 0
old08: .long 0
