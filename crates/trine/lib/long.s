; ---- long runtime: generated code calls these for 81-trit integers ----------
;
; A long is three words: in registers r1 (low), r2 (middle), r3 (high); in
; memory the high word first (the long's address is its high word's). A
; binary operation finds its left operand at [sp] (pushed r1, r2, r3, so laid
; out as in memory) and its right one in r1..r3, and leaves the result in
; r1..r3. Every other register is kept (f registers too, but f1 for a
; conversion to real), so code around a call keeps its values in them.
; Subtraction adds the negation: SBC subtracts the carry where multi-word
; subtraction needs it added.
_trine_ladd:
        push  r4
        ldw   r4, [sp+9]
        add   r1, r4, r1
        ldw   r4, [sp+6]
        adc   r2, r4, r2
        ldw   r4, [sp+3]
        adc   r3, r4, r3
        pop   r4
        ret
_trine_lsub:
        neg   r1, r1
        neg   r2, r2
        neg   r3, r3
        b     _trine_ladd
; tritwise minimum and maximum (& and |)
_trine_lmin:
        push  r4
        ldw   r4, [sp+9]
        min   r1, r4, r1
        ldw   r4, [sp+6]
        min   r2, r4, r2
        ldw   r4, [sp+3]
        min   r3, r4, r3
        pop   r4
        ret
_trine_lmax:
        push  r4
        ldw   r4, [sp+9]
        max   r1, r4, r1
        ldw   r4, [sp+6]
        max   r2, r4, r2
        ldw   r4, [sp+3]
        max   r3, r4, r3
        pop   r4
        ret
; r1 := sign(left - right), exact: the high words decide, then the others
_trine_lcmp:
        push  r4
        ldw   r4, [sp+3]
        cmp   r4, r3
        bne   _trine_lcmp_s
        ldw   r4, [sp+6]
        cmp   r4, r2
        bne   _trine_lcmp_s
        ldw   r4, [sp+9]
        cmp   r4, r1
_trine_lcmp_s:
        li    r1, 0
        beq   _trine_lcmp_d
        li    r1, 1
        bgt   _trine_lcmp_d
        li    r1, -1
_trine_lcmp_d:
        pop   r4
        ret
; the low 81 trits of the product: with W = 3^27, a = a2 W^2 + a1 W + a0 and
; ai bj = hij W + lij (MULH, MUL), the result is l00, h00 + l01 + l10 and
; h01 + h10 + l02 + l11 + l20 plus the middle word's carries
_trine_lmul:
        push  r4
        push  r5
        push  r6
        push  r7
        push  r8
        push  r9
        push  r10
        ldw   r4, [sp+27]             ; a0
        ldw   r5, [sp+24]             ; a1
        ldw   r6, [sp+21]             ; a2
        mul   r7, r4, r1              ; l00
        mulh  r8, r4, r1              ; h00
        mul   r9, r4, r2              ; l01
        add   r8, r8, r9
        adc   r10, r0, r0             ; its carry
        mul   r9, r5, r1              ; l10
        add   r8, r8, r9
        adc   r10, r10, r0
        mulh  r9, r4, r2              ; h01
        add   r10, r10, r9
        mulh  r9, r5, r1              ; h10
        add   r10, r10, r9
        mul   r9, r4, r3              ; l02
        add   r10, r10, r9
        mul   r9, r5, r2              ; l11
        add   r10, r10, r9
        mul   r9, r6, r1              ; l20
        add   r10, r10, r9
        mov   r1, r7
        mov   r2, r8
        mov   r3, r10
        pop   r10
        pop   r9
        pop   r8
        pop   r7
        pop   r6
        pop   r5
        pop   r4
        ret
; the left operand shifted by r1 trits: left for r1 > 0 (high trits fall
; off), right for r1 < 0 (low trits fall off, rounding to nearest)
_trine_lsh:
        push  r4
        push  r5
        mov   r4, r1
        ldw   r3, [sp+6]
        ldw   r2, [sp+9]
        ldw   r1, [sp+12]
        cmp   r4, 81
        bge   _trine_lsh_z
        cmp   r4, -81
        ble   _trine_lsh_z
_trine_lsh_l:
        cmp   r4, 0
        ble   _trine_lsh_r
        trit  r5, r2, 1
        sh    r3, r3, 1
        add   r3, r3, r5
        trit  r5, r1, 1
        sh    r2, r2, 1
        add   r2, r2, r5
        sh    r1, r1, 1
        sub   r4, r4, 1
        b     _trine_lsh_l
_trine_lsh_r:
        cmp   r4, 0
        bge   _trine_lsh_d
        trit  r5, r2, 27
        sh    r5, r5, 26
        sh    r1, r1, -1
        add   r1, r1, r5
        trit  r5, r3, 27
        sh    r5, r5, 26
        sh    r2, r2, -1
        add   r2, r2, r5
        sh    r3, r3, -1
        add   r4, r4, 1
        b     _trine_lsh_r
_trine_lsh_z:
        li    r1, 0
        li    r2, 0
        li    r3, 0
_trine_lsh_d:
        pop   r5
        pop   r4
        ret
; quotient (truncated) and remainder (the sign of the dividend), as for
; int; dividing by zero traps as an int division does. On magnitudes, a
; trit of the dividend at a time: R := 3R + t, Q := 3Q, then R brought into
; 0 <= R < D. 3R cannot overflow while the divisor's top trit is 0; a
; bigger divisor goes into the dividend at most twice.
; Registers: Q r4..r6, R r7..r9, D r10, r-10, r-11 (low first), the
; dividend's current word r-12, its trit number r-13, the word's slot r1.
; Frame: [sp] the result wanted (+1 quotient, -1 remainder), [sp+3] the
; quotient's sign, [sp+6] the remainder's, [sp+9..15] the dividend's
; magnitude, [sp+18..48] the saved registers, [sp+51..57] the dividend.
_trine_ldiv:
        push  r4
        push  r5
        push  r6
        push  r7
        push  r8
        push  r9
        push  r10
        push  r-10
        push  r-11
        push  r-12
        push  r-13
        sub   sp, sp, 18
        li    r-12, 1
        stw   r-12, [sp]
        b     _trine_ldm
_trine_lmod:
        push  r4
        push  r5
        push  r6
        push  r7
        push  r8
        push  r9
        push  r10
        push  r-10
        push  r-11
        push  r-12
        push  r-13
        sub   sp, sp, 18
        li    r-12, -1
        stw   r-12, [sp]
_trine_ldm:
        mov   r10, r1
        mov   r-10, r2
        mov   r-11, r3
        cmp   r-11, 0
        bne   _trine_ldm_d1
        cmp   r-10, 0
        bne   _trine_ldm_d1
        cmp   r10, 0
        bne   _trine_ldm_d1
        div   r1, r1, r0              ; by zero: the int division's trap
_trine_ldm_d1:
        li    r2, 1                   ; the divisor's sign
        bgt   _trine_ldm_d2
        li    r2, -1
        neg   r10, r10
        neg   r-10, r-10
        neg   r-11, r-11
_trine_ldm_d2:
        ldw   r9, [sp+51]
        ldw   r8, [sp+54]
        ldw   r7, [sp+57]
        li    r3, 1                   ; the dividend's sign
        cmp   r9, 0
        bne   _trine_ldm_a1
        cmp   r8, 0
        bne   _trine_ldm_a1
        cmp   r7, 0
_trine_ldm_a1:
        bge   _trine_ldm_a2
        li    r3, -1
        neg   r7, r7
        neg   r8, r8
        neg   r9, r9
_trine_ldm_a2:
        stw   r9, [sp+9]
        stw   r8, [sp+12]
        stw   r7, [sp+15]
        stw   r3, [sp+6]
        mul   r2, r2, r3
        stw   r2, [sp+3]
        li    r4, 0
        li    r5, 0
        li    r6, 0
        trit  r2, r-11, 1
        cmp   r2, 0
        bne   _trine_ldm_big
        li    r7, 0
        li    r8, 0
        li    r9, 0
        li    r1, 9
_trine_ldm_w:
        add   r2, sp, r1
        ldw   r-12, [r2]
        li    r-13, 1
_trine_ldm_t:
        trit  r3, r-12, r-13          ; the dividend's next trit
        trit  r2, r8, 1               ; R := 3R + t
        sh    r9, r9, 1
        add   r9, r9, r2
        trit  r2, r7, 1
        sh    r8, r8, 1
        add   r8, r8, r2
        sh    r7, r7, 1
        add   r7, r7, r3
        trit  r2, r5, 1               ; Q := 3Q
        sh    r6, r6, 1
        add   r6, r6, r2
        trit  r2, r4, 1
        sh    r5, r5, 1
        add   r5, r5, r2
        sh    r4, r4, 1
        cmp   r9, 0                   ; R < 0: R += D, Q -= 1
        bne   _trine_ldm_s
        cmp   r8, 0
        bne   _trine_ldm_s
        cmp   r7, 0
_trine_ldm_s:
        bge   _trine_ldm_ge
        add   r7, r7, r10
        adc   r8, r8, r-10
        adc   r9, r9, r-11
        add   r4, r4, -1
        adc   r5, r5, r0
        adc   r6, r6, r0
_trine_ldm_ge:
        cmp   r9, r-11                ; while R >= D: R -= D, Q += 1
        bne   _trine_ldm_c
        cmp   r8, r-10
        bne   _trine_ldm_c
        cmp   r7, r10
_trine_ldm_c:
        blt   _trine_ldm_n
        neg   r10, r10                ; R -= D as R + (-D), Q += 1
        neg   r-10, r-10
        neg   r-11, r-11
        add   r7, r7, r10
        adc   r8, r8, r-10
        adc   r9, r9, r-11
        neg   r10, r10
        neg   r-10, r-10
        neg   r-11, r-11
        add   r4, r4, 1
        adc   r5, r5, r0
        adc   r6, r6, r0
        b     _trine_ldm_ge
_trine_ldm_n:
        add   r-13, r-13, 1
        cmp   r-13, 27
        ble   _trine_ldm_t
        add   r1, r1, 3
        cmp   r1, 15
        ble   _trine_ldm_w
        b     _trine_ldm_out
_trine_ldm_big:
        ldw   r9, [sp+9]
        ldw   r8, [sp+12]
        ldw   r7, [sp+15]
_trine_ldm_b:
        cmp   r9, r-11
        bne   _trine_ldm_b2
        cmp   r8, r-10
        bne   _trine_ldm_b2
        cmp   r7, r10
_trine_ldm_b2:
        blt   _trine_ldm_out
        neg   r10, r10                ; R -= D as R + (-D), Q += 1
        neg   r-10, r-10
        neg   r-11, r-11
        add   r7, r7, r10
        adc   r8, r8, r-10
        adc   r9, r9, r-11
        neg   r10, r10
        neg   r-10, r-10
        neg   r-11, r-11
        add   r4, r4, 1
        adc   r5, r5, r0
        adc   r6, r6, r0
        b     _trine_ldm_b
_trine_ldm_out:
        ldw   r2, [sp]
        cmp   r2, 0
        blt   _trine_ldm_rem
        mov   r1, r4
        mov   r2, r5
        mov   r3, r6
        ldw   r-12, [sp+3]
        b     _trine_ldm_fix
_trine_ldm_rem:
        mov   r1, r7
        mov   r2, r8
        mov   r3, r9
        ldw   r-12, [sp+6]
_trine_ldm_fix:
        cmp   r-12, 0
        bge   _trine_ldm_ret
        neg   r1, r1
        neg   r2, r2
        neg   r3, r3
_trine_ldm_ret:
        add   sp, sp, 18
        pop   r-13
        pop   r-12
        pop   r-11
        pop   r-10
        pop   r10
        pop   r9
        pop   r8
        pop   r7
        pop   r6
        pop   r5
        pop   r4
        ret
; a long to the nearest real: (hi W + mid) W + lo, the first sum exact
_trine_ltor:
        push  r4
        sub   sp, sp, 9
        fstd  f2, [sp]
        li    r4, 27
        fcvtif f1, r3
        fscl  f1, f1, r4
        fcvtif f2, r2
        fadd  f1, f1, f2
        fscl  f1, f1, r4
        fcvtif f2, r1
        fadd  f1, f1, f2
        fldd  f2, [sp]
        add   sp, sp, 9
        pop   r4
        ret
; a real to the nearest long (ties to even); a NaN gives 0, and values out
; of range the nearest end of it. Each word in turn, the rest exact.
_trine_rtol:
        push  r4
        sub   sp, sp, 27
        fstd  f1, [sp]
        fstd  f2, [sp+9]
        fstd  f3, [sp+18]
        fcmp  f1, f1
        bov   _trine_rtol_z
        li    r4, 1
        fcvtif f2, r4
        li    r4, 81
        fscl  f2, f2, r4              ; 3^81
        fadd  f3, f1, f1              ; 2x
        fcmp  f3, f2
        bge   _trine_rtol_max
        fneg  f2, f2
        fcmp  f3, f2
        ble   _trine_rtol_min
        li    r4, -54
        fscl  f2, f1, r4
        fcvtfi r3, f2
        fcvtif f2, r3
        li    r4, 54
        fscl  f2, f2, r4
        fsub  f1, f1, f2
        li    r4, -27
        fscl  f2, f1, r4
        fcvtfi r2, f2
        fcvtif f2, r2
        li    r4, 27
        fscl  f2, f2, r4
        fsub  f1, f1, f2
        fcvtfi r1, f1
        b     _trine_rtol_e
_trine_rtol_max:
        li    r1, 3812798742493
        li    r2, 3812798742493
        li    r3, 3812798742493
        b     _trine_rtol_e
_trine_rtol_min:
        li    r1, -3812798742493
        li    r2, -3812798742493
        li    r3, -3812798742493
        b     _trine_rtol_e
_trine_rtol_z:
        li    r1, 0
        li    r2, 0
        li    r3, 0
_trine_rtol_e:
        fldd  f1, [sp]
        fldd  f2, [sp+9]
        fldd  f3, [sp+18]
        add   sp, sp, 27
        pop   r4
        ret
