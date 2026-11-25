    .code16
    .intel_syntax noprefix
    .text
start:
    mov bx, 0x1000
    mov ah, 0x4a
    int 0x21
    mov ax, 0x1687
    int 0x2f
    test ax, ax
    jnz fail1
    mov word ptr [entry], di
    mov word ptr [entry+2], es
    test si, si
    jz noalloc
    mov bx, si
    mov ah, 0x48
    int 0x21
    jc fail2
    mov es, ax
noalloc:
    mov ax, 1                   # 32-bit client (HDPMI32)
    call dword ptr [entry]
    jc fail3
    # protected mode, 16-bit client
    mov ebx, 0x10000000
    mov ecx, 0x2D000
    mov edx, 1
    mov ax, 0x0504
    int 0x31
    sbb ebp, ebp                # -1 if carry
    mov esi, eax
    mov edi, ebx
    mov al, 'C'
    call putc
    mov eax, ebp
    call puthex
    mov al, 'A'
    call putc
    mov eax, esi
    call puthex
    mov al, 'B'
    call putc
    mov eax, edi
    call puthex
    mov al, 13
    call putc
    mov al, 10
    call putc
    jmp done
fail1:
    mov dx, offset m1
    jmp pr
fail2:
    mov dx, offset m2
    jmp pr
fail3:
    mov dx, offset m3
pr:
    mov ah, 9
    int 0x21
    jmp done
fail:
    mov dx, offset msg
    mov ah, 9
    int 0x21
done:
    mov ax, 0x4c00
    int 0x21
putc:
    push eax
    push edx
    mov dl, al
    mov ah, 2
    int 0x21
    pop edx
    pop eax
    ret
puthex:
    push ebx
    push ecx
    mov ebx, eax
    mov ecx, 8
1:  rol ebx, 4
    mov al, bl
    and al, 15
    cmp al, 10
    jb 2f
    add al, 7
2:  add al, '0'
    call putc
    loop 1b
    pop ecx
    pop ebx
    ret
entry: .long 0
msg: .ascii "fail$"
m1: .ascii "no host$"
m2: .ascii "alloc$"
m3: .ascii "enterPM$"
