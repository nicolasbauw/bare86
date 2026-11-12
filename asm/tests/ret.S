# RET final : saute en PSP:0000 (INT 20h), code de sortie 0.
    .code16
    .intel_syntax noprefix
    .text
    mov dl, 'R'
    mov ah, 0x02
    int 0x21
    ret
