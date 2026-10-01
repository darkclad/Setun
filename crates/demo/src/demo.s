; Setun-386 demo ROM.
;
; Boots from the reset vector (ring +1, paging off) and shows a text screen
; with machine facts, the RAM size, a live Unix clock and the 27 text colours;
; echoes the keyboard and the serial console into a typing area; Tab draws a
; sweep through the 3^9-colour space; CPU faults are reported on screen.
;
; Global registers:
;   r-1 cursor cell     r-2 Shift held (0/1)   r-3 framebuffer base
;   r-4 typing colour   r-5 keyboard ports     r-6 serial ports
;   r-7 display ports

ENTRY       = -59049            ; reset PC: -3^10
ITB         = -29160            ; vector table: a multiple of 729 in ROM frame -1
FB_BASE     = -1604164          ; first framebuffer tryte
STACK_TOP   = 9000

PROMPT_CELL = 14*80
TYPE_START  = PROMPT_CELL + 2
TYPE_END    = 29*80
CLOCK_CELL  = 8*80 + 13

; Text attributes: foreground R G B, background R G B, style (one trit each:
; -1 = 0 %, 0 = 50 %, +1 = 100 %).
A_TITLE     = 0t111TT1000       ; white on blue
A_BODY      = 0t111TTT000       ; white on black
A_YELLOW    = 0t11TTTT000
A_CYAN      = 0tT11TTT000
A_GREEN     = 0tT1TTTT000
A_FAULT     = 0t1111TT000       ; white on red

; USB HID usage codes
KEY_ENTER = 40
KEY_ESC   = 41
KEY_BS    = 42
KEY_TAB   = 43
KEY_LAST  = 56                  ; last code in the keymap
LSHIFT    = 225
RSHIFT    = 229

        .org ENTRY
entry:  li    sp, STACK_TOP
        li    r-3, FB_BASE
        li    r-5, 108                ; keyboard block
        li    r-6, 162                ; serial block
        li    r-7, 243                ; display block
        li    r-4, A_GREEN
        li    r-2, 0
        li    r1, ITB
        mtc   itb, r1
        la    r1, s_banner
        sub   r1, r1, 1
        call  serial_puts
        call  draw_screen

; ---- main loop: clock, keyboard, serial ---------------------------------------
main:   in    r1, 81                  ; RTC seconds
        li    r2, CLOCK_CELL
        li    r3, A_BODY
        call  print_num
        in    r1, [r-5+1]             ; keyboard COUNT
        cmp   r1, 0
        beq   check_serial
        in    r1, [r-5]               ; key event: +press / -release
        cmp   r1, 0
        blt   key_release
        cmp   r1, LSHIFT
        beq   shift_on
        cmp   r1, RSHIFT
        beq   shift_on
        cmp   r1, KEY_TAB
        beq   do_gfx
        cmp   r1, KEY_ESC
        beq   do_redraw
        cmp   r1, KEY_ENTER
        beq   key_enter
        cmp   r1, KEY_BS
        beq   key_bs
        cmp   r1, KEY_LAST
        bgt   main
        la    r3, keymap
        sub   r3, r3, 1               ; first tryte of the table
        add   r3, r3, r1
        cmp   r-2, 0
        beq   km_lookup
        add   r3, r3, 57              ; shifted half
km_lookup:
        ldt   r1, [r3]
        cmp   r1, 0
        beq   main
        call  echo
        b     main
key_enter:
        li    r1, 10
        call  echo
        b     main
key_bs: li    r1, 8
        call  echo
        b     main
shift_on:
        li    r-2, 1
        b     main
key_release:
        sub   r1, r0, r1
        cmp   r1, LSHIFT
        beq   shift_off
        cmp   r1, RSHIFT
        beq   shift_off
        b     main
shift_off:
        li    r-2, 0
        b     main
do_gfx: call  graphics
        call  draw_screen
        b     main
do_redraw:
        call  draw_screen
        b     main
check_serial:
        in    r1, [r-6+1]             ; serial RX_COUNT
        cmp   r1, 0
        beq   main
        in    r1, [r-6]
        call  echo
        b     main

; ---- draw_screen --------------------------------------------------------------
draw_screen:
        push  lr
        li    r1, -1
        out   r1, [r-7]               ; text mode
        mov   r5, r-3
        li    r6, 30*80
        li    r7, ' '
        li    r8, A_BODY
ds_fill:
        stt   r7, [r5]
        stt   r8, [r5+1]
        add   r5, r5, 3
        sub   r6, r6, 1
        bgt   ds_fill
        mov   r5, r-3
        li    r6, 80
        li    r8, A_TITLE
ds_bar: stt   r7, [r5]
        stt   r8, [r5+1]
        add   r5, r5, 3
        sub   r6, r6, 1
        bgt   ds_bar
        la    r1, s_title
        sub   r1, r1, 1
        li    r2, 2
        li    r3, A_TITLE
        call  puts
        la    r1, s_sub
        sub   r1, r1, 1
        li    r2, 2*80 + 2
        li    r3, A_YELLOW
        call  puts
        la    r1, s_l4
        sub   r1, r1, 1
        li    r2, 4*80 + 2
        li    r3, A_BODY
        call  puts
        la    r1, s_l5
        sub   r1, r1, 1
        li    r2, 5*80 + 2
        li    r3, A_BODY
        call  puts
        la    r1, s_ram
        sub   r1, r1, 1
        li    r2, 7*80 + 2
        li    r3, A_CYAN
        call  puts
        in    r1, 1                   ; SYSCTL RAM_FRAMES
        li    r3, A_BODY
        call  print_num
        la    r1, s_ram2
        sub   r1, r1, 1
        li    r3, A_CYAN
        call  puts
        la    r1, s_clock
        sub   r1, r1, 1
        li    r2, 8*80 + 2
        li    r3, A_CYAN
        call  puts
        la    r1, s_pal
        sub   r1, r1, 1
        li    r2, 10*80 + 2
        li    r3, A_CYAN
        call  puts
        li    r6, -13                 ; 27 swatches: background field = i
        li    r7, ' '
ds_pal: mul   r3, r6, 27
        add   r5, r2, r2
        add   r5, r5, r2
        add   r5, r5, r-3
        stt   r7, [r5]
        stt   r3, [r5+1]
        stt   r7, [r5+3]
        stt   r3, [r5+4]
        add   r2, r2, 2
        add   r6, r6, 1
        cmp   r6, 13
        ble   ds_pal
        la    r1, s_keys
        sub   r1, r1, 1
        li    r2, 12*80 + 2
        li    r3, A_CYAN
        call  puts
        call  clear_typing
        pop   lr
        ret

; ---- clear_typing: blank rows 14..29, prompt, cursor ------------------------------
clear_typing:
        li    r2, PROMPT_CELL
        add   r5, r2, r2
        add   r5, r5, r2
        add   r5, r5, r-3
        li    r6, TYPE_END - PROMPT_CELL + 80
        li    r7, ' '
        li    r8, A_BODY
ct_loop:
        stt   r7, [r5]
        stt   r8, [r5+1]
        add   r5, r5, 3
        sub   r6, r6, 1
        bgt   ct_loop
        add   r5, r2, r2
        add   r5, r5, r2
        add   r5, r5, r-3
        li    r7, '>'
        li    r8, A_YELLOW
        stt   r7, [r5]
        stt   r8, [r5+1]
        li    r-1, TYPE_START
        out   r-1, [r-7+7]            ; display CURSOR
        ret

; ---- puts: r1 = string (tryte address), r2 = cell, r3 = attribute -------------------
;      returns r2 = next cell; clobbers r4, r5
puts:   add   r5, r2, r2
        add   r5, r5, r2
        add   r5, r5, r-3
puts_loop:
        ldt   r4, [r1]
        cmp   r4, 0
        beq   puts_done
        stt   r4, [r5]
        stt   r3, [r5+1]
        add   r5, r5, 3
        add   r1, r1, 1
        add   r2, r2, 1
        b     puts_loop
puts_done:
        ret

; ---- print_num: r1 = value, r2 = cell, r3 = attribute ------------------------------
;      returns r2 = next cell; clobbers r1, r4..r7
print_num:
        add   r5, r2, r2
        add   r5, r5, r2
        add   r5, r5, r-3
        cmp   r1, 0
        bge   pn_pos
        li    r4, '-'
        stt   r4, [r5]
        stt   r3, [r5+1]
        add   r5, r5, 3
        add   r2, r2, 1
        sub   r1, r0, r1
pn_pos: li    r6, 0
pn_div: mod.l r7, r1, 10
        div.l r1, r1, 10
        push  r7
        add   r6, r6, 1
        cmp   r1, 0
        bgt   pn_div
pn_out: pop   r7
        add   r7, r7, '0'
        stt   r7, [r5]
        stt   r3, [r5+1]
        add   r5, r5, 3
        add   r2, r2, 1
        sub   r6, r6, 1
        bgt   pn_out
        ret

; ---- serial_puts: r1 = string -------------------------------------------------------
serial_puts:
        ldt   r4, [r1]
        cmp   r4, 0
        beq   sp_done
        out   r4, [r-6]
        add   r1, r1, 1
        b     serial_puts
sp_done:
        ret

; ---- echo: r1 = character -----------------------------------------------------------
echo:   push  lr
        cmp   r1, 10
        beq   echo_nl
        cmp   r1, 13
        beq   echo_nl
        cmp   r1, 8
        beq   echo_bs
        cmp   r1, 127
        beq   echo_bs
        out   r1, [r-6]
        add   r5, r-1, r-1
        add   r5, r5, r-1
        add   r5, r5, r-3
        stt   r1, [r5]
        stt   r-4, [r5+1]
        add   r-1, r-1, 1
        b     echo_wrap
echo_nl:
        li    r2, 10
        out   r2, [r-6]
        div   r2, r-1, 80             ; next row, under the prompt
        add   r2, r2, 1
        mul   r-1, r2, 80
        add   r-1, r-1, 2
        b     echo_wrap
echo_bs:
        li    r2, TYPE_START
        cmp   r-1, r2
        ble   echo_cur
        sub   r-1, r-1, 1
        add   r5, r-1, r-1
        add   r5, r5, r-1
        add   r5, r5, r-3
        li    r4, ' '
        stt   r4, [r5]
        li    r2, 8
        out   r2, [r-6]
        b     echo_cur
echo_wrap:
        li    r2, TYPE_END
        cmp   r-1, r2
        blt   echo_cur
        call  clear_typing
echo_cur:
        out   r-1, [r-7+7]
        pop   lr
        ret

; ---- graphics: sweep the 3^9-colour cube, wait for a key ----------------------------
;      pixel = R*729 + G*27 + B, each channel -13..13
graphics:
        out   r0, [r-7]               ; 640x480 graphics
        mov   r9, r-3
        li    r2, 0                   ; y
gy:     mul   r5, r2, 27              ; G from y
        div   r5, r5, 480
        sub   r5, r5, 13
        mul   r5, r5, 27
        li    r3, 0                   ; x
gx:     mul   r6, r3, 27              ; R from x
        div   r6, r6, 640
        sub   r6, r6, 13
        mul   r6, r6, 729
        add   r7, r3, r2              ; B from x + y
        mul   r7, r7, 27
        div   r7, r7, 1119
        sub   r7, r7, 13
        add   r6, r6, r5
        add   r6, r6, r7
        stt   r6, [r9]
        add   r9, r9, 1
        add   r3, r3, 1
        cmp   r3, 640
        blt   gx
        add   r2, r2, 1
        cmp   r2, 480
        blt   gy
gwait:  in    r1, [r-6+1]
        cmp   r1, 0
        bgt   gserial
        in    r1, [r-5+1]
        cmp   r1, 0
        beq   gwait
        in    r1, [r-5]
        cmp   r1, 0
        ble   gwait                   ; ignore key releases
        ret
gserial:
        in    r1, [r-6]
        ret

; ---- fault handler: every vector points here ---------------------------------------
fault:  li    sp, STACK_TOP
        li    r-3, FB_BASE
        li    r1, -1
        li    r2, 243
        out   r1, [r2]                ; back to text mode
        mfc   r1, fcause
        ext   r1, r1, 23, 5           ; vector number
        push  r1
        la    r1, s_fault
        sub   r1, r1, 1
        li    r2, 29*80 + 2
        li    r3, A_FAULT
        call  puts
        pop   r1
        call  print_num
fault_halt:
        hlt
        b     fault_halt

; ---- data -----------------------------------------------------------------------------
s_banner: .string "Setun-386 demo ROM ready. Type here or in the window.\n"
s_title:  .string "SETUN-386 · balanced ternary computer"
s_sub:    .string "Every size is a power of 3.  After Brusentsov's Setun, MSU 1958/1970."
s_l4:     .string "tryte = 9 trits    word = 27 trits    address space = 3^27 trytes"
s_l5:     .string "page = 3^9 trytes  27 registers       rings: -1 user, 0 drivers, +1 kernel"
s_ram:    .string "RAM: "
s_ram2:   .string " pages × 3^9 trytes"
s_clock:  .string "Unix time: "
s_pal:    .string "27 text colours: "
s_keys:   .string "Type to echo (keyboard or serial).  Tab = 3^9-colour graphics.  Esc = redraw."
s_fault:  .string "CPU fault, vector "

; HID usage -> character: 57 unshifted entries, then 57 shifted.
keymap:
        .tryte 0, 0, 0, 0
        .tryte 'a','b','c','d','e','f','g','h','i','j','k','l','m'
        .tryte 'n','o','p','q','r','s','t','u','v','w','x','y','z'
        .tryte '1','2','3','4','5','6','7','8','9','0'
        .tryte 0, 0, 0, 0, ' ', '-', '=', '[', ']', '\\', 0, ';', '\'', '`', ',', '.', '/'
        .tryte 0, 0, 0, 0
        .tryte 'A','B','C','D','E','F','G','H','I','J','K','L','M'
        .tryte 'N','O','P','Q','R','S','T','U','V','W','X','Y','Z'
        .tryte '!','@','#','$','%','^','&','*','(',')'
        .tryte 0, 0, 0, 0, ' ', '_', '+', '{', '}', '|', 0, ':', '"', '~', '<', '>', '?'

; ---- vector table: 243 words centred on ITB, all -> fault ------------------------------
        .org ITB - 3*121
        .fill 243, fault
