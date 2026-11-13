# Fichiers par FCB : 29h (analyse du nom), 16h, 15h, 10h, 0Fh, 14h, 23h,
# 21h, 11h, 13h. Code de sortie : 0, ou n° du contrôle en échec (DI).
    .code16
    .intel_syntax noprefix
    .text
    mov si, offset name         # 1. « FCBTEST.DAT » analysé dans le FCB
    mov di, offset fcb
    mov ax, 0x2901
    int 0x21
    mov di, 1
    test al, al
    jnz fail
    mov dx, offset fcb          # 2. création
    mov ah, 0x16
    int 0x21
    mov di, 2
    test al, al
    jnz fail
    mov word ptr [fcb + 0x0E], 16   # enregistrements de 16 octets
    mov dx, offset buf1         # 3. deux écritures séquentielles
    mov ah, 0x1A
    int 0x21
    mov dx, offset fcb
    mov ah, 0x15
    int 0x21
    mov di, 3
    test al, al
    jnz fail
    mov dx, offset buf2
    mov ah, 0x1A
    int 0x21
    mov dx, offset fcb
    mov ah, 0x15
    int 0x21
    test al, al
    jnz fail
    mov dx, offset fcb          # 4. fermeture, réouverture
    mov ah, 0x10
    int 0x21
    mov dx, offset fcb
    mov ah, 0x0F
    int 0x21
    mov di, 4
    test al, al
    jnz fail
    cmp word ptr [fcb + 0x10], 32   # taille du fichier
    jne fail
    mov word ptr [fcb + 0x0E], 16
    mov dx, offset rbuf         # 5. lecture séquentielle du 1er enregistrement
    mov ah, 0x1A
    int 0x21
    mov dx, offset fcb
    mov ah, 0x14
    int 0x21
    mov di, 5
    test al, al
    jnz fail
    cmp byte ptr [rbuf + 15], 'A'
    jne fail
    mov dx, offset fcb          # 6. taille en enregistrements (23h) : 2
    mov ah, 0x23
    int 0x21
    mov di, 6
    test al, al
    jnz fail
    cmp word ptr [fcb + 0x21], 2
    jne fail
    mov word ptr [fcb + 0x21], 1    # 7. lecture aléatoire de l'enregistrement 1
    mov dx, offset fcb
    mov ah, 0x21
    int 0x21
    mov di, 7
    test al, al
    jnz fail
    cmp byte ptr [rbuf], 'B'
    jne fail
    mov dx, offset fcb          # 8. fin de fichier en lecture séquentielle
    mov ah, 0x14
    int 0x21
    mov dx, offset fcb
    mov ah, 0x14
    int 0x21
    mov di, 8
    cmp al, 1
    jne fail
    mov dx, offset fcb
    mov ah, 0x10
    int 0x21
    mov dx, offset dta          # 9. recherche « FCBTEST.??? »
    mov ah, 0x1A
    int 0x21
    mov dx, offset pat
    mov ah, 0x11
    int 0x21
    mov di, 9
    test al, al
    jnz fail
    cmp word ptr [dta + 9], 0x4144  # « DA » de « DAT »
    jne fail
    mov dx, offset fcb          # 10. suppression, puis l'ouverture échoue
    mov ah, 0x13
    int 0x21
    mov di, 10
    test al, al
    jnz fail
    mov dx, offset fcb
    mov ah, 0x0F
    int 0x21
    cmp al, 0xFF
    jne fail
    xor di, di
fail:
    mov ax, di
    mov ah, 0x4C
    int 0x21
name: .asciz "FCBTEST.DAT"
pat: .byte 0
    .ascii "FCBTEST ???"
    .fill 25, 1, 0
buf1: .fill 16, 1, 'A'
buf2: .fill 16, 1, 'B'
rbuf: .fill 16, 1, 0
fcb: .fill 37, 1, 0
dta: .fill 64, 1, 0
