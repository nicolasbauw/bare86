# SPDX-License-Identifier: GPL-3.0-or-later
# ROM BIOS (segment F000h, 64 Ko).
#
# F000:n*16 : stub du vecteur n, « out 0xE0, al ; iret ». L'hôte retrouve le
# vecteur à partir de l'IP (bits 4 à 11) et sert l'appel (BIOS, DOS en HLE).
# Un programme qui détourne un vecteur garde l'IVT ordinaire ; il peut
# chaîner sur l'ancien vecteur, qui pointe ici.

    .code16
    .intel_syntax noprefix
    .text

    .set HOST_CALL, 0xE0

stubs:
    .rept 256
    .balign 16, 0x90
    out HOST_CALL, al
    iret
    .endr

    # Table de configuration (INT 15h AH=C0h), à son adresse traditionnelle.
    .org 0xE6F5, 0xFF
config_table:
    .word 8                 # longueur
    .byte 0xFC, 0x01, 0x00  # modèle AT, sous-modèle, révision
    .byte 0x60              # 8259 esclave, RTC
    .byte 0, 0, 0, 0

    # Point d'entrée après réinitialisation (F000:FFF0) : pas de POST,
    # l'hôte prépare la machine et charge le programme directement.
    .org 0xFFF0, 0xFF
reset:
    hlt
    jmp reset

    .org 0xFFF5, 0xFF
    .ascii "11/12/26"       # date du BIOS (F000:FFF5)
    .org 0xFFFE, 0xFF
    .byte 0xFC              # modèle : AT
    .byte 0x00
