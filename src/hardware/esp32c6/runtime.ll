; Espressif ESP32-C6 firmware runtime for its high-performance RISC-V core
; (RV32IMAC), compiled by the Dodo compiler for every ESP32-C6 executable
; (`chip = "esp32c6"` or a board using it) and linked before the program
; object.
;
; The firmware uses the boot ROM's direct boot: when the first two words of
; flash are 0xaedb041d, the ROM maps the whole flash chip through the cache
; at 0x42000000, offset for offset, and jumps to 0x42000008. So there is no
; second-stage bootloader, partition table, or image header: the flash image
; is the linked program as it lies in memory.
;
; It provides, in order of boot:
;   .boot_header   The direct boot marker, twice.
;   Reset_Handler  Masks interrupts, sets the stack pointer, turns off the
;                  watchdogs the boot ROM leaves running (the LP and both
;                  timer group watchdogs, and the super watchdog's need to be
;                  fed) and the access permission filters that keep DMA out
;                  of SRAM, copies .data from flash, zeroes .bss, switches the
;                  flash to dual I/O reads at 80 MHz, installs the trap
;                  vector, routes CPU interrupt 1 through the interrupt
;                  controller, unmasks interrupts, as a Cortex-M starts, and
;                  calls `main`. If `main` returns, the core sleeps.
;   .trap_vector   The vectored trap table mtvec points at: exceptions enter
;                  dodo_trap_exception, every CPU interrupt line
;                  dodo_trap_interrupt.
;   dodo_trap_interrupt
;                  Saves the registers a C function may change, then calls
;                  the handler of every pending interrupt source that is
;                  routed to a CPU interrupt (std/embedded/chip/esp32c6
;                  routes the sources it enables to CPU interrupt 1), from
;                  dodo_interrupt_table, and returns with `mret`. Handlers
;                  are plain C functions: `@interrupt(UART0)` in Dodo, or
;                  `extern "C" fn UART0()`, replaces the weak default, which
;                  halts. A handler must clear its source, as the sources are
;                  level-triggered.
;   USB_DEVICE     Calls dodo_usb_interrupt if the program defines it.
;   DMA_IN_CH0 to DMA_OUT_CH2
;                  Call dodo_dma_interrupt if the program defines it:
;                  std/embedded/chip/esp32c6_dma does, to raise the signals
;                  of finished DMA channels.
;   dodo_trap_exception
;                  Calls Exception_Handler(mcause, mepc, mtval); its weak
;                  default reports the exception through the panic hook as
;                  `processor exception check failed at mcause:mepc:CAUSE:PC`.
;   dodo_flash_erase, dodo_flash_program
;                  Erase or program flash above the program from SRAM, for
;                  std/embedded/chip/esp_flash: see below.
;   dodo_flash_image_end, dodo_flash_end
;                  Where the program image ends in flash and where the
;                  board's flash ends, as mapped at 0x42000000.
;   dodo_board_panic
;                  The panic hook: masks interrupts, passes its arguments to
;                  dodo_panic_report if the program defines it, and halts.
;                  std/embedded/chip/esp32c6 defines that reporter, which
;                  writes the failure to the serial ports the program claimed
;                  and to the USB serial port. Weak, so a program can define
;                  its own hook.
;
; The boot ROM's functions the chip package calls, such as software_reset,
; are named in the linker script at their entries in the ROM's jump table.
; The memory, integer, and floating-point helpers LLVM calls follow from
; src/hardware/arm_eabi.ll, arm_float.ll, and arm_float_soft.ll, which the
; compiler appends to this file: they are plain integer code, which the
; ESP32-C6's soft-float RISC-V calling convention passes like Arm's. The
; core multiplies and divides 32-bit integers in hardware, so only the
; 64-bit division helpers are linked.

module asm ".section .boot_header, \22a\22, @progbits"
module asm ".global __boot_header"
module asm "__boot_header:"
module asm "    .word 0xaedb041d"
module asm "    .word 0xaedb041d"

module asm ".section .text.Reset_Handler, \22ax\22, @progbits"
module asm ".global Reset_Handler"
module asm ".type Reset_Handler, @function"
module asm "Reset_Handler:"
module asm "    csrci mstatus, 8"                         ; mask interrupts
module asm "    csrw mie, zero"
module asm "    lui sp, %hi(__stack_top)"
module asm "    addi sp, sp, %lo(__stack_top)"
module asm "    li t0, 0x50d83aa1"                        ; watchdog write key
module asm "    li t1, 0x600b1c00"                        ; LP_WDT
module asm "    sw t0, 0x18(t1)"                          ; unlock
module asm "    sw zero, 0x00(t1)"                        ; CONFIG0: off
module asm "    sw zero, 0x18(t1)"                        ; lock
module asm "    li t2, 0x50d83aa1"                        ; super watchdog key
module asm "    sw t2, 0x20(t1)"
module asm "    lw t3, 0x1c(t1)"                          ; SWD_CONF:
module asm "    li t4, 0x40000"                           ; feed itself
module asm "    or t3, t3, t4"
module asm "    sw t3, 0x1c(t1)"
module asm "    sw zero, 0x20(t1)"
module asm "    li t1, 0x60008000"                        ; TIMG0, then TIMG1
module asm "    li t5, 0x60009000"
module asm "    li t3, 0x400000"                          ; WDT_CONF_UPDATE_EN
module asm "1:  sw t0, 0x64(t1)"                          ; WDTWPROTECT: unlock
module asm "    sw zero, 0x48(t1)"                        ; WDTCONFIG0: off
module asm "    sw t3, 0x48(t1)"                          ; latch it
module asm "    sw zero, 0x64(t1)"
module asm "    beq t1, t5, 2f"
module asm "    mv t1, t5"
module asm "    j 1b"
module asm "2:  li t1, 0x60099000"                        ; HP_APM, LP_APM0, LP_APM:
module asm "    sw zero, 0xc4(t1)"                        ; turn off the access path
module asm "    li t1, 0x60099800"                        ; filters, which admit only
module asm "    sw zero, 0xc4(t1)"                        ; masters in TEE mode, so
module asm "    li t1, 0x600b3800"                        ; that DMA, which starts in
module asm "    sw zero, 0xc4(t1)"                        ; REE mode, reaches SRAM
module asm "    lui a0, %hi(__data_load)"
module asm "    addi a0, a0, %lo(__data_load)"
module asm "    lui a1, %hi(__data_start)"
module asm "    addi a1, a1, %lo(__data_start)"
module asm "    lui a2, %hi(__data_end)"
module asm "    addi a2, a2, %lo(__data_end)"
module asm "    j 4f"
module asm "3:  lw a3, 0(a0)"
module asm "    sw a3, 0(a1)"
module asm "    addi a0, a0, 4"
module asm "    addi a1, a1, 4"
module asm "4:  bltu a1, a2, 3b"
module asm "    lui a1, %hi(__bss_start)"
module asm "    addi a1, a1, %lo(__bss_start)"
module asm "    lui a2, %hi(__bss_end)"
module asm "    addi a2, a2, %lo(__bss_end)"
module asm "    j 6f"
module asm "5:  sw zero, 0(a1)"
module asm "    addi a1, a1, 4"
module asm "6:  bltu a1, a2, 5b"
module asm "    fence"
module asm "    fence.i"                                  ; .ramfunc code
module asm "    call dodo_esp32c6_flash_setup"
module asm "    lui a0, %hi(__vector_table)"
module asm "    addi a0, a0, %lo(__vector_table)"
module asm "    ori a0, a0, 1"                            ; vectored mode
module asm "    csrw mtvec, a0"
module asm "    li a0, 0x20001000"                        ; PLIC_MX
module asm "    li a1, 2"
module asm "    sw a1, 0x00(a0)"                          ; enable CPU interrupt 1 only
module asm "    sw zero, 0x04(a0)"                        ; all level-triggered
module asm "    li a1, 1"
module asm "    sw a1, 0x14(a0)"                          ; interrupt 1: priority 1
module asm "    sw a1, 0x90(a0)"                          ; threshold: priority 1 and up
module asm "    li a1, -1"
module asm "    csrw mie, a1"
module asm "    csrsi mstatus, 8"                         ; unmask: nothing is routed yet
module asm "    call main"
module asm "7:  wfi"                                      ; main returned: sleep
module asm "    j 7b"

; Read flash faster than the boot ROM leaves it, in 1-bit mode with the 03h
; command at 30 MHz: dual I/O (BBh) at 80 MHz, which every flash chip on
; ESP32-C6 modules supports without changing its status register. The
; flash controller may not change while the cache fetches from flash, so
; this runs from SRAM with the cache suspended, calling the boot ROM's own
; configuration functions, as its image header handling does.
module asm ".section .ramfunc.dodo_esp32c6_flash_setup, \22ax\22, @progbits"
module asm ".global dodo_esp32c6_flash_setup"
module asm ".type dodo_esp32c6_flash_setup, @function"
module asm "dodo_esp32c6_flash_setup:"
module asm "    addi sp, sp, -16"
module asm "    sw ra, 12(sp)"
module asm "    sw s0, 8(sp)"
module asm "    call Cache_Suspend_ICache"
module asm "    mv s0, a0"                                ; the autoload state to restore
module asm "    li t0, 0x6009601c"                        ; PCR MSPI_CLK_CONF: the
module asm "    lw t1, 0(t0)"                             ; controller's clock is
module asm "    li t2, -65281"                            ; 480 MHz / 6 = 80 MHz
module asm "    and t1, t1, t2"
module asm "    li t2, 0x500"
module asm "    or t1, t1, t2"
module asm "    sw t1, 0(t0)"
module asm "    li a0, 1"                                 ; SPI0 and SPI1 at 80 MHz
module asm "    li a1, 0"
module asm "    call esp_rom_spiflash_config_clk"
module asm "    li a0, 1"
module asm "    li a1, 1"
module asm "    call esp_rom_spiflash_config_clk"
module asm "    li a0, 0"
module asm "    li a1, 1"
module asm "    call spi_dummy_len_fix"
module asm "    li a0, 1"
module asm "    li a1, 1"
module asm "    call spi_dummy_len_fix"
module asm "    li a0, 2"                                 ; ESP_ROM_SPIFLASH_DIO_MODE,
module asm "    li a1, 0"                                 ; status register untouched
module asm "    call esp_rom_spiflash_config_readmode"
module asm "    li a0, 2"
module asm "    call spi_common_set_dummy_output"
module asm "    mv a0, s0"
module asm "    call Cache_Resume_ICache"
module asm "    lw ra, 12(sp)"
module asm "    lw s0, 8(sp)"
module asm "    addi sp, sp, 16"
module asm "    ret"

; dodo_flash_erase(offset, count) erases `count` bytes of flash from
; `offset` (multiples of 4 KiB) sector by sector, and
; dodo_flash_program(offset, data, count) programs `count` bytes from
; word-aligned `data` in SRAM (a multiple of 4, within one 256-byte page);
; both return 0, or 1 if the boot ROM reports an error. The ROM's
; esp_rom_spiflash_erase_sector and esp_rom_spiflash_write drive the flash
; through SPI1, during which the cache may not fetch from it over SPI0, so
; this code runs from SRAM with interrupts masked and the cache suspended,
; as ESP-IDF's spi_flash_disable_interrupts_caches_and_other_cpu does. The
; ROM's flash chip record (rom_spiflash_legacy_data) says 2 MiB after
; direct boot, and the ROM refuses offsets beyond it, so the board's size
; goes there first. Afterwards the cache resumes and the changed range is
; invalidated, so reads through the mapping at 0x42000000 see the new
; contents. The reset handler left SPI1, like SPI0, reading in dual I/O
; mode (SPI_MEM_CTRL FREAD_DIO), in which the ROM's status register reads
; return garbage, so its erase would not wait for the flash; SPI1's read
; mode bits are cleared meanwhile, as the ROM's own page program does, and
; restored afterwards. SPI0, which the cache reads through, keeps its mode.
; A flash whose status register has block protection bits set (BP0 to BP4)
; ignores erases and programs without an error, as the DevKitC-1's arrived:
; then esp_rom_spiflash_unlock clears them, keeping the QE bit, as esp-hal's
; esp-storage does. That writes the non-volatile status register once; the
; bits stay clear afterwards.
module asm ".section .ramfunc.dodo_flash, \22ax\22, @progbits"
module asm ".global dodo_flash_erase"
module asm ".type dodo_flash_erase, @function"
module asm "dodo_flash_erase:"
module asm "    mv a3, a1"                                ; count
module asm "    li a2, 0"                                 ; no data: erase
module asm "    j 1f"
module asm ".global dodo_flash_program"
module asm ".type dodo_flash_program, @function"
module asm "dodo_flash_program:"
module asm "    mv a3, a2"                                ; count
module asm "    mv a2, a1"                                ; data
module asm "1:  addi sp, sp, -48"
module asm "    sw ra, 44(sp)"
module asm "    sw s0, 40(sp)"
module asm "    sw s1, 36(sp)"
module asm "    sw s2, 32(sp)"
module asm "    sw s3, 28(sp)"
module asm "    sw s4, 24(sp)"
module asm "    sw s5, 20(sp)"
module asm "    sw s6, 16(sp)"
module asm "    mv s1, a0"                                ; offset
module asm "    mv s2, a2"                                ; data, 0 to erase
module asm "    mv s3, a3"                                ; count
module asm "    li s5, 0"                                 ; the result
module asm "    csrrci s0, mstatus, 8"                    ; mask interrupts
module asm "    call Cache_Suspend_ICache"
module asm "    mv s4, a0"                                ; the autoload state to restore
module asm "    lui t0, %hi(rom_spiflash_legacy_data)"
module asm "    lw t0, %lo(rom_spiflash_legacy_data)(t0)"
module asm "    lui t1, %hi(__flash_end)"
module asm "    addi t1, t1, %lo(__flash_end)"
module asm "    lui t2, 0x42000"
module asm "    sub t1, t1, t2"
module asm "    sw t1, 4(t0)"                             ; chip_size: the board's
module asm "    lui t0, 0x60003"                          ; SPI1 CTRL: single-line
module asm "    lw t1, 8(t0)"                             ; reads (FREAD_DUAL, QUAD,
module asm "    sw t1, 12(sp)"                            ; DIO, and QIO off)
module asm "    lui t2, 0xfe6fc"
module asm "    addi t2, t2, -1"
module asm "    and t1, t1, t2"
module asm "    sw t1, 8(t0)"
module asm "    lui t0, %hi(rom_spiflash_legacy_data)"
module asm "    lw a0, %lo(rom_spiflash_legacy_data)(t0)"
module asm "    addi a1, sp, 8"
module asm "    call esp_rom_spiflash_read_status"
module asm "    bnez a0, 5f"
module asm "    lw t1, 8(sp)"
module asm "    andi t1, t1, 0x7c"                        ; BP0 to BP4
module asm "    beqz t1, 7f"
module asm "    call esp_rom_spiflash_unlock"
module asm "    bnez a0, 5f"
module asm "7:  bnez s2, 3f"
module asm "    mv s6, s1"                                ; erase: sector by sector
module asm "    add t0, s1, s3"
module asm "2:  bgeu s6, t0, 4f"
module asm "    srli a0, s6, 12"
module asm "    call esp_rom_spiflash_erase_sector"
module asm "    bnez a0, 5f"
module asm "    lui t0, 1"
module asm "    add s6, s6, t0"
module asm "    add t0, s1, s3"
module asm "    j 2b"
module asm "3:  mv a0, s1"                                ; program
module asm "    mv a1, s2"
module asm "    mv a2, s3"
module asm "    call esp_rom_spiflash_write"
module asm "    beqz a0, 4f"
module asm "5:  li s5, 1"
module asm "4:  lw t1, 12(sp)"                            ; SPI1's read mode back
module asm "    lui t0, 0x60003"
module asm "    sw t1, 8(t0)"
module asm "    mv a0, s4"
module asm "    call Cache_Resume_ICache"
module asm "    lui a0, 0x42000"
module asm "    add a0, a0, s1"
module asm "    mv a1, s3"
module asm "    call Cache_Invalidate_Addr"
module asm "    andi s0, s0, 8"
module asm "    beqz s0, 6f"
module asm "    csrsi mstatus, 8"                         ; unmask, if they were
module asm "6:  mv a0, s5"
module asm "    lw ra, 44(sp)"
module asm "    lw s0, 40(sp)"
module asm "    lw s1, 36(sp)"
module asm "    lw s2, 32(sp)"
module asm "    lw s3, 28(sp)"
module asm "    lw s4, 24(sp)"
module asm "    lw s5, 20(sp)"
module asm "    lw s6, 16(sp)"
module asm "    addi sp, sp, 48"
module asm "    ret"

; dodo_flash_image_end() and dodo_flash_end(): the linker script's addresses.
module asm ".section .text.dodo_flash_image_end, \22ax\22, @progbits"
module asm ".global dodo_flash_image_end"
module asm ".type dodo_flash_image_end, @function"
module asm "dodo_flash_image_end:"
module asm "    lui a0, %hi(__flash_image_end)"
module asm "    addi a0, a0, %lo(__flash_image_end)"
module asm "    ret"
module asm ".section .text.dodo_flash_end, \22ax\22, @progbits"
module asm ".global dodo_flash_end"
module asm ".type dodo_flash_end, @function"
module asm "dodo_flash_end:"
module asm "    lui a0, %hi(__flash_end)"
module asm "    addi a0, a0, %lo(__flash_end)"
module asm "    ret"

; mtvec in vectored mode: exceptions enter at the base, CPU interrupt n at
; base + 4n. The jumps must stay 4 bytes long, so this table is never
; compressed.
module asm ".section .trap_vector, \22ax\22, @progbits"
module asm ".balign 256"
module asm ".global __vector_table"
module asm "__vector_table:"
module asm ".option push"
module asm ".option norvc"
module asm "    j dodo_trap_exception"
module asm "    .rept 31"
module asm "    j dodo_trap_interrupt"
module asm "    .endr"
module asm ".option pop"

; The caller-saved registers, and s0 to s2, which the dispatch loop keeps
; across handler calls: 19 words in an 80-byte frame, keeping the stack
; 16-byte aligned. Interrupts stay masked while handlers run.
module asm ".global dodo_trap_interrupt"
module asm ".type dodo_trap_interrupt, @function"
module asm "dodo_trap_interrupt:"
module asm "    addi sp, sp, -80"
module asm "    sw ra, 0(sp)"
module asm "    sw t0, 4(sp)"
module asm "    sw t1, 8(sp)"
module asm "    sw t2, 12(sp)"
module asm "    sw a0, 16(sp)"
module asm "    sw a1, 20(sp)"
module asm "    sw a2, 24(sp)"
module asm "    sw a3, 28(sp)"
module asm "    sw a4, 32(sp)"
module asm "    sw a5, 36(sp)"
module asm "    sw a6, 40(sp)"
module asm "    sw a7, 44(sp)"
module asm "    sw t3, 48(sp)"
module asm "    sw t4, 52(sp)"
module asm "    sw t5, 56(sp)"
module asm "    sw t6, 60(sp)"
module asm "    sw s0, 64(sp)"
module asm "    sw s1, 68(sp)"
module asm "    sw s2, 72(sp)"
module asm "    li s1, 0x60010134"                        ; INTMTX status, sources 0 to 31
module asm "    li s0, 0"                                 ; first source of the word
module asm "1:  lw s2, 0(s1)"
module asm "2:  beqz s2, 5f"
module asm "    neg t0, s2"
module asm "    and t0, t0, s2"                           ; lowest pending source
module asm "    xor s2, s2, t0"
module asm "    mv t1, s0"
module asm "3:  andi t2, t0, 1"
module asm "    bnez t2, 4f"
module asm "    srli t0, t0, 1"
module asm "    addi t1, t1, 1"
module asm "    j 3b"
module asm "4:  slli t1, t1, 2"
module asm "    li t2, 0x60010000"                        ; INTMTX map: the source's
module asm "    add t2, t2, t1"                           ; CPU interrupt, 0 when
module asm "    lw t2, 0(t2)"                             ; not routed
module asm "    beqz t2, 2b"
module asm "    lui t2, %hi(dodo_interrupt_table)"
module asm "    addi t2, t2, %lo(dodo_interrupt_table)"
module asm "    add t2, t2, t1"
module asm "    lw t2, 0(t2)"
module asm "    jalr t2"
module asm "    j 2b"
module asm "5:  addi s1, s1, 4"
module asm "    addi s0, s0, 32"
module asm "    li t0, 96"
module asm "    bltu s0, t0, 1b"
module asm "    lw ra, 0(sp)"
module asm "    lw t0, 4(sp)"
module asm "    lw t1, 8(sp)"
module asm "    lw t2, 12(sp)"
module asm "    lw a0, 16(sp)"
module asm "    lw a1, 20(sp)"
module asm "    lw a2, 24(sp)"
module asm "    lw a3, 28(sp)"
module asm "    lw a4, 32(sp)"
module asm "    lw a5, 36(sp)"
module asm "    lw a6, 40(sp)"
module asm "    lw a7, 44(sp)"
module asm "    lw t3, 48(sp)"
module asm "    lw t4, 52(sp)"
module asm "    lw t5, 56(sp)"
module asm "    lw t6, 60(sp)"
module asm "    lw s0, 64(sp)"
module asm "    lw s1, 68(sp)"
module asm "    lw s2, 72(sp)"
module asm "    addi sp, sp, 80"
module asm "    mret"

; An exception: Exception_Handler(mcause, mepc, mtval). If it returns, the
; faulting instruction runs again.
module asm ".global dodo_trap_exception"
module asm ".type dodo_trap_exception, @function"
module asm "dodo_trap_exception:"
module asm "    addi sp, sp, -64"
module asm "    sw ra, 0(sp)"
module asm "    sw t0, 4(sp)"
module asm "    sw t1, 8(sp)"
module asm "    sw t2, 12(sp)"
module asm "    sw a0, 16(sp)"
module asm "    sw a1, 20(sp)"
module asm "    sw a2, 24(sp)"
module asm "    sw a3, 28(sp)"
module asm "    sw a4, 32(sp)"
module asm "    sw a5, 36(sp)"
module asm "    sw a6, 40(sp)"
module asm "    sw a7, 44(sp)"
module asm "    sw t3, 48(sp)"
module asm "    sw t4, 52(sp)"
module asm "    sw t5, 56(sp)"
module asm "    sw t6, 60(sp)"
module asm "    csrr a0, mcause"
module asm "    csrr a1, mepc"
module asm "    csrr a2, mtval"
module asm "    call Exception_Handler"
module asm "    lw ra, 0(sp)"
module asm "    lw t0, 4(sp)"
module asm "    lw t1, 8(sp)"
module asm "    lw t2, 12(sp)"
module asm "    lw a0, 16(sp)"
module asm "    lw a1, 20(sp)"
module asm "    lw a2, 24(sp)"
module asm "    lw a3, 28(sp)"
module asm "    lw a4, 32(sp)"
module asm "    lw a5, 36(sp)"
module asm "    lw a6, 40(sp)"
module asm "    lw a7, 44(sp)"
module asm "    lw t3, 48(sp)"
module asm "    lw t4, 52(sp)"
module asm "    lw t5, 56(sp)"
module asm "    lw t6, 60(sp)"
module asm "    addi sp, sp, 64"
module asm "    mret"

; The default Exception_Handler reports the exception through the panic
; hook, which does not return.
module asm ".section .text.dodo_esp32c6_exception, \22ax\22, @progbits"
module asm ".type dodo_esp32c6_exception, @function"
module asm "dodo_esp32c6_exception:"
module asm "    mv a3, a1"                                ; column: the faulting PC
module asm "    mv a2, a0"                                ; line: mcause
module asm "    lui a0, %hi(dodo_esp32c6_exception_check)"
module asm "    addi a0, a0, %lo(dodo_esp32c6_exception_check)"
module asm "    lui a1, %hi(dodo_esp32c6_exception_file)"
module asm "    addi a1, a1, %lo(dodo_esp32c6_exception_file)"
module asm "    tail dodo_board_panic"
module asm ".section .rodata.dodo_esp32c6_exception_text, \22a\22, @progbits"
module asm "dodo_esp32c6_exception_check:"
module asm "    .asciz \22processor exception\22"
module asm "dodo_esp32c6_exception_file:"
module asm "    .asciz \22mcause:mepc\22"

module asm ".section .text.DefaultHandler, \22ax\22, @progbits"
module asm ".global DefaultHandler"
module asm ".type DefaultHandler, @function"
module asm "DefaultHandler:"
module asm "    j DefaultHandler"

; USB_DEVICE, weak like the other handlers, tail-calls the weak
; dodo_usb_interrupt if the program defines it, and is DefaultHandler
; otherwise. The weak references are absolute, so an undefined one is 0.
module asm ".section .text.UsbHandler, \22ax\22, @progbits"
module asm ".weak dodo_usb_interrupt"
module asm ".type UsbHandler, @function"
module asm "UsbHandler:"
module asm "    lui t0, %hi(dodo_usb_interrupt)"
module asm "    addi t0, t0, %lo(dodo_usb_interrupt)"
module asm "    bnez t0, 1f"
module asm "    tail DefaultHandler"
module asm "1:  jr t0"

; The six DMA channel interrupts do the same with dodo_dma_interrupt.
module asm ".section .text.DmaHandler, \22ax\22, @progbits"
module asm ".weak dodo_dma_interrupt"
module asm ".type DmaHandler, @function"
module asm "DmaHandler:"
module asm "    lui t0, %hi(dodo_dma_interrupt)"
module asm "    addi t0, t0, %lo(dodo_dma_interrupt)"
module asm "    bnez t0, 1f"
module asm "    tail DefaultHandler"
module asm "1:  jr t0"

; A weak reference: without a definition the linker makes it 0, and a
; definition is kept by --gc-sections only while this hook is.
module asm ".section .text.dodo_board_panic, \22ax\22, @progbits"
module asm ".weak dodo_board_panic"
module asm ".weak dodo_panic_report"
module asm ".type dodo_board_panic, @function"
module asm "dodo_board_panic:"
module asm "    csrci mstatus, 8"
module asm "    lui t0, %hi(dodo_panic_report)"
module asm "    addi t0, t0, %lo(dodo_panic_report)"
module asm "    beqz t0, 1f"
module asm "    jalr t0"                                  ; a0 to a3 are the hook's arguments
module asm "1:  wfi"
module asm "    j 1b"

; The handler of each interrupt source, by source number.
module asm ".section .rodata.dodo_interrupt_table, \22a\22, @progbits"
module asm ".balign 4"
module asm ".global dodo_interrupt_table"
module asm "dodo_interrupt_table:"
module asm "    .word WIFI_MAC, WIFI_MAC_NMI, WIFI_PWR, WIFI_BB"
module asm "    .word BT_MAC, BT_BB, BT_BB_NMI, LP_TIMER"
module asm "    .word COEX, BLE_TIMER, BLE_SEC, I2C_MASTER"
module asm "    .word ZB_MAC, PMU, EFUSE, LP_RTC_TIMER"
module asm "    .word LP_UART, LP_I2C, LP_WDT, LP_PERI_TIMEOUT"
module asm "    .word LP_APM_M0, LP_APM_M1, FROM_CPU_INTR0, FROM_CPU_INTR1"
module asm "    .word FROM_CPU_INTR2, FROM_CPU_INTR3, ASSIST_DEBUG, TRACE"
module asm "    .word CACHE, CPU_PERI_TIMEOUT, GPIO, GPIO_NMI"
module asm "    .word PAU, HP_PERI_TIMEOUT, MODEM_PERI_TIMEOUT, HP_APM_M0"
module asm "    .word HP_APM_M1, HP_APM_M2, HP_APM_M3, LP_APM0"
module asm "    .word MSPI, I2S0, UHCI0, UART0"
module asm "    .word UART1, LEDC, TWAI0, TWAI1"
module asm "    .word USB_DEVICE, RMT, I2C_EXT0, TG0_T0_LEVEL"
module asm "    .word TG0_T1_LEVEL, TG0_WDT_LEVEL, TG1_T0_LEVEL, TG1_T1_LEVEL"
module asm "    .word TG1_WDT_LEVEL, SYSTIMER_TARGET0, SYSTIMER_TARGET1, SYSTIMER_TARGET2"
module asm "    .word APB_SARADC, MCPWM0, PCNT, PARL_IO"
module asm "    .word SLC0, SLC1, DMA_IN_CH0, DMA_IN_CH1"
module asm "    .word DMA_IN_CH2, DMA_OUT_CH0, DMA_OUT_CH1, DMA_OUT_CH2"
module asm "    .word SPI2, AES, SHA, RSA"
module asm "    .word ECC"

module asm ".weak Exception_Handler"
module asm ".set Exception_Handler, dodo_esp32c6_exception"
module asm ".weak WIFI_MAC, WIFI_MAC_NMI, WIFI_PWR, WIFI_BB, BT_MAC, BT_BB"
module asm ".weak BT_BB_NMI, LP_TIMER, COEX, BLE_TIMER, BLE_SEC, I2C_MASTER"
module asm ".weak ZB_MAC, PMU, EFUSE, LP_RTC_TIMER, LP_UART, LP_I2C"
module asm ".weak LP_WDT, LP_PERI_TIMEOUT, LP_APM_M0, LP_APM_M1, FROM_CPU_INTR0, FROM_CPU_INTR1"
module asm ".weak FROM_CPU_INTR2, FROM_CPU_INTR3, ASSIST_DEBUG, TRACE, CACHE, CPU_PERI_TIMEOUT"
module asm ".weak GPIO, GPIO_NMI, PAU, HP_PERI_TIMEOUT, MODEM_PERI_TIMEOUT, HP_APM_M0"
module asm ".weak HP_APM_M1, HP_APM_M2, HP_APM_M3, LP_APM0, MSPI, I2S0"
module asm ".weak UHCI0, UART0, UART1, LEDC, TWAI0, TWAI1"
module asm ".weak USB_DEVICE, RMT, I2C_EXT0, TG0_T0_LEVEL, TG0_T1_LEVEL, TG0_WDT_LEVEL"
module asm ".weak TG1_T0_LEVEL, TG1_T1_LEVEL, TG1_WDT_LEVEL, SYSTIMER_TARGET0, SYSTIMER_TARGET1, SYSTIMER_TARGET2"
module asm ".weak APB_SARADC, MCPWM0, PCNT, PARL_IO, SLC0, SLC1"
module asm ".weak DMA_IN_CH0, DMA_IN_CH1, DMA_IN_CH2, DMA_OUT_CH0, DMA_OUT_CH1, DMA_OUT_CH2"
module asm ".weak SPI2, AES, SHA, RSA, ECC"
module asm ".set WIFI_MAC, DefaultHandler"
module asm ".set WIFI_MAC_NMI, DefaultHandler"
module asm ".set WIFI_PWR, DefaultHandler"
module asm ".set WIFI_BB, DefaultHandler"
module asm ".set BT_MAC, DefaultHandler"
module asm ".set BT_BB, DefaultHandler"
module asm ".set BT_BB_NMI, DefaultHandler"
module asm ".set LP_TIMER, DefaultHandler"
module asm ".set COEX, DefaultHandler"
module asm ".set BLE_TIMER, DefaultHandler"
module asm ".set BLE_SEC, DefaultHandler"
module asm ".set I2C_MASTER, DefaultHandler"
module asm ".set ZB_MAC, DefaultHandler"
module asm ".set PMU, DefaultHandler"
module asm ".set EFUSE, DefaultHandler"
module asm ".set LP_RTC_TIMER, DefaultHandler"
module asm ".set LP_UART, DefaultHandler"
module asm ".set LP_I2C, DefaultHandler"
module asm ".set LP_WDT, DefaultHandler"
module asm ".set LP_PERI_TIMEOUT, DefaultHandler"
module asm ".set LP_APM_M0, DefaultHandler"
module asm ".set LP_APM_M1, DefaultHandler"
module asm ".set FROM_CPU_INTR0, DefaultHandler"
module asm ".set FROM_CPU_INTR1, DefaultHandler"
module asm ".set FROM_CPU_INTR2, DefaultHandler"
module asm ".set FROM_CPU_INTR3, DefaultHandler"
module asm ".set ASSIST_DEBUG, DefaultHandler"
module asm ".set TRACE, DefaultHandler"
module asm ".set CACHE, DefaultHandler"
module asm ".set CPU_PERI_TIMEOUT, DefaultHandler"
module asm ".set GPIO, DefaultHandler"
module asm ".set GPIO_NMI, DefaultHandler"
module asm ".set PAU, DefaultHandler"
module asm ".set HP_PERI_TIMEOUT, DefaultHandler"
module asm ".set MODEM_PERI_TIMEOUT, DefaultHandler"
module asm ".set HP_APM_M0, DefaultHandler"
module asm ".set HP_APM_M1, DefaultHandler"
module asm ".set HP_APM_M2, DefaultHandler"
module asm ".set HP_APM_M3, DefaultHandler"
module asm ".set LP_APM0, DefaultHandler"
module asm ".set MSPI, DefaultHandler"
module asm ".set I2S0, DefaultHandler"
module asm ".set UHCI0, DefaultHandler"
module asm ".set UART0, DefaultHandler"
module asm ".set UART1, DefaultHandler"
module asm ".set LEDC, DefaultHandler"
module asm ".set TWAI0, DefaultHandler"
module asm ".set TWAI1, DefaultHandler"
module asm ".set USB_DEVICE, UsbHandler"
module asm ".set RMT, DefaultHandler"
module asm ".set I2C_EXT0, DefaultHandler"
module asm ".set TG0_T0_LEVEL, DefaultHandler"
module asm ".set TG0_T1_LEVEL, DefaultHandler"
module asm ".set TG0_WDT_LEVEL, DefaultHandler"
module asm ".set TG1_T0_LEVEL, DefaultHandler"
module asm ".set TG1_T1_LEVEL, DefaultHandler"
module asm ".set TG1_WDT_LEVEL, DefaultHandler"
module asm ".set SYSTIMER_TARGET0, DefaultHandler"
module asm ".set SYSTIMER_TARGET1, DefaultHandler"
module asm ".set SYSTIMER_TARGET2, DefaultHandler"
module asm ".set APB_SARADC, DefaultHandler"
module asm ".set MCPWM0, DefaultHandler"
module asm ".set PCNT, DefaultHandler"
module asm ".set PARL_IO, DefaultHandler"
module asm ".set SLC0, DefaultHandler"
module asm ".set SLC1, DefaultHandler"
module asm ".set DMA_IN_CH0, DmaHandler"
module asm ".set DMA_IN_CH1, DmaHandler"
module asm ".set DMA_IN_CH2, DmaHandler"
module asm ".set DMA_OUT_CH0, DmaHandler"
module asm ".set DMA_OUT_CH1, DmaHandler"
module asm ".set DMA_OUT_CH2, DmaHandler"
module asm ".set SPI2, DefaultHandler"
module asm ".set AES, DefaultHandler"
module asm ".set SHA, DefaultHandler"
module asm ".set RSA, DefaultHandler"
module asm ".set ECC, DefaultHandler"
module asm ".text"
