#[allow(
    clippy::needless_late_init,
    clippy::let_and_return,
    non_snake_case,
    unreachable_code,
    unused_assignments
)]
#[machine_check::machine_description]
pub mod machine_module {
    use ::machine_check::{bitmask_switch, Bitvector, BitvectorArray, Ext, Signed, Unsigned};
    use ::std::{
        clone::Clone,
        cmp::{Eq, PartialEq},
        convert::Into,
        fmt::Debug,
        hash::Hash,
        panic, unimplemented,
    };

    /// System input.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct Input {}

    /// System state.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct State {
        /// Program Counter.
        PC: Bitvector<64>,

        /// 32 general-purpose registers.
        ///
        /// Register 0 is always zero.
        reg: BitvectorArray<5, 64>,

        // should i add here the SRAM with the machine code parsed from the .elf?
        /// Onboard SRAM (Parity), 0x2000_4000..0x2000_7000
        ///
        /// this means there are 12288 bytes
        /// use a 14-bit array (16384) with further bounds checking
        sram_dynamic: BitvectorArray<14, 8>,
    }

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct System {
        /// Program flash, 0x0000_0000..0x0002_0000
        ///
        /// here stored as little-endian halfwords
        pub sram_static: BitvectorArray<14, 8>,
        pub sram_dynamic: BitvectorArray<14, 8>,
        pub pc_entry: Bitvector<64>,
    }

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct Param {
        pub reg: BitvectorArray<5, 64>,
    }

    impl ::machine_check::Machine for System {
        type Input = Input;
        type State = State;
        type Param = Param;

        fn init(&self, _input: &Input, param: &Param) -> State {
            // the value of general-purpose registers after reset is undefined (except R0)
            let mut reg = Clone::clone(&param.reg);
            reg[Bitvector::<5>::new(0)] = Bitvector::<64>::new(0);

            // SRAM data is undefined after power-on
            let sram_dynamic = Clone::clone(&self.sram_dynamic);
            let PC = Clone::clone(&self.pc_entry);

            State {
                // Program Counter starts at zero
                PC,
                reg,
                sram_dynamic,
            }
        }

        fn next(&self, state: &State, input: &Input, param: &Param) -> State {
            // fetch first instruction halfword
            // gets from flash so new methode here as well?
            let first_half: Unsigned<16> = Self::pc_halfword(self, state.PC);
            let opcode_low = Ext::<2>::ext(first_half);
            let first_half_1 = first_half >> Unsigned::<16>::new(2);
            let first_half_2 = Ext::<14>::ext(first_half_1);

            // move PC past the first halfword
            let PC = state.PC + Bitvector::<64>::new(2);

            let mut next_state = State {
                PC,
                reg: Clone::clone(&state.reg),
                sram_dynamic: Clone::clone(&state.sram_dynamic),
            };

            // perform instruction according to quadrant
            bitmask_switch!(opcode_low {
                "00" => {
                    // compressed quadrant 0
                    next_state = Self::instruction_00(self, &next_state, input, param, first_half_2);
                }
                "01" =>  {
                    // compressed quadrant 1
                    next_state = Self::instruction_01(self, &next_state, input, param, first_half_2);
                }
                "10" => {
                    // compressed quadrant 2
                    next_state = Self::instruction_10(self, &next_state, input, param, first_half_2);
                }
                "11" => {
                    // non-compressed 32-bit instruction
                    next_state = Self::instruction_full(self, &next_state, input, param, first_half_2);
                },
            });

            // sanity-check that reg[0] is still zero
            if next_state.reg[Bitvector::<5>::new(0)] != Bitvector::<64>::new(0) {
                panic!("Register 0 has been set to non-zero");
            }

            next_state
        }
    }

    impl System {
        fn instruction_full(
            &self,
            state: &State,
            input: &Input,
            _param: &Param,
            first_half_noncomp: Unsigned<14>,
        ) -> State {
            // 32-bit instruction
            // fetch the upper half of instruction word
            let second_half: Unsigned<16> = Self::pc_halfword(self, state.PC);
            let mut PC = state.PC + Bitvector::<64>::new(2);

            let opcode = Ext::<5>::ext(first_half_noncomp);
            let rd = Into::<Bitvector<5>>::into(Ext::<5>::ext(
                first_half_noncomp >> Unsigned::<14>::new(5),
            ));
            let first_half_rest = Ext::<4>::ext(first_half_noncomp >> Unsigned::<14>::new(10));

            let mut reg = Clone::clone(&state.reg);
            let mut sram_dynamic = Clone::clone(&state.sram_dynamic);

            // RV64I implemented here

            bitmask_switch!(opcode {
                "01100" => {
                    // R-type bitwise/arith/slt(u)
                    let funct3 = Ext::<3>::ext(first_half_rest);
                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let rs2: Bitvector<5> = Self::extract_rs2(second_half);

                    // funct7 is in 25:31, i.e. 9:15 in the second half
                    let funct7 = Ext::<7>::ext(second_half >> Unsigned::<16>::new(9));

                    let value1 = reg[rs1];
                    let value2 = reg[rs2];

                    let mut result = Bitvector::<64>::new(0);


                    bitmask_switch!(funct3 {
                        "000" => {
                            // ADD/SUB
                            if funct7 == Unsigned::<7>::new(0) {
                                // ADD
                                result = value1 + value2;
                            } else if funct7 == Unsigned::<7>::new(0x20) {
                                // SUB
                                result = value1 - value2;
                            } else {
                                unimplemented!("ADD/SUB-like with other funct7");
                            }
                        },
                        "100" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // XOR
                                result = value1 ^ value2;
                            } else {
                                unimplemented!("XOR-like with other funct7");
                            }
                        }
                        "110" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // OR
                                result = value1 | value2;
                            } else {
                                unimplemented!("OR-like with other funct7");
                            }
                        }
                        "111" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // AND
                                result = value1 & value2;
                            } else {
                                unimplemented!("AND-like with other funct7");
                            }
                        }
                        "001" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // SLL
                                // 64-bit only uses the lower 6 bits of rs2
                                // mask out the others first
                                let shift_amount = value2 & Bitvector::<64>::new(0b111_111);
                                result = value1 << shift_amount;
                            } else {
                                unimplemented!("SLL-like with other funct7");
                            }
                        }
                        "101" => {
                            // SRL/SRA
                            // 64-bit only uses the lower 6 bits of rs2 for shifting
                            // mask out the others first
                            let shift_amount = value2 & Bitvector::<64>::new(0b111_111);

                            if funct7 == Unsigned::<7>::new(0) {
                                // SRL
                                // i.e. shift as unsigned
                                result = Into::<Bitvector<64>>::into(Into::<Unsigned<64>>::into(value1) >> Into::<Unsigned<64>>::into(shift_amount));
                            } else if funct7 == Unsigned::<7>::new(0x20) {
                                // SRA
                                // i.e. shift as signed
                                result = Into::<Bitvector<64>>::into(Into::<Signed<64>>::into(value1) >> Into::<Signed<64>>::into(shift_amount));
                            } else {
                                unimplemented!("SRL/SRA-like with other funct7");
                            }
                        }
                        "010" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // SLT (set less than)
                                if Into::<Signed<64>>::into(value1) < Into::<Signed<64>>::into(value2) {
                                    result = Bitvector::<64>::new(1);
                                } else {
                                    result = Bitvector::<64>::new(0);
                                }
                            } else {
                                unimplemented!("SLT-like with other funct7");
                            }
                        }
                        "011" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // SLTU (set less than unsigned)
                                if Into::<Unsigned<64>>::into(value1) < Into::<Unsigned<64>>::into(value2) {
                                    result = Bitvector::<64>::new(1);
                                } else {
                                    result = Bitvector::<64>::new(0);
                                }
                            } else {
                                unimplemented!("SLT-like with other funct7");
                            }
                        }
                    });

                    let store;
                    if rd != Bitvector::<5>::new(0) {
                        store = result;
                    } else {
                        store = Bitvector::<64>::new(0);
                    }
                    reg[rd] = store;
                }
                "01110" => {
                    // R-type bitwise/arith/slt(u), low word only (32-bit)
                    let funct3 = Ext::<3>::ext(first_half_rest);
                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let rs2: Bitvector<5> = Self::extract_rs2(second_half);

                    // funct7 is in 25:31, i.e. 9:15 in the second half
                    let funct7 = Ext::<7>::ext(second_half >> Unsigned::<16>::new(9));

                    // take only the low 32 bits and compute only low 32 bits
                    let value1 = Into::<Bitvector<32>>::into(Ext::<32>::ext(Into::<Unsigned<64>>::into(reg[rs1])));
                    let value2 = Into::<Bitvector<32>>::into(Ext::<32>::ext(Into::<Unsigned<64>>::into(reg[rs2])));

                    let mut result_32 = Bitvector::<32>::new(0);

                    bitmask_switch!(funct3 {
                        "000" => {
                            // ADDW/SUBW
                            if funct7 == Unsigned::<7>::new(0) {
                                // ADDW
                                result_32 = value1 + value2;
                            } else if funct7 == Unsigned::<7>::new(0x20) {
                                // SUBW
                                result_32 = value1 - value2;
                            } else {
                                unimplemented!("ADDW/SUBW-like with other funct7");
                            }
                        },
                        "001" => {
                            if funct7 == Unsigned::<7>::new(0) {
                                // SLLW
                                // 32-bit only uses the lower 5 bits of rs2
                                // mask out the others first
                                let shift_amount = value2 & Bitvector::<32>::new(0b11_111);
                                result_32 = value1 << shift_amount;
                            } else {
                                unimplemented!("SLL-like with other funct7");
                            }
                        }
                        "101" => {
                            // SRLW/SRAW
                            // 32-bit only uses the lower 5 bits of rs2
                            // mask out the others first
                            let shift_amount = value2 & Bitvector::<32>::new(0b11_111);

                            if funct7 == Unsigned::<7>::new(0) {
                                // SRLW
                                // i.e. shift as unsigned
                                result_32 = Into::<Bitvector<32>>::into(Into::<Unsigned<32>>::into(value1) >> Into::<Unsigned<32>>::into(shift_amount));
                            } else if funct7 == Unsigned::<7>::new(0x20) {
                                // SRAW
                                // i.e. shift as signed
                                result_32 = Into::<Bitvector<32>>::into(Into::<Signed<32>>::into(value1) >> Into::<Signed<32>>::into(shift_amount));
                            } else {
                                unimplemented!("SRL/SRA-like with other funct7");
                            }
                        }
                        _ => {
                            unimplemented!("R-type 32-bit widening-like");
                        }
                    });
                    let store;
                    if rd != Bitvector::<5>::new(0) {
                        // sign-extend the 32-bit result
                        store = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<32>>::into(result_32)));
                    } else {
                        store = Bitvector::<64>::new(0);
                    }
                    reg[rd] = store;
                }
                "00100" => {
                    // I-type normal immediate
                    let funct3 = Ext::<3>::ext(first_half_rest);

                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let value1 = reg[rs1];

                    // the immediate is taken as signed
                    let imm_low = Ext::<12>::ext(second_half >> Unsigned::<16>::new(4));
                    let imm = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<12>>::into(imm_low)));

                    let mut result = Bitvector::<64>::new(0);

                    bitmask_switch!(funct3 {
                        "000" => {
                            // ADDI
                            result = value1 + imm;
                        }
                        "100" => {
                            // XORI
                            result = value1 ^ imm;
                        }
                        "110" => {
                            // ORI
                            result = value1 | imm;
                        }
                        "111" => {
                            // ANDI
                            result = value1 & imm;
                        }
                        "001" => {
                            // SLLI
                            // RV64I: 6 instead of 5 immediate bits
                            let imm_lo = Ext::<6>::ext(Into::<Unsigned::<64>>::into(imm));
                            let imm_hi = Ext::<6>::ext(Into::<Unsigned::<64>>::into(imm) >> Unsigned::<64>::new(6));
                            // imm[6:11] must be 0x00
                            if imm_hi == Unsigned::<6>::new(0) {
                                result = Into::<Bitvector<64>>::into(Into::<Unsigned<64>>::into(value1) << Ext::<64>::ext(imm_lo));
                            } else {
                                unimplemented!("SLLI-like with non-zero high immediate");
                            }
                        }
                        "101" => {
                            // SRLI / SRAI
                            // RV64I: 6 instead of 5 immediate bits
                            let imm_lo = Ext::<6>::ext(Into::<Unsigned::<64>>::into(imm));
                            let imm_hi = Ext::<6>::ext(Into::<Unsigned::<64>>::into(imm) >> Unsigned::<64>::new(6));
                            // logical (SRLI) when imm[6:11] = 0x00
                            // arithmetical (SRAI) when imm[6:11] = 0x10
                            if imm_hi == Unsigned::<6>::new(0) {
                                result = Into::<Bitvector<64>>::into(Into::<Unsigned<64>>::into(value1) << Ext::<64>::ext(imm_lo));
                            } else if imm_hi == Unsigned::<6>::new(0x10) {
                                result = Into::<Bitvector<64>>::into(Into::<Signed<64>>::into(value1) << Into::<Signed<64>>::into(Ext::<64>::ext(imm_lo)));
                            } else {
                                unimplemented!("SRLI/SRAI-like with unrecognised high immediate");
                            }
                        }
                        "010" => {
                            // SLTI (set less than immediate, signed)
                            // compare rs1 value with sign-extended immediate, using signed comparison
                            // write 1 to result if rs1 is lesser, 0 otherwise
                            if Into::<Signed::<64>>::into(value1) < Into::<Signed::<64>>::into(imm) {
                                result = Bitvector::<64>::new(1);
                            } else {
                                result = Bitvector::<64>::new(0);
                            }
                        }
                        "011" => {
                            // SLTIU (set less than immediate, unsigned)
                            // the immediate is sign extended as normal, but unsigned comparison is used
                            // write 1 to result if rs1 is lesser, 0 otherwise
                            if Into::<Unsigned::<64>>::into(value1) < Into::<Unsigned::<64>>::into(imm) {
                                result = Bitvector::<64>::new(1);
                            } else {
                                result = Bitvector::<64>::new(0);
                            }
                        }
                    });

                    let store;
                    if rd != Bitvector::<5>::new(0) {
                        store = result;
                    } else {
                        store = Bitvector::<64>::new(0);
                    }
                    reg[rd] = store;
                }
                "00110" => {
                    // I-type normal immediate, low word only (32-bit)
                    let funct3 = Ext::<3>::ext(first_half_rest);

                    // take only the low 32 bits
                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let value1 = Into::<Bitvector<32>>::into(Ext::<32>::ext(Into::<Unsigned<64>>::into(reg[rs1])));

                    // the immediate is taken as signed
                    let imm_low = Ext::<12>::ext(second_half >> Unsigned::<16>::new(4));
                    let imm = Into::<Bitvector<32>>::into(Ext::<32>::ext(Into::<Signed<12>>::into(imm_low)));

                    // compute only low 32 bits
                    let mut result_32 = Bitvector::<32>::new(0);

                    bitmask_switch!(funct3 {
                        "000" => {
                            // ADDIW
                            result_32 = value1 + imm;
                        }
                        "001" => {
                            // SLLIW
                            // 32-bit: 5 immediate bits
                            let imm_lo = Ext::<5>::ext(Into::<Unsigned::<32>>::into(imm));
                            let imm_hi = Ext::<7>::ext(Into::<Unsigned::<32>>::into(imm) >> Unsigned::<32>::new(5));
                            // imm[5:11] must be 0x00
                            if imm_hi == Unsigned::<7>::new(0) {
                                result_32 = Into::<Bitvector<32>>::into(Into::<Unsigned<32>>::into(value1) << Ext::<32>::ext(imm_lo));
                            } else {
                                unimplemented!("SLLIW-like with non-zero high immediate");
                            }
                        }
                        "101" => {
                            // SRLIW / SRAIW
                            // 32-bit: 5 immediate bits
                            let imm_lo = Ext::<5>::ext(Into::<Unsigned::<32>>::into(imm));
                            let imm_hi = Ext::<7>::ext(Into::<Unsigned::<32>>::into(imm) >> Unsigned::<32>::new(5));
                            // logical (SRLIW) when imm[5:11] = 0x00
                            // arithmetical (SRAIW) when imm[5:11] = 0x20
                            if imm_hi == Unsigned::<7>::new(0) {
                                result_32 = Into::<Bitvector<32>>::into(Into::<Unsigned<32>>::into(value1) << Ext::<32>::ext(imm_lo));
                            } else if imm_hi == Unsigned::<7>::new(0x20) {
                                result_32 = Into::<Bitvector<32>>::into(Into::<Signed<32>>::into(value1) << Into::<Signed<32>>::into(Ext::<32>::ext(imm_lo)));
                            } else {
                                unimplemented!("SRLIW/SRAIW-like with unrecognised high immediate");
                            }
                        }
                        _ => {
                            unimplemented!("I-type 32-bit widening-like");
                        }
                    });

                    let store;
                    if rd != Bitvector::<5>::new(0) {
                        // sign-extend the 32-bit result
                        store = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<32>>::into(result_32)));
                    } else {
                        store = Bitvector::<64>::new(0);
                    }
                    reg[rd] = store;
                }
                "00000" => {
                    // I-type load
                    // LB / LH / LW / LBU / LHU

                    let funct3 = Ext::<3>::ext(first_half_rest);

                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let value1 = reg[rs1];

                    // the immediate is taken as signed
                    let imm_low = Ext::<12>::ext(second_half >> Unsigned::<16>::new(4));
                    let imm = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<12>>::into(imm_low)));

                    let address = Into::<Unsigned<64>>::into(value1 + imm);


                    reg = Self::load(self, state, input, rd, address, funct3);
                }
                "01000" => {
                    // S-type store
                    // SB / SH / SW

                    let funct3 = Ext::<3>::ext(first_half_rest);
                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let rs2: Bitvector<5> = Self::extract_rs2(second_half);

                    let imm_low: Bitvector<12> = Self::extract_s_type_imm(first_half_noncomp, second_half);
                    let imm: Unsigned<64> =Into::<Unsigned<64>>::into(Ext::<64>::ext(Into::<Signed<12>>::into(imm_low)));

                    let value1 = Into::<Unsigned<64>>::into(reg[rs1]);

                    let address = value1 + imm;
                    let store_value = reg[rs2];

                    let store_result: State = Self::store(state, address, store_value, funct3);
                    reg = Clone::clone(&store_result.reg);
                    sram_dynamic = Clone::clone(&store_result.sram_dynamic);
                }
                "11000" => {
                    // B-type branch
                    let funct3 = Ext::<3>::ext(first_half_rest);
                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);
                    let rs2: Bitvector<5> = Self::extract_rs2(second_half);

                    // branches have the immediate organised similarly to S-type instructions
                    // but instead of the bit 0 in S-type immediate, there is 0
                    // the bit 0 of S-type imm is instead used as bit 11 in B-type imm
                    // and the bit 11 of S-type imm is instead used as bit 12 in B-type imm

                    let s_imm: Bitvector<12> = Self::extract_s_type_imm(first_half_noncomp, second_half);
                    let s_imm_unsigned: Unsigned<12> = Into::<Unsigned<12>>::into(s_imm);

                    let imm_11 = Ext::<1>::ext(s_imm_unsigned);
                    let imm_10_1 = Ext::<10>::ext(s_imm_unsigned >> Unsigned::<12>::new(1));
                    let imm_12 = Ext::<1>::ext(s_imm_unsigned >> Unsigned::<12>::new(11));

                    let imm_11_placed = Ext::<13>::ext(imm_11) << Unsigned::<13>::new(11);
                    let imm_10_1_placed = Ext::<13>::ext(imm_10_1) << Unsigned::<13>::new(1);
                    let imm_12_placed = Ext::<13>::ext(imm_12) << Unsigned::<13>::new(12);

                    let short_imm = imm_11_placed | imm_10_1_placed | imm_12_placed;

                    // extend immediate as signed integer
                    let extended_imm = Ext::<64>::ext(Into::<Signed<13>>::into(short_imm));

                    // determine if we should branch
                    let value1 = reg[rs1];
                    let value2 = reg[rs2];

                    let mut should_branch = Bitvector::<1>::new(0);

                    bitmask_switch!(funct3 {
                        "000" => {
                            // BEQ (branch if equal)
                            if value1 == value2 {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                        "001" => {
                            // BNE (branch if not equal)
                            if value1 != value2 {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                        "01-" => {
                            unimplemented!("Non-standard branch");
                        }
                        "100" => {
                            // BLT (branch if less than, signed)
                            if Into::<Signed::<64>>::into(value1) < Into::<Signed::<64>>::into(value2) {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                        "101" => {
                            // BGE (branch if greater or equal, signed)
                            if Into::<Signed::<64>>::into(value1) >= Into::<Signed::<64>>::into(value2) {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                        "110" => {
                            // BLTU (branch if less than, unsigned)
                            if Into::<Unsigned::<64>>::into(value1) < Into::<Unsigned::<64>>::into(value2) {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                        "111" => {
                            // BGEU (branch if greater or equal, unsigned)
                            if Into::<Unsigned::<64>>::into(value1) >= Into::<Unsigned::<64>>::into(value2) {
                                should_branch = Bitvector::<1>::new(1);
                            }
                        }
                    });

                    if should_branch == Bitvector::<1>::new(1) {
                        // undo pre-increment (non-compressed instruction, 4 bytes) and add immediate to PC
                       PC = PC - Bitvector::<64>::new(4) + Into::<Bitvector<64>>::into(Ext::<64>::ext(extended_imm));
                    }

                }
                "11011" => {
                    // J-type
                    // JAL (Jump and Link)

                    // bits 12:19 in opcode encode 12:19 in immediate
                    // handle 12:15 and 16:19 separately
                    let imm_15_12 = Ext::<4>::ext(first_half_noncomp >> Unsigned::<14>::new(10));
                    let imm_19_16 = Ext::<4>::ext(second_half);

                    // bit 20 in opcode (i.e. 4 in second half) encodes 11 in immediate
                    let imm_11 = Ext::<1>::ext(second_half >> Unsigned::<16>::new(4));

                    // bits 21:30 in opcode (i.e. 5:14 in second half) encode 1:10 in immediate
                    let imm_10_1 = Ext::<10>::ext(second_half >> Unsigned::<16>::new(5));

                    // bit 31 in opcode (i.e. 15 in second half) encodes 20 in immediate
                    let imm_20 = Ext::<1>::ext(second_half >> Unsigned::<16>::new(15));

                    // construct 21-bit immediate (bit 0 is zero)
                    let imm_15_12_placed = Ext::<20>::ext(imm_15_12) << Unsigned::<20>::new(12);
                    let imm_19_16_placed = Ext::<20>::ext(imm_19_16) << Unsigned::<20>::new(16);
                    let imm_11_placed = Ext::<20>::ext(imm_11) << Unsigned::<20>::new(11);
                    let imm_10_1_placed = Ext::<20>::ext(imm_10_1) << Unsigned::<20>::new(1);
                    let imm_20_placed = Ext::<20>::ext(imm_20) << Unsigned::<20>::new(20);

                    let imm = imm_15_12_placed | imm_19_16_placed | imm_11_placed | imm_10_1_placed | imm_20_placed;

                    // sign-extend the immediate to obtain the actual offset
                    let offset = Ext::<64>::ext(Into::<Signed<20>>::into(imm));

                    // if the destination (link) register is not zero, write to it
                    // the address of the next instructions (currently in PC)
                    let link_value;
                    if rd != Bitvector::<5>::new(0) {
                        link_value = PC;
                    } else {
                        link_value = Bitvector::<64>::new(0);
                    }
                    reg[rd] = link_value;

                    // relative jump to the sum of pre-increment PC and offset
                    // undo pre-increment (non-compressed instruction, 4 bytes) and add offset to PC
                    PC = PC - Bitvector::<64>::new(4) + Into::<Bitvector<64>>::into(Ext::<64>::ext(offset));
                }
                "11001" => {
                    // I-type
                    // JALR (Jump and Link Register)
                    let funct3 = Ext::<3>::ext(first_half_rest);
                    if funct3 != Unsigned::<3>::new(0) {
                        unimplemented!("JALR-like with unrecognised funct3");
                    }

                    let rs1: Bitvector<5> = Self::extract_rs1(first_half_rest, second_half);

                    // in JALR, the immediate is stored in bits 20:31
                    // i.e. bits 4:15 of second half-word
                    let imm_low = Ext::<12>::ext(second_half >> Unsigned::<16>::new(4));
                    // signed-extend the immediate
                    let imm = Into::<Bitvector::<64>>::into(Ext::<64>::ext(Into::<Signed<12>>::into(imm_low)));

                    // if the destination (link) register is not zero, write to it
                    // the address of the next instructions (currently in PC)
                    let link_value;
                    if rd != Bitvector::<5>::new(0) {
                        link_value = PC;
                    } else {
                        link_value = Bitvector::<64>::new(0);
                    }
                    reg[rd] = link_value;

                    let value1 = reg[rs1];

                    // absolute jump to the sum of the rs1 value and immediate
                    // to obtain the new PC value
                    // the least significant bit must be set to zero afterward
                    let new_pc = value1 + imm;
                    PC = new_pc & !Bitvector::<64>::new(1);
                }
                "0q101" => {
                    // U-type
                    // if q = 0, AUIPC (Add Upper Immediate to PC)
                    // if q = 1, LUI (Load Upper Immediate)
                    let imm_second_half = Ext::<20>::ext(second_half) << Unsigned::<20>::new(4);
                    let imm = imm_second_half + Ext::<20>::ext(first_half_rest);
                    let extended_imm = Ext::<32>::ext(imm) << Unsigned::<32>::new(12);
                    // RV64I: sign-extend the 32-bit result
                    let sign_extended_imm = Into::<Unsigned<64>>::into(Ext::<64>::ext(Into::<Signed<32>>::into(extended_imm)));

                    let instruction_start_pc = Into::<Unsigned<64>>::into(PC - Bitvector::<64>::new(4));

                    let mut result;

                    if q == Bitvector::<1>::new(0) {
                        // add the PC to the extended immediate
                        result = sign_extended_imm + instruction_start_pc;
                    } else {
                        // just load the extended immediate
                        result = sign_extended_imm;
                    }
                    if rd == Bitvector::<5>::new(0) {
                        result = Unsigned::<64>::new(0);
                    }
                    reg[rd] = Into::<Bitvector<64>>::into(result);

                }
                "11100" => {
                    let funct3 = Ext::<3>::ext(first_half_rest);
                    if funct3 == Unsigned::<3>::new(0) {
                            if rd == Bitvector::<5>::new(0) && first_half_rest == Unsigned::<4>::new(0) && second_half == Unsigned::<16>::new(0) {
                                // ECALL
                                unimplemented!("Environment Call");
                            }
                            else if rd == Bitvector::<5>::new(0) && first_half_rest == Unsigned::<4>::new(0) && second_half == Unsigned::<16>::new(0x10) {
                                // EBREAK
                                unimplemented!("Environment Break");
                            }
                            unimplemented!("ECall/EBreak-like (funct3 = 0)");
                    } else {
                        unimplemented!("ECall/EBreak-like (funct3 != 0)");
                    }
                }
                "00011" => {
                    // FENCE / FENCE.TSO / PAUSE
                    // do nothing as we only have one hart
                    // and are not currently concerned with external devices
                }
                _ => unimplemented!("Given non-compressed instruction")
            });

            State {
                PC,
                reg,
                sram_dynamic,
            }
        }

        fn instruction_00(
            &self,
            state: &State,
            input: &Input,
            _param: &Param,
            opcode_instr: Unsigned<14>,
        ) -> State {
            let mut reg = Clone::clone(&state.reg);
            let mut sram_dynamic = Clone::clone(&state.sram_dynamic);

            // all C extension instructions in this quadrant (0) implemented

            bitmask_switch!(opcode_instr {
                "000_u_uuuuu_uu_ddd" => {
                    if d == Bitvector::<3>::new(0) {
                        if u != Bitvector::<8>::new(0) {
                        // reserved in RV32C
                        unimplemented!("Given instruction");
                    } else {
                        // all zeroes
                        panic!("Illegal zero instruction");
                    }

                    } else if u == Bitvector::<8>::new(0) {
                            // reserved in RV32C
                            unimplemented!("Given instruction");
                    } else {
                        // C.ADDI4SPN

                        // convert the three-bit destination register code to register index
                        let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));

                        // the 8 bits of immediate u correspond to 5:4 | 9:6 | 2 | 3
                        let uimm = Into::<Unsigned<8>>::into(u);
                        let uimm_3 = Ext::<1>::ext(uimm);
                        let uimm_2 = Ext::<1>::ext(uimm >> Unsigned::<8>::new(1));
                        let uimm_9_6 = Ext::<4>::ext(uimm >> Unsigned::<8>::new(2));
                        let uimm_5_4 = Ext::<2>::ext(uimm >> Unsigned::<8>::new(6));

                        let uimm_3_placed = Ext::<64>::ext(uimm_3) << Unsigned::<64>::new(3);
                        let uimm_2_placed = Ext::<64>::ext(uimm_2) << Unsigned::<64>::new(2);
                        let uimm_9_6_placed = Ext::<64>::ext(uimm_9_6) << Unsigned::<64>::new(6);
                        let uimm_5_4_placed = Ext::<64>::ext(uimm_5_4) << Unsigned::<64>::new(4);

                        let offset = Into::<Bitvector::<64>>::into(uimm_9_6_placed | uimm_5_4_placed | uimm_3_placed | uimm_2_placed);

                        // we add the offset to the stack pointer x2
                        let result = reg[Bitvector::<5>::new(2)] + offset;

                        // and write the result to the destination register
                        reg[rd] = result;
                    }
                }
                "q10_uuu_sss_uu_ddd" => {
                    // if q = 0: C.LW
                    // if q = 1: C.SW

                    // convert the three-bit register codes to register indices
                    let rs1 = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(s)) + Unsigned::<5>::new(8));
                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));

                    // the 5 bits of immediate u correspond to 5:3 | 2 | 6
                    // note the difference from C.LD/C.SD
                    let uimm = Into::<Unsigned<5>>::into(u);
                    let uimm_6 = Ext::<1>::ext(uimm);
                    let uimm_2 = Ext::<1>::ext(uimm >> Unsigned::<5>::new(1));
                    let uimm_5_3 = Ext::<3>::ext(uimm >> Unsigned::<5>::new(2));

                    let uimm_6_placed = Ext::<64>::ext(uimm_6) << Unsigned::<64>::new(6);
                    let uimm_2_placed = Ext::<64>::ext(uimm_2) << Unsigned::<64>::new(2);
                    let uimm_5_3_placed = Ext::<64>::ext(uimm_5_3) << Unsigned::<64>::new(3);

                    let offset = Into::<Bitvector::<64>>::into(uimm_6_placed | uimm_5_3_placed | uimm_2_placed);

                    // we add the offset to the base register in rs1
                    let address = Into::<Unsigned<64>>::into(reg[rs1] + offset);

                    // we need to load/store a 32-bit value (word), i.e. funct3 value 2
                    let funct3 = Unsigned::<3>::new(2);

                    if q == Bitvector::<1>::new(0) {
                        // C.LW
                        reg = Self::load(self, state, input, rd, address, funct3);
                    } else {
                        // C.SW
                        // store the value in rd to memory
                        let store_value = reg[rd];

                        let store_result: State = Self::store(state, address, store_value, funct3);
                        reg = Clone::clone(&store_result.reg);
                        sram_dynamic = Clone::clone(&store_result.sram_dynamic);
                    }
                }
                "q11_uuu_sss_uu_ddd" => {
                    // if q = 0: C.LD
                    // if q = 1: C.SD

                    // convert the three-bit register codes to register indices
                    let rs1 = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(s)) + Unsigned::<5>::new(8));
                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));

                    // the 5 bits of immediate u correspond to 5:3 | 7 | 6
                    // note the difference from C.LW/C.SW
                    let uimm = Into::<Unsigned<5>>::into(u);
                    let uimm_6 = Ext::<1>::ext(uimm);
                    let uimm_7 = Ext::<1>::ext(uimm >> Unsigned::<5>::new(1));
                    let uimm_5_3 = Ext::<3>::ext(uimm >> Unsigned::<5>::new(2));

                    let uimm_6_placed = Ext::<64>::ext(uimm_6) << Unsigned::<64>::new(6);
                    let uimm_7_placed = Ext::<64>::ext(uimm_7) << Unsigned::<64>::new(7);
                    let uimm_5_3_placed = Ext::<64>::ext(uimm_5_3) << Unsigned::<64>::new(3);

                    let offset = Into::<Bitvector::<64>>::into(uimm_7_placed | uimm_6_placed | uimm_5_3_placed);

                    // we add the offset to the base register in rs1
                    let address = Into::<Unsigned<64>>::into(reg[rs1] + offset);

                    // we need to load/store a 64-bit value (double word), i.e. funct3 value 3
                    let funct3 = Unsigned::<3>::new(3);

                    if q == Bitvector::<1>::new(0) {
                        // C.LW
                        reg = Self::load(self, state, input, rd, address, funct3);
                    } else {
                        // C.SW
                        // store the value in rd to memory
                        let store_value = reg[rd];

                        let store_result: State = Self::store(state, address, store_value, funct3);
                        reg = Clone::clone(&store_result.reg);
                        sram_dynamic = Clone::clone(&store_result.sram_dynamic);
                    }
                }

                _ => unimplemented!("Given instruction in compressed quadrant 0")
            });

            State {
                PC: state.PC,
                reg,
                sram_dynamic,
            }
        }

        fn instruction_01(
            &self,
            state: &State,
            _input: &Input,
            _param: &Param,
            opcode_instr: Unsigned<14>,
        ) -> State {
            let mut reg = Clone::clone(&state.reg);

            let mut PC = state.PC;
            let sram_dynamic = Clone::clone(&state.sram_dynamic);

            // all C extension instructions in this quadrant (1) implemented

            bitmask_switch!(opcode_instr {
                "010_i_ddddd_iiiii" => {
                    // C.LI
                    // load immediate (sign-extended)
                    let imm = Into::<Signed<6>>::into(i);
                    if d != Bitvector::<5>::new(0) {
                        reg[d] = Into::<Bitvector<64>>::into(Ext::<64>::ext(imm));
                    }
                }
                "011_i_ddddd_iiiii" => {
                    if i == Bitvector::<6>::new(0) {
                        // this part is reserved
                        unimplemented!("Reserved C.LUI-like");
                    }

                    if d == Bitvector::<5>::new(2) {
                        // C.ADDI16SP
                        // add sign-extended immediate scaled to represent multiples of 16 (2 to the power of 4)
                        let lower_imm = Ext::<10>::ext(Into::<Unsigned<6>>::into(i)) << Unsigned::<10>::new(4);
                        let imm = Ext::<64>::ext(Into::<Signed<10>>::into(lower_imm));

                        reg[d] = Into::<Bitvector<64>>::into(imm);
                    } else if d != Bitvector::<5>::new(0) {
                        // C.LUI
                        // the immediate corresponds to bits 17:12
                        // lower bits are cleared and the upper sign-extended

                        let lower_imm = Ext::<17>::ext(Into::<Unsigned<6>>::into(i)) << Unsigned::<17>::new(12);
                        let imm = Ext::<64>::ext(Into::<Signed<17>>::into(lower_imm));

                        reg[d] = Into::<Bitvector<64>>::into(imm);
                    } else {
                        // hint, do nothing
                    }
                }
                "001_i_ddddd_iiiii" => {
                    if d != Bitvector::<5>::new(0) {
                        // C.ADDIW
                        // add immediate to word (sign-extended), then sign-extend to 64 bits
                        // note the encoding is reassigned from C.JAL present in RV32C
                        let imm = Into::<Signed<6>>::into(i);
                        let src = Into::<Signed<32>>::into(Ext::<32>::ext(Into::<Signed<64>>::into(reg[d])));
                        let result_32 = src + Ext::<32>::ext(imm);
                        reg[d] = Into::<Bitvector<64>>::into(Ext::<64>::ext(result_32));
                    } else {
                        unimplemented!("Reserved C.ADDIW-like");
                    }
                }
                "101_h_bffgd_eaaac" => {
                    // C.J (jump)

                    // according to the spec, indexing imm (opcode_instr) from 1,
                    // the offset is specified by 11|4|9:8|10|6|7|3:1|5

                    let a_part = Ext::<12>::ext(Into::<Unsigned::<3>>::into(a)) << Unsigned::<12>::new(1);
                    let b_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(b)) << Unsigned::<12>::new(4);
                    let c_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(c)) << Unsigned::<12>::new(5);
                    let d_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(d)) << Unsigned::<12>::new(6);
                    let e_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(e)) << Unsigned::<12>::new(7);
                    let f_part = Ext::<12>::ext(Into::<Unsigned::<2>>::into(f)) << Unsigned::<12>::new(8);
                    let g_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(g)) << Unsigned::<12>::new(10);
                    let h_part = Ext::<12>::ext(Into::<Unsigned::<1>>::into(h)) << Unsigned::<12>::new(11);

                    let offset = Into::<Bitvector<12>>::into(a_part | b_part | c_part | d_part | e_part | f_part | g_part | h_part);

                    let extended_offset = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<12>>::into(offset)));

                    // no link value store as C.JAL is not present

                    // undo pre-increment and add offset to PC
                    PC = PC - Bitvector::<64>::new(2) + extended_offset;

                }
                "11q_ebb_sss_ddaac" => {
                    // C.BEQZ (q = 0)
                    // or C.BNEZ (q = 1)

                    let a_part = Ext::<9>::ext(Into::<Unsigned::<2>>::into(a)) << Unsigned::<9>::new(1);
                    let b_part = Ext::<9>::ext(Into::<Unsigned::<2>>::into(b)) << Unsigned::<9>::new(3);
                    let c_part = Ext::<9>::ext(Into::<Unsigned::<1>>::into(c)) << Unsigned::<9>::new(5);
                    let d_part = Ext::<9>::ext(Into::<Unsigned::<2>>::into(d)) << Unsigned::<9>::new(6);
                    let e_part = Ext::<9>::ext(Into::<Unsigned::<1>>::into(e)) << Unsigned::<9>::new(8);

                    let offset = Into::<Bitvector<9>>::into(a_part | b_part | c_part | d_part | e_part);

                    let extended_offset = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<9>>::into(offset)));

                    // three-bit register operands map to registers 8:15
                    let rs1 = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(s)) + Unsigned::<5>::new(8));

                    let value1 = reg[rs1];

                    let mut should_branch = Bitvector::<1>::new(0);

                    if q == Bitvector::<1>::new(0) {
                        // branch if equal to zero
                        if value1 == Bitvector::<64>::new(0) {
                            should_branch = Bitvector::<1>::new(1);
                        }
                    } else {
                        // branch if not equal to zero
                        if value1 != Bitvector::<64>::new(0) {
                            should_branch = Bitvector::<1>::new(1);
                        }
                    }

                    if should_branch == Bitvector::<1>::new(1) {
                        // undo pre-increment and add offset to PC
                        PC = PC - Bitvector::<64>::new(2) + extended_offset;
                    }
                }
                "000_n_ddddd_nnnnn" => {
                    // C.NOP (n == 0) or C.ADDI (n != 0)
                    if n != Bitvector::<6>::new(0) {
                        // sign-extend immediate
                        let extended_imm = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<6>>::into(n)));

                        let result;

                        if d != Bitvector::<5>::new(0) {
                            result = reg[d] + extended_imm;
                        } else {
                            // hint
                            result = Bitvector::<64>::new(0);
                        }

                        reg[d] = result;
                    }
                }
                "100_0_0a_ddd_uuuuu" => {
                    // C.SRLI (a = 0) or C.SRAI (a = 1)
                    // HINT if uimm is zero

                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));
                    let shift_amount = Ext::<64>::ext(Into::<Unsigned::<5>>::into(u));

                    if a == Bitvector::<1>::new(1) {
                        // C.SRAI
                        let shift_amount_signed = Into::<Signed::<64>>::into(shift_amount);
                        reg[rd] = Into::<Bitvector<64>>::into(Into::<Signed<64>>::into(reg[rd]) << shift_amount_signed);
                    } else {
                        // C.SRLI
                        reg[rd] = Into::<Bitvector<64>>::into(Into::<Unsigned<64>>::into(reg[rd]) >> shift_amount);
                    }
                }
                "100_i_10_ddd_iiiii" => {
                    // C.ANDI

                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));
                    let immediate = Into::<Bitvector::<64>>::into(Ext::<64>::ext(Into::<Signed::<6>>::into(i)));

                    reg[rd] = reg[rd] & immediate;
                }
                "100_0_11_ddd_pp_sss" => {
                    // C.SUB / C.XOR / C.OR / C.AND

                    // three-bit register operands map to registers 8:15
                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));
                    let rs2 = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(s)) + Unsigned::<5>::new(8));

                    bitmask_switch!(p {
                        "00" => {
                            // C.SUB
                            reg[rd] = reg[rd] - reg[rs2];
                        }
                        "01" => {
                            // C.XOR
                            reg[rd] = reg[rd] ^ reg[rs2];
                        }
                        "10" => {
                            // C.OR
                            reg[rd] = reg[rd] | reg[rs2];
                        }
                        "11" => {
                            // C.AND
                            reg[rd] = reg[rd] & reg[rs2];

                        }
                    });
                }
                "100_1_11_ddd_0p_sss" => {
                    // p = 0: C.SUBW
                    // p = 1: C.ADDW

                    // three-bit register operands map to registers 8:15
                    let rd = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(d)) + Unsigned::<5>::new(8));
                    let rs2 = Into::<Bitvector::<5>>::into(Ext::<5>::ext(Into::<Unsigned<3>>::into(s)) + Unsigned::<5>::new(8));

                    if p == Bitvector::<1>::new(1) {
                        // C.ADDW
                        reg[rd] = reg[rd] + reg[rs2];
                    } else {
                        // C.SUBW
                        reg[rd] = reg[rd] - reg[rs2];
                    }
                }
                _ => unimplemented!("Given instruction in compressed quadrant 1")
            });

            State {
                PC,
                reg,
                sram_dynamic,
            }
        }

        fn instruction_10(
            &self,
            state: &State,
            input: &Input,
            _param: &Param,
            opcode_instr: Unsigned<14>,
        ) -> State {
            let mut PC = state.PC;
            let mut reg = Clone::clone(&state.reg);
            let mut sram_dynamic = Clone::clone(&state.sram_dynamic);

            // all C extension instructions in this quadrant (2) implemented

            bitmask_switch!(opcode_instr {
                "000_u_ddddd_uuuuu" => {
                    // C.SLLI (if rd = 0 or imm = 0, hint)

                    let shamt: Unsigned<6> = Into::<Unsigned<6>>::into(u);

                    // shamt5 must be zero
                    if Ext::<1>::ext(shamt >> Unsigned::<6>::new(5)) == Unsigned::<1>::new(1) {
                        panic!("C.SLLI-like with nonzero highest shift bit not permitted in RV32C");
                    }

                    if d != Bitvector::<5>::new(0) {
                        reg[d] = Into::<Bitvector<64>>::into(Into::<Unsigned<64>>::into(reg[d]) << Ext::<64>::ext(shamt));
                    }
                }

                "100_q_ddddd_sssss" => {
                    // C.MV / C.ADD / C.JR / C.JALR

                    let rd = d;
                    let rs2 = s;
                    // if rd != 0 and rs2 != 0: C.MV (q = 0) or C.ADD (q = 1)
                    // if rd != 0 and rs2 == 0: C.JR (q = 0) or C.JALR (q = 1)
                    // if rd == 0 and rs2 != 0: not in compressed extension
                    // if rd == 0 and rs2 == 0: not in compressed extension (q = 0) or C.EBREAK (q = 1)

                    if rs2 != Bitvector::<5>::new(0) {
                        // only a hint if rd is zero
                        if rd != Bitvector::<5>::new(0) {
                            // Move (q = 0) or Add (q = 1)
                            if q == Bitvector::<1>::new(1) {
                                // Add
                                reg[rd] = reg[rd] + reg[rs2];
                            } else {
                                // Move
                                reg[rd] = reg[rs2];
                            }
                        }
                    } else if rd != Bitvector::<5>::new(0) {
                            // rs2 == 0, rd != 0
                            // Jump Relative (q = 0) or Jump and Link Relative (q = 1)

                            if q == Bitvector::<1>::new(1) {
                                // link register is hard-wired to x1
                                // write the address of the next instruction (currently in PC) to it
                                reg[Bitvector::<5>::new(1)] = PC;
                            }

                            // there is no immediate, jump to the rd value
                            // the least significant bit must be set to zero afterward

                            let jump_address = reg[rd];
                            PC = jump_address & !Bitvector::<64>::new(1);

                        } else if q == Bitvector::<1>::new(1) {
                                // rs2 == 0, rd2 == 0, q == 1
                                panic!("Environment break");
                            } else {
                                // rs2 == 0, rd2 == 0, q == 0
                                // reserved in compressed extension
                                unimplemented!("Compressed C.JR/C.JALR-like");
                            }
                }
                "010_u_ddddd_uuuuu" => {
                    if d != Bitvector::<5>::new(0) {
                        // C.LWSP

                        let uimm = Into::<Unsigned<6>>::into(u);
                        // the 6 bits of immediate u correspond to 5 | 4:2 | 7:6
                        let uimm_7_6 = Ext::<2>::ext(uimm);
                        let uimm_4_2 = Ext::<3>::ext(uimm >> Unsigned::<6>::new(2));
                        let uimm_5 = Ext::<3>::ext(uimm >> Unsigned::<6>::new(5));

                        let uimm_7_6_placed = Ext::<64>::ext(uimm_7_6) << Unsigned::<64>::new(6);
                        let uimm_4_2_placed = Ext::<64>::ext(uimm_4_2) << Unsigned::<64>::new(2);
                        let uimm_5_placed = Ext::<64>::ext(uimm_5) << Unsigned::<64>::new(5);

                        let offset = Into::<Bitvector::<64>>::into(uimm_7_6_placed | uimm_4_2_placed | uimm_5_placed);

                        // the effective address adds the offset to the stack pointer x2
                        let address = Into::<Unsigned::<64>>::into(reg[Bitvector::<5>::new(2)] + offset);

                        // we need to load a 32-bit value (word), i.e. funct3 value 2
                        let funct3 = Unsigned::<3>::new(2);

                        reg = Self::load(self, state, input, d, address, funct3);
                    } else {
                        unimplemented!("Reserved C.LWSP-like");
                    }
                }
                "011_u_ddddd_uuuuu" => {
                    if d != Bitvector::<5>::new(0) {
                        // C.LWDP

                        let uimm = Into::<Unsigned<6>>::into(u);
                        // the 6 bits of immediate u correspond to 5 | 4:3 | 8:6
                        let uimm_8_6 = Ext::<3>::ext(uimm);
                        let uimm_4_3 = Ext::<2>::ext(uimm >> Unsigned::<6>::new(3));
                        let uimm_5 = Ext::<3>::ext(uimm >> Unsigned::<6>::new(5));

                        let uimm_8_6_placed = Ext::<64>::ext(uimm_8_6) << Unsigned::<64>::new(6);
                        let uimm_4_3_placed = Ext::<64>::ext(uimm_4_3) << Unsigned::<64>::new(3);
                        let uimm_5_placed = Ext::<64>::ext(uimm_5) << Unsigned::<64>::new(5);

                        let offset = Into::<Bitvector::<64>>::into(uimm_8_6_placed | uimm_5_placed | uimm_4_3_placed);

                        // the effective address adds the offset to the stack pointer x2
                        let address = Into::<Unsigned::<64>>::into(reg[Bitvector::<5>::new(2)] + offset);

                        // we need to load a 64-bit value (double word), i.e. funct3 value 3
                        let funct3 = Unsigned::<3>::new(3);

                        reg = Self::load(self, state, input, d, address, funct3);
                    } else {
                        unimplemented!("Reserved C.LWDP-like");
                    }
                }
                "110_u_uuuuu_sssss" => {
                    // C.SWSP

                    // the 6 bits of immediate u correspond to 5:2 | 7:6
                    let uimm = Into::<Unsigned<6>>::into(u);
                    let uimm_7_6 = Ext::<2>::ext(uimm);
                    let uimm_5_2 = Ext::<4>::ext(uimm >> Unsigned::<6>::new(2));

                    let uimm_7_6_placed = Ext::<64>::ext(uimm_7_6) << Unsigned::<64>::new(6);
                    let uimm_5_2_placed = Ext::<64>::ext(uimm_5_2) << Unsigned::<64>::new(2);

                    let offset = Into::<Bitvector::<64>>::into(uimm_7_6_placed | uimm_5_2_placed);

                    // the effective address adds the offset to the stack pointer x2
                    let address = Into::<Unsigned::<64>>::into(reg[Bitvector::<5>::new(2)] + offset);

                    // we need to store a 32-bit value (word), i.e. funct3 value 2
                    let funct3 = Unsigned::<3>::new(2);
                    let store_value = reg[s];
                    let store_result: State = Self::store(state, address, store_value, funct3);
                    reg = Clone::clone(&store_result.reg);
                    sram_dynamic = Clone::clone(&store_result.sram_dynamic);

                }
                "111_u_uuuuu_sssss" => {
                    // C.SDSP

                    // the 6 bits of immediate u correspond to 5:3 | 8:6
                    let uimm = Into::<Unsigned<6>>::into(u);
                    let uimm_8_6 = Ext::<3>::ext(uimm);
                    let uimm_5_3 = Ext::<3>::ext(uimm >> Unsigned::<6>::new(3));

                    let uimm_8_6_placed = Ext::<64>::ext(uimm_8_6) << Unsigned::<64>::new(6);
                    let uimm_5_3_placed = Ext::<64>::ext(uimm_5_3) << Unsigned::<64>::new(3);

                    let offset = Into::<Bitvector::<64>>::into(uimm_8_6_placed | uimm_5_3_placed);

                    // the effective address adds the offset to the stack pointer x2
                    let address = Into::<Unsigned::<64>>::into(reg[Bitvector::<5>::new(2)] + offset);

                    // we need to store a 64-bit value (word), i.e. funct3 value 3
                    let funct3 = Unsigned::<3>::new(3);
                    let store_value = reg[s];
                    let store_result: State = Self::store(state, address, store_value, funct3);
                    reg = Clone::clone(&store_result.reg);
                    sram_dynamic = Clone::clone(&store_result.sram_dynamic);

                }
                _ => unimplemented!("Given instruction in compressed quadrant 2")
            });

            State {
                PC,
                reg,
                sram_dynamic,
            }
        }

        fn pc_halfword(&self, PC: Bitvector<64>) -> Unsigned<16> {
            // first byte of PC
            let unsigned_pc0: Unsigned<64> = Into::<Unsigned<64>>::into(PC);
            // second byte of PC
            let unsigned_pc1: Unsigned<64> =
                Into::<Unsigned<64>>::into(PC + Bitvector::<64>::new(1));
            // only fetching from sram_static is supported (0x2000_0000 - 0x2000_4000 )
            if unsigned_pc0 < Unsigned::<64>::new(0x2000_0000)
                || unsigned_pc0 >= Unsigned::<64>::new(0x2000_4000)
                || unsigned_pc1 < Unsigned::<64>::new(0x2000_0000)
                || unsigned_pc1 >= Unsigned::<64>::new(0x2000_4000)
            {
                unimplemented!("Program Counter not within SRAM_STATIC");
            }
            // index 14 is needed because of sram_static size (0x4000) only takes lower 14 bits
            let index0 = Into::<Bitvector<14>>::into(Ext::<14>::ext(unsigned_pc0));
            let index1 = Into::<Bitvector<14>>::into(Ext::<14>::ext(unsigned_pc1));

            // not sure if here sram_static or dynamic would say static
            let pc_value0 = Ext::<16>::ext(Into::<Unsigned<8>>::into(self.sram_static[index0]));

            // have to shift here for higher byte of PC
            let pc_value1 = Ext::<16>::ext(Into::<Unsigned<8>>::into(self.sram_static[index1]))
                << Unsigned::<16>::new(8);

            pc_value0 | pc_value1
        }

        fn extract_rs1(first_half_rest: Unsigned<4>, second_half: Unsigned<16>) -> Bitvector<5> {
            // rs1 is in positions 15 to 19
            let lower_from_first_half = Ext::<5>::ext(first_half_rest >> Unsigned::<4>::new(3));
            let upper_from_second_half =
                Ext::<5>::ext(Ext::<4>::ext(second_half)) << Unsigned::<5>::new(1);

            Into::<Bitvector<5>>::into(lower_from_first_half + upper_from_second_half)
        }

        fn extract_rs2(second_half: Unsigned<16>) -> Bitvector<5> {
            // rs2 is in positions 20 to 24, i.e. 4 to 8 in second half
            Into::<Bitvector<5>>::into(Ext::<5>::ext(second_half >> Unsigned::<16>::new(4)))
        }

        fn extract_s_type_imm(
            first_half_noncomp: Unsigned<14>,
            second_half: Unsigned<16>,
        ) -> Bitvector<12> {
            // s-type instructions have the immediate organised as follows
            // bits 11:7 of opcode contain 4:0 of immediate
            // bits 31:25 of opcode (i.e. 15:9 of second half) contain 12|10:5 of immediate
            let imm_4_0 = Ext::<5>::ext(first_half_noncomp >> Unsigned::<14>::new(5));
            let imm_11_5 = Ext::<7>::ext(second_half >> Unsigned::<16>::new(9));

            let imm_4_0_placed = Ext::<12>::ext(imm_4_0);
            let imm_11_5_placed = Ext::<12>::ext(imm_11_5) << Unsigned::<12>::new(5);

            Into::<Bitvector<12>>::into(imm_4_0_placed | imm_11_5_placed)
        }

        fn load(
            &self,
            state: &State,
            _input: &Input,
            rd: Bitvector<5>,
            address: Unsigned<64>,
            funct3: Unsigned<3>,
        ) -> BitvectorArray<5, 64> {
            // RV64I: the load type depends on funct3
            // 000: byte (signed)
            // 001: half-word (signed)
            // 010: word (signed)
            // 011: double word
            // 100: byte (unsigned)
            // 101: half-word (unsigned)
            // 110: word (unsigned)
            // 111: reserved

            let is_signed = funct3 & Unsigned::<3>::new(0b100) != Unsigned::<3>::new(0);
            let funct2 = Ext::<2>::ext(funct3);
            let last_address = Self::load_store_last(address, funct2);
            let mut load_value = Bitvector::<64>::new(0);

            if address >= Unsigned::<64>::new(0x2000_4000)
                && last_address < Unsigned::<64>::new(0x2000_8000)
            {
                // SRAM_dynamic
                let relative_address = Into::<Bitvector<14>>::into(Ext::<14>::ext(
                    address - Unsigned::<64>::new(0x2000_4000),
                ));

                // load little-endian
                bitmask_switch!(funct2 {
                    "00" => {
                        // byte
                        let byte0 = state.sram_dynamic[relative_address];
                        if is_signed {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<8>>::into(byte0)));
                        } else {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Unsigned<8>>::into(byte0)));
                        }
                    }
                    "01" => {
                        // halfword
                        let byte0 = state.sram_dynamic[relative_address];
                        let byte1 = state.sram_dynamic[relative_address + Bitvector::<14>::new(1)];
                        let halfword: Bitvector<16> = Self::halfword_from_bytes(byte0, byte1);
                        if is_signed {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<16>>::into(halfword)));
                        } else {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Unsigned<16>>::into(halfword)));
                        }
                    }
                    "10" => {
                        // word
                        let byte0 = state.sram_dynamic[relative_address];
                        let byte1 = state.sram_dynamic[relative_address + Bitvector::<14>::new(1)];
                        let byte2 = state.sram_dynamic[relative_address + Bitvector::<14>::new(2)];
                        let byte3 = state.sram_dynamic[relative_address + Bitvector::<14>::new(3)];
                        let word: Bitvector<32> = Self::word_from_bytes(byte0, byte1, byte2, byte3);
                        if is_signed {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Signed<32>>::into(word)));
                        } else {
                            load_value = Into::<Bitvector<64>>::into(Ext::<64>::ext(Into::<Unsigned<32>>::into(word)));
                        }
                    }
                    "11" => {
                        // double word
                        let byte0 = state.sram_dynamic[relative_address];
                        let byte1 = state.sram_dynamic[relative_address + Bitvector::<14>::new(1)];
                        let byte2 = state.sram_dynamic[relative_address + Bitvector::<14>::new(2)];
                        let byte3 = state.sram_dynamic[relative_address + Bitvector::<14>::new(3)];
                        let byte4 = state.sram_dynamic[relative_address + Bitvector::<14>::new(4)];
                        let byte5 = state.sram_dynamic[relative_address + Bitvector::<14>::new(5)];
                        let byte6 = state.sram_dynamic[relative_address + Bitvector::<14>::new(6)];
                        let byte7 = state.sram_dynamic[relative_address + Bitvector::<14>::new(7)];

                        let word0: Bitvector<32> = Self::word_from_bytes(byte0, byte1, byte2, byte3);
                        let word1: Bitvector<32> = Self::word_from_bytes(byte4, byte5, byte6, byte7);
                        let dword: Bitvector<64> = Self::dword_from_words(word0, word1);
                        if is_signed {
                            load_value = dword;
                        } else {
                            unimplemented!("Load with funct3 = 7");
                        }
                    }
                });
            } else {
                unimplemented!("Load at given address");
            }

            // load the value into the register if it is nonzero
            let mut reg = Clone::clone(&state.reg);
            if rd != Bitvector::<5>::new(0) {
                reg[rd] = load_value;
            }

            reg
        }

        fn store(
            state: &State,
            address: Unsigned<64>,
            store_value: Bitvector<64>,
            funct3: Unsigned<3>,
        ) -> State {
            // RV64I: the store type depends on funct3
            // 000: byte (signed)
            // 001: half-word (signed)
            // 010: word (signed)
            // 011: double word
            // 100: byte (unsigned)
            // 101: half-word (unsigned)
            // 110: word (unsigned)
            // 111: reserved

            if funct3 & Unsigned::<3>::new(0b100) != Unsigned::<3>::new(0) {
                unimplemented!("Store with highest byte of funct3 set");
            }
            let funct2 = Ext::<2>::ext(funct3);
            let last_address = Self::load_store_last(address, funct2);
            let store_unsigned = Into::<Unsigned<64>>::into(store_value);

            let mut sram_dynamic = Clone::clone(&state.sram_dynamic);

            if address >= Unsigned::<64>::new(0x2000_4000)
                && last_address < Unsigned::<64>::new(0x2000_8000)
            {
                // SRAM parity
                let relative_address = Into::<Bitvector<14>>::into(Ext::<14>::ext(
                    address - Unsigned::<64>::new(0x2000_4000),
                ));

                // store little-endian
                bitmask_switch!(funct2 {
                    "00" => {
                        // byte
                        sram_dynamic[relative_address] = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned));
                    }
                    "01" => {
                        // halfword
                        let byte0 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned));
                        let byte1 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(8)));

                        sram_dynamic[relative_address] = byte0;
                        sram_dynamic[relative_address + Bitvector::<14>::new(1)] = byte1;
                    }
                    "10" => {
                        // word
                        let byte0 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned));
                        let byte1 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(8)));
                        let byte2 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(16)));
                        let byte3 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(24)));

                        sram_dynamic[relative_address] = byte0;
                        sram_dynamic[relative_address + Bitvector::<14>::new(1)] = byte1;
                        sram_dynamic[relative_address + Bitvector::<14>::new(2)] = byte2;
                        sram_dynamic[relative_address + Bitvector::<14>::new(3)] = byte3;
                    }
                    "11" => {
                        // double word
                        let byte0 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned));
                        let byte1 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(8)));
                        let byte2 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(16)));
                        let byte3 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(24)));
                        let byte4 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(32)));
                        let byte5 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(40)));
                        let byte6 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(48)));
                        let byte7 = Into::<Bitvector<8>>::into(Ext::<8>::ext(store_unsigned >> Unsigned::<64>::new(56)));

                        sram_dynamic[relative_address] = byte0;
                        sram_dynamic[relative_address + Bitvector::<14>::new(1)] = byte1;
                        sram_dynamic[relative_address + Bitvector::<14>::new(2)] = byte2;
                        sram_dynamic[relative_address + Bitvector::<14>::new(3)] = byte3;
                        sram_dynamic[relative_address + Bitvector::<14>::new(4)] = byte4;
                        sram_dynamic[relative_address + Bitvector::<14>::new(5)] = byte5;
                        sram_dynamic[relative_address + Bitvector::<14>::new(6)] = byte6;
                        sram_dynamic[relative_address + Bitvector::<14>::new(7)] = byte7;
                    }
                });
            } else {
                unimplemented!("Store at given address");
            }

            State {
                PC: state.PC,
                reg: Clone::clone(&state.reg),
                sram_dynamic,
            }
        }

        fn halfword_from_bytes(byte0: Bitvector<8>, byte1: Bitvector<8>) -> Bitvector<16> {
            let byte0_placed = Ext::<16>::ext(Into::<Unsigned<8>>::into(byte0));
            let byte1_placed =
                Ext::<16>::ext(Into::<Unsigned<8>>::into(byte1)) << Unsigned::<16>::new(8);

            Into::<Bitvector<16>>::into(byte0_placed | byte1_placed)
        }

        fn word_from_bytes(
            byte0: Bitvector<8>,
            byte1: Bitvector<8>,
            byte2: Bitvector<8>,
            byte3: Bitvector<8>,
        ) -> Bitvector<32> {
            let byte0_placed = Ext::<32>::ext(Into::<Unsigned<8>>::into(byte0));
            let byte1_placed =
                Ext::<32>::ext(Into::<Unsigned<8>>::into(byte1)) << Unsigned::<32>::new(8);
            let byte2_placed =
                Ext::<32>::ext(Into::<Unsigned<8>>::into(byte2)) << Unsigned::<32>::new(16);
            let byte3_placed =
                Ext::<32>::ext(Into::<Unsigned<8>>::into(byte3)) << Unsigned::<32>::new(24);

            Into::<Bitvector<32>>::into(byte0_placed | byte1_placed | byte2_placed | byte3_placed)
        }

        fn dword_from_words(load_word0: Bitvector<32>, load_word1: Bitvector<32>) -> Bitvector<64> {
            let load_word0_placed = Ext::<64>::ext(Into::<Unsigned<32>>::into(load_word0));
            let load_word1_placed =
                Ext::<64>::ext(Into::<Unsigned<32>>::into(load_word1)) << Unsigned::<64>::new(32);

            Into::<Bitvector<64>>::into(load_word0_placed | load_word1_placed)
        }

        fn load_store_last(address: Unsigned<64>, funct2: Unsigned<2>) -> Unsigned<64> {
            let mut last_address = address;
            bitmask_switch!(funct2 {
                "00" => {
                    // byte
                }
                "01" => {
                    // halfword
                    last_address = last_address + Unsigned::<64>::new(1);
                }
                "10" => {
                    // word
                    last_address = last_address + Unsigned::<64>::new(3);
                }
                "11" => {
                    // double word
                    last_address = last_address + Unsigned::<64>::new(7);
                }
            });
            last_address
        }
    }
}
