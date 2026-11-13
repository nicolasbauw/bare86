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

    # Point d'entrée XMS (INT 2Fh AX=4310h) : saut court suivi de trois NOP,
    # que la norme permet de détourner, puis appel hôte et retour lointain.
    .org 0xE8A0, 0xFF
xms_entry:
    .byte 0xEB, 0x03, 0x90, 0x90, 0x90
    out HOST_CALL, al
    retf

    # EMS : le vecteur INT 67h pointe en FE8Bh:0012h ; ce segment commence
    # par un en-tête de périphérique dont le nom (offset 0Ah) est EMMXXXX0,
    # que les programmes vérifient pour détecter le pilote.
    .org 0xE8B0, 0xFF
ems_device:
    .word 0xFFFF, 0xFFFF, 0x8000, 0, 0
    .ascii "EMMXXXX0"
    jmp 0xF000:(stubs + 0x67 * 16)

    # VBE : fonction de fenêtre (appel lointain, BH=0 réglage / 1 lecture,
    # BL=fenêtre, DX=position), qui relaie vers INT 10h AX=4F05h ; chaînes
    # du bloc d'information et liste des modes (INT 10h AX=4F00h).
    .org 0xE8D0, 0xFF
vbe_win_func:
    push ax
    mov ax, 0x4F05
    int 0x10
    pop ax
    retf
    .org 0xE8E0, 0xFF
vbe_oem:
    .asciz "Bare86"
    .org 0xE8F0, 0xFF
vbe_vendor:
    .asciz "Bare86"
    .org 0xE900, 0xFF
vbe_product:
    .asciz "SVGA VBE 2.0"
    .org 0xE910, 0xFF
vbe_revision:
    .asciz "0.1"
    .org 0xE918, 0xFF
vbe_modes:
    .word 0x100, 0x101, 0x103, 0x105
    .word 0x110, 0x111, 0x112, 0x113, 0x114, 0x115, 0x116, 0x117, 0x118
    .word 0xFFFF

    # Souris PS/2 (INT 15h AX=C2xxh) : routine du pilote (déposée par l'hôte)
    # et INT 74h (IRQ 12), qui demande un paquet à l'hôte (AX ≠ 0 : état en
    # BX, X en CX, Y en DX) et le passe à la routine en pile : état, X, Y, 0.
    .org 0xE938, 0xFF
ps2_handler:
    .word 0, 0
    .org 0xE940, 0xFF
int74:
    pusha
    push ds
    push es
    xor ax, ax
    .org 0xE948, 0x90
ps2_fetch:
    out HOST_CALL, al
    test ax, ax
    jz 1f
    push bx
    push cx
    push dx
    push 0
    call dword ptr cs:[ps2_handler]
    add sp, 8
1:  mov al, 0x20
    out 0xA0, al
    out 0x20, al
    pop es
    pop ds
    popa
    iret

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

    # Table des paramètres de disquette (vecteur 1Eh, INT 13h AH=08h), à son
    # adresse traditionnelle : 1,44 Mo, 18 secteurs par piste.
    .org 0xEFC7, 0xFF
diskette_params:
    .byte 0xDF, 0x02, 0x25, 0x02, 0x12, 0x1B, 0xFF, 0x6C, 0xF6, 0x0F, 0x08

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

    # Point d'entrée après réinitialisation (F000:FFF0) : pas de POST.
    # Un saut ici (redémarrage à chaud) devient un INT 19h servi par l'hôte.
    .org 0xFFF0, 0xFF
    jmp 0xF000:0x0190

    .org 0xFFF5, 0xFF
    .ascii "11/12/26"       # date du BIOS (F000:FFF5)
    .org 0xFFFE, 0xFF
    .byte 0xFC              # modèle : AT
    .byte 0x00
