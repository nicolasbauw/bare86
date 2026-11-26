	.386
	.MODEL FLAT, c
	option casemap:none
	.CODE
;--- trace output to DOS stderr (replaces int 41h ax=2 which nobody handles here)
__dbgstr_esi proc public
	push eax
	push ebx
	push ecx
	push edx
	mov edx, esi
	xor ecx, ecx
@@:
	cmp byte ptr [esi+ecx], 0
	jz @F
	inc ecx
	jmp @B
@@:
	mov ebx, 2
	mov ah, 40h
	int 21h
	pop edx
	pop ecx
	pop ebx
	pop eax
	ret
__dbgstr_esi endp

__dw2aDebug proc stdcall public
	pushad
	sub esp, 16
	mov edi, esp
	mov ecx, 8
dwl:
	rol eax, 4
	mov dl, al
	and dl, 0Fh
	add dl, '0'
	cmp dl, '9'
	jbe dwok
	add dl, 7
dwok:
	mov [edi], dl
	inc edi
	dec ecx
	jnz dwl
	mov byte ptr [edi], ' '
	mov byte ptr [edi+1], 0
	mov esi, esp
	call __dbgstr_esi
	add esp, 16
	popad
	ret
__dw2aDebug endp
	end
