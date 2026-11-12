# Quitte avec le code 42 (INT 21h AH=4Ch).
    .code16
    .intel_syntax noprefix
    .text
    mov ax, 0x4C2A
    int 0x21
