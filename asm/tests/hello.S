# Affiche une chaîne (INT 21h AH=09h) et quitte avec le code 0.
    .code16
    .intel_syntax noprefix
    .text
    mov dx, offset msg
    mov ah, 0x09
    int 0x21
    mov ax, 0x4C00
    int 0x21
msg: .ascii "Hello from KVM!\r\n$"
