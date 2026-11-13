# Détection du 386 par FLAGS : les bits 12 à 14 (IOPL, NT) doivent rester
# posés après POPF en mode réel (sur 286 ils retombent à 0).
# Code de sortie : bits 12 à 14 relus, décalés (7 attendu).
    .code16
    .intel_syntax noprefix
    .text
    pushf
    pop ax
    or ax, 0x7000
    push ax
    popf
    pushf
    pop ax
    shr ax, 12
    and al, 7
    mov ah, 0x4C
    int 0x21
