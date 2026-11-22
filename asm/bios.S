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

    # Souris PS/2 (INT 15h AX=C2xxh) : routine du pilote (déposée par l'hôte)
    # et INT 74h (IRQ 12), qui demande un paquet à l'hôte (AX ≠ 0 : état en
    # BX, X en CX, Y en DX) et le passe à la routine en pile : état, X, Y, 0.
    .org 0xE938, 0xFF
ps2_handler:
    .word 0, 0
    .org 0xE940, 0xFF
int74:
    jmp int74_body

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

    # Liste des modes VBE (INT 10h AX=4F00h), écrite par l'hôte depuis
    # `bios::VBE_MODE_LIST` : place réservée (64 modes et la fin).
    .org 0xEA00, 0xFF
vbe_modes:
    .fill 130, 1, 0xFF

    # Suite de l'INT 74h (IRQ 12). Souris PS/2 du BIOS : paquet demandé à
    # l'hôte (AX ≠ 0 : état en BX, X en CX, Y en DX), passé à la routine du
    # pilote en pile (état, X, Y, 0). Pilote INT 33h intégré : événement
    # demandé à l'hôte (AX = conditions ≠ 0, BX boutons, CX/DX position,
    # SI/DI mickeys), gestionnaire de l'utilisateur appelé (adresse déposée
    # par l'hôte en mouse33_handler).
    .org 0xEAA0, 0xFF
int74_body:
    pusha
    push ds
    push es
    xor ax, ax
    .org 0xEAA8, 0x90
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
1:  xor ax, ax
    .org 0xEAC0, 0x90
mouse33_fetch:
    out HOST_CALL, al
    test ax, ax
    jz 2f
    call dword ptr cs:[mouse33_handler]
2:  mov al, 0x20
    out 0xA0, al
    out 0x20, al
    pop es
    pop ds
    popa
    iret
    .org 0xEAE0, 0xFF
mouse33_handler:
    .word 0, 0

    # Étalonnage de `Delay` par l'unité CRT de Turbo/Borland Pascal 7
    # (`not ax; not dx; mov cx,55; div cx`), remplacé par un appel lointain
    # ici (`dos::crt_delay_patch`) : boucles comptées en DX:AX pendant un
    # tic, divisées par 55 ms. Au-delà de 55 × 65 536 (tout processeur
    # récent), la division déborde (« Runtime error 200 ») : plafonnée.
    .org 0xEAF0, 0xFF
crt_delay_fix:
    not ax
    not dx
    mov cx, 55
    cmp dx, cx
    jb 1f
    mov ax, 0xFFFF
    retf
1:  div cx
    retf

    # INT 16h : AH=01h/11h (touche prête ?) servis ici sans sortie de VM
    # quand le tampon de la BDA est vide (ZF = 1 dans le cadre) ; le reste
    # passe par l'hôte. Des programmes l'appellent en boucle pendant leurs
    # attentes (chargeur de Second Reality : ≈ 40 000 fois par seconde).
    .org 0xEB10, 0xFF
int16:
    sti                         # comme le BIOS d'IBM
    cmp ah, 0x01
    je 1f
    cmp ah, 0x11
    jne 2f
1:  push ds
    push ax
    push 0x40
    pop ds
    mov ax, ds:[0x1A]
    cmp ax, ds:[0x1C]
    pop ax
    pop ds
    jne 2f
    push bp
    mov bp, sp
    or word ptr [bp + 6], 0x40
    pop bp
    iret
2:  jmp stubs + 0x16 * 16

    # VBE 2.0 : interface en mode protégé 32 bits (INT 10h AX=4F0Ah BL=0). Les
    # programmes copient cette table, puis appellent ses fonctions (appel
    # proche, registres de 4F05h, 4F07h et 4F09h) depuis leur propre segment.
    # Les ports 1D0h (fenêtre), 1D2h et 1D4h (début d'affichage : x puis y)
    # sont ceux de l'hôte ; la palette passe par le DAC (3C8h-3C9h).
    .org 0xEC00, 0xFF
vbe_pm:
    .word pm_window - vbe_pm, pm_start - vbe_pm, pm_palette - vbe_pm, pm_ports - vbe_pm
pm_window:
    .code32
    cmp bh, 1
    je 1f
    push eax
    push edx
    mov ax, dx
    mov dx, 0x1D0
    out dx, ax
    pop edx
    pop eax
    ret
1:  push eax
    mov dx, 0x1D0
    in ax, dx
    mov dx, ax
    pop eax
    ret
pm_start:
    push eax
    push edx
    mov ax, cx
    mov dx, 0x1D2
    out dx, ax
    pop edx
    push edx
    mov ax, dx
    mov dx, 0x1D4
    out dx, ax
    pop edx
    pop eax
    ret
pm_palette:
    push eax
    push ecx
    push edx
    push edi
    movzx ecx, cx
    mov al, dl
    mov dx, 0x3C8
    out dx, al
    inc dx
2:  mov al, es:[edi + 2]
    out dx, al
    mov al, es:[edi + 1]
    out dx, al
    mov al, es:[edi]
    out dx, al
    add edi, 4
    loop 2b
    pop edi
    pop edx
    pop ecx
    pop eax
    ret
    .code16
pm_ports:
    .word 0x1D0, 0x1D2, 0x1D4, 0x3C0, 0x3C4, 0x3C5, 0x3C6, 0x3C7, 0x3C8, 0x3C9, 0x3CE, 0x3CF, 0x3D4, 0x3D5, 0x3DA, 0xFFFF
    .word 0xFFFF
vbe_pm_end:

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
