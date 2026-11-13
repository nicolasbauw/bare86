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

    # Table DBCS vide du DOS (INT 21h AX=6300h) : aucun octet de tête.
    .org 0xE700, 0xFF
dbcs_table:
    .word 0

    # Conversion de casse des informations nationales (INT 21h AH=38h) :
    # rien à convertir au-delà de l'ASCII.
    .org 0xE710, 0xFF
case_map:
    retf

    # IRQ sans gestionnaire : fin d'interruption au 8259 maître (IRQ 2–7)
    # ou aux deux (IRQ 8–15).
    .org 0xE820, 0xFF
irq_master_default:
    push ax
    mov al, 0x20
    out 0x20, al
    pop ax
    iret
    .org 0xE830, 0xFF
irq_slave_default:
    push ax
    mov al, 0x20
    out 0xA0, al
    out 0x20, al
    pop ax
    iret
    # Vecteurs sans effet (INT 1Ch, tic utilisateur) : sans sortie de VM.
    .org 0xE840, 0xFF
dummy_iret:
    iret

    # Reprise d'un appel qui attend une touche : l'hôte rétablit les registres
    # d'entrée et place IP ici ; l'IRQ 1 réveille le HLT, puis l'appel est
    # refait sur le stub du vecteur (le cadre de l'INT d'origine reste en pile).
    .org 0xE850, 0xFF
wait16:
    sti
    hlt
    cli
    jmp stubs + 0x16 * 16
    .org 0xE860, 0xFF
wait21:
    sti
    hlt
    cli
    jmp stubs + 0x21 * 16

    # INT 33h : AX=0003h (position et boutons) servi ici sans sortie de VM,
    # d'après le bloc d'état que l'hôte tient à jour (lecture seule pour
    # l'invité) ; les autres fonctions passent par l'hôte.
    .org 0xE870, 0xFF
int33:
    cmp ax, 3
    jne 1f
    mov bx, cs:[mouse_state]
    mov cx, cs:[mouse_state + 2]
    mov dx, cs:[mouse_state + 4]
    iret
1:  jmp stubs + 0x33 * 16
    .org 0xE890, 0xFF
mouse_state:
    .word 0, 320, 100

    # INT 09h, IRQ 1 (adresse de l'IBM AT) : scancode lu au 8042, proposé à
    # INT 15h AH=4Fh, puis confié à l'hôte (port E1h) qui tient la BDA.
    .org 0xE987, 0xFF
int09:
    push ax
    in al, 0x60
    mov ah, 0x4F
    stc
    int 0x15
    jnc 1f
    out 0xE1, al
1:  mov al, 0x20
    out 0x20, al
    pop ax
    iret

    # INT 08h, IRQ 0 (adresse de l'IBM AT) : tics de la BDA, passage de
    # minuit, INT 1Ch, puis fin d'interruption.
    .org 0xFEA5, 0xFF
int08:
    push ds
    push ax
    mov ax, 0x40
    mov ds, ax
    inc word ptr ds:[0x6C]
    jnz 1f
    inc word ptr ds:[0x6E]
1:  cmp word ptr ds:[0x6E], 0x18
    jne 2f
    cmp word ptr ds:[0x6C], 0xB0
    jne 2f
    mov word ptr ds:[0x6C], 0
    mov word ptr ds:[0x6E], 0
    mov byte ptr ds:[0x70], 1
2:  int 0x1C
    mov al, 0x20
    out 0x20, al
    pop ax
    pop ds
    iret

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
