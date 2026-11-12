# Appelle un service non géré (INT 99h) puis quitte avec le code 7.
    .code16
    .intel_syntax noprefix
    .text
    int 0x99
    mov ax, 0x4C07
    int 0x21
