; diagonal.s — draws a diagonal of '*' on the 32x32 character plane
; and prints "Hello, silicon!" to the console via OUT.
;
; Demonstrates: labels, indexed addressing (STA $F800,X), the TXA/A/TAX
; trick to increment X, and the CMP/JNZ loop idiom.

.origin $8000

        LDA #$48                 ; 'H'
        OUT
        LDA #$65                 ; 'e'
        OUT
        LDA #$6C                 ; 'l'
        OUT
        LDA #$6C                 ; 'l'
        OUT
        LDA #$6F                 ; 'o'
        OUT
        LDA #$2C                 ; ','
        OUT
        LDA #$20                 ; ' '
        OUT
        LDA #$73                 ; 's'
        OUT
        LDA #$69                 ; 'i'
        OUT
        LDA #$6C                 ; 'l'
        OUT
        LDA #$69                 ; 'i'
        OUT
        LDA #$63                 ; 'c'
        OUT
        LDA #$6F                 ; 'o'
        OUT
        LDA #$6E                 ; 'n'
        OUT
        LDA #$21                 ; '!'
        OUT

        LDX #$00
loop:
        LDA #'*'
        STA $F800,X              ; one star per column/row (char plane)
        LDA #2                   ; palette index 2 = green (color plane)
        STA $FC00,X
        TXA
        INC A
        TAX
        CMP #$20                 ; stop when X == 32
        JNZ loop

        HLT