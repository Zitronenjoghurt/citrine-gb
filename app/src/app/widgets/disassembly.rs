use citrine_gb::disassembly::{Confidence, Disassembly, DisassemblySource};
use citrine_gb::gb::cartridge::{Cartridge, RomLocation};
use citrine_gb::gb::cpu::Cpu;
use citrine_gb::instructions::{Instruction, Operand};
use egui::{Response, ScrollArea, Stroke, Ui, Widget};
use std::collections::HashSet;

pub struct DisassemblyView<'a> {
    cpu: &'a Cpu,
    cartridge: &'a Cartridge,
    disassembly: &'a Disassembly,
    breakpoints: &'a mut HashSet<RomLocation>,
    static_analysis_enabled: &'a mut bool,
    track_pc: &'a mut bool,
    row_height: f32,
}

impl<'a> DisassemblyView<'a> {
    pub fn new(
        cpu: &'a Cpu,
        cartridge: &'a Cartridge,
        disassembly: &'a Disassembly,
        breakpoints: &'a mut HashSet<RomLocation>,
        static_analysis_enabled: &'a mut bool,
        track_pc: &'a mut bool,
    ) -> Self {
        Self {
            cpu,
            cartridge,
            disassembly,
            breakpoints,
            static_analysis_enabled,
            track_pc,
            row_height: 20.0,
        }
    }

    fn draw_row(
        &mut self,
        ui: &mut Ui,
        index: usize,
        current_loc: RomLocation,
        font: &egui::FontId,
        is_dark_mode: bool,
    ) {
        let Some(decoded) = self.disassembly.get_by_index(index) else {
            return;
        };

        let end_loc = decoded.loc.offset(decoded.instruction.length() as i16);
        let is_pc = current_loc >= decoded.loc && current_loc < end_loc;
        let is_bp = self.breakpoints.contains(&decoded.loc);

        let (rect, row_response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), self.row_height),
            egui::Sense::click(),
        );

        row_response.context_menu(|ui| {
            ui.label(format!("Location: {}", decoded.loc));
            ui.separator();
            ui.horizontal(|ui| {
                ui.small(
                    decoded
                        .instruction_bytes()
                        .iter()
                        .map(|b| format!("0x{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                ui.small("•");
                ui.small(format!("{} machine cycles", decoded.machine_cycles()));
                ui.small("•");
                match decoded.confidence {
                    Confidence::Fetched => ui.small("Was fetched by the CPU"),
                    Confidence::Unconditional => {
                        ui.small("Reachable through unconditional branching")
                    }
                    Confidence::Conditional => ui.small("Reachable through conditional branching"),
                }
            });
            ui.label(decoded.educational_text());
        });

        let gutter_width = 24.0;
        let bp_rect = egui::Rect::from_min_size(rect.min, egui::vec2(gutter_width, rect.height()));
        let bp_hovered = ui.rect_contains_pointer(bp_rect);

        if bp_hovered && row_response.clicked() && !self.breakpoints.remove(&decoded.loc) {
            self.breakpoints.insert(decoded.loc);
        }

        let row_hovered = row_response.hovered() && !bp_hovered;
        let bg = if is_pc {
            ui.visuals().selection.bg_fill
        } else if row_hovered {
            if is_dark_mode {
                egui::Color32::from_white_alpha(15)
            } else {
                egui::Color32::from_black_alpha(10)
            }
        } else if index.is_multiple_of(2) {
            ui.visuals().faint_bg_color
        } else {
            egui::Color32::TRANSPARENT
        };

        if bg != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 0.0, bg);
        }

        let bp_center = egui::pos2(rect.left() + 10.0, rect.center().y);
        if is_bp {
            ui.painter()
                .circle_filled(bp_center, 4.0, ui.visuals().error_fg_color);
        } else if bp_hovered {
            ui.painter().circle_stroke(
                bp_center,
                4.0,
                Stroke::new(1.0, ui.visuals().error_fg_color),
            );
        }

        let y = rect.center().y;

        ui.painter().text(
            egui::pos2(rect.left() + gutter_width, y),
            egui::Align2::LEFT_CENTER,
            decoded.loc.to_string(),
            font.clone(),
            ui.visuals().weak_text_color(),
        );

        let mnemonic_color = instruction_color(&decoded.instruction, is_dark_mode);
        ui.painter().text(
            egui::pos2(rect.left() + gutter_width + 70.0, y),
            egui::Align2::LEFT_CENTER,
            decoded.instruction.mnemonic(),
            font.clone(),
            mnemonic_color,
        );

        let operands = decoded.instruction.operands(&decoded.ctx);
        let mut op_x = gutter_width + 115.0;
        for op in operands.iter().flatten() {
            ui.painter().text(
                egui::pos2(rect.left() + op_x, y),
                egui::Align2::LEFT_CENTER,
                op.to_string(),
                font.clone(),
                operand_color(op, is_dark_mode),
            );
            op_x += 45.0;
        }

        let (conf_str, conf_color) = match decoded.confidence {
            Confidence::Fetched => ("Fetch", egui::Color32::from_rgb(100, 200, 100)),
            Confidence::Unconditional => ("Uncond", egui::Color32::from_rgb(200, 200, 100)),
            Confidence::Conditional => ("Cond", egui::Color32::from_rgb(200, 150, 100)),
        };

        ui.painter().text(
            egui::pos2(rect.right() - 8.0, y),
            egui::Align2::RIGHT_CENTER,
            conf_str,
            font.clone(),
            conf_color,
        );
    }
}

impl Widget for DisassemblyView<'_> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        let active_pc = self.cpu.pc.saturating_sub(1);
        let current_loc = self.cartridge.probe_rom_location(active_pc);
        let row_count = self.disassembly.len();
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let is_dark_mode = ui.visuals().dark_mode;

        ui.horizontal(|ui| {
            ui.toggle_value(self.track_pc, "Follow PC");
            ui.separator();
            ui.checkbox(self.static_analysis_enabled, "Static Analysis");
            ui.separator();
            ui.label(format!("{} Instructions", row_count));
        });

        ui.separator();

        let item_spacing = ui.spacing().item_spacing.y;
        let effective_row_height = self.row_height + item_spacing;

        let mut scroll_area = ScrollArea::vertical().auto_shrink(false);

        if *self.track_pc
            && let Some(pc_index) = self.disassembly.iter().position(|d| {
                let end_loc = d.loc.offset(d.instruction.length() as i16);
                current_loc >= d.loc && current_loc < end_loc
            })
        {
            let item_top = pc_index as f32 * effective_row_height;
            let half_screen = ui.available_height() / 2.0;
            let center_offset = item_top - half_screen + (effective_row_height / 2.0);
            scroll_area = scroll_area.vertical_scroll_offset(center_offset.max(0.0));
        }

        let row_height = self.row_height;
        let output = scroll_area.show_rows(ui, row_height, row_count, |ui, range| {
            for i in range {
                ui.push_id(i, |ui| {
                    self.draw_row(ui, i, current_loc, &font, is_dark_mode);
                });
            }
        });

        ui.allocate_rect(output.inner_rect, egui::Sense::hover())
    }
}

fn instruction_color(instr: &Instruction, is_dark_mode: bool) -> egui::Color32 {
    match instr {
        Instruction::JP_nn
        | Instruction::JP_c_nn(_)
        | Instruction::JR_n
        | Instruction::JR_c_n(_)
        | Instruction::JP_HL => {
            if is_dark_mode {
                egui::Color32::from_rgb(150, 255, 150)
            } else {
                egui::Color32::from_rgb(0, 150, 0)
            }
        }
        Instruction::CALL_nn | Instruction::CALL_c_nn(_) | Instruction::RST_n(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(150, 200, 255)
            } else {
                egui::Color32::from_rgb(0, 0, 200)
            }
        }
        Instruction::RET | Instruction::RETI | Instruction::RET_c(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(255, 150, 150)
            } else {
                egui::Color32::from_rgb(200, 0, 0)
            }
        }
        Instruction::PUSH(_) | Instruction::POP(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(220, 150, 255)
            } else {
                egui::Color32::from_rgb(120, 0, 150)
            }
        }
        Instruction::LD_rr_nn(_)
        | Instruction::LD_rr_A(_)
        | Instruction::LD_A_rr(_)
        | Instruction::LD_nn_SP
        | Instruction::LD_r_n(_)
        | Instruction::LD_r_r(_, _)
        | Instruction::LDH_C_A
        | Instruction::LDH_A_C
        | Instruction::LDH_n_A
        | Instruction::LDH_A_n
        | Instruction::LD_nn_A
        | Instruction::LD_A_nn
        | Instruction::LD_HL_SP_n
        | Instruction::LD_SP_HL => {
            if is_dark_mode {
                egui::Color32::from_rgb(120, 255, 255)
            } else {
                egui::Color32::from_rgb(0, 120, 120)
            }
        }
        Instruction::INC_rr(_)
        | Instruction::DEC_rr(_)
        | Instruction::ADD_HL_rr(_)
        | Instruction::INC_r(_)
        | Instruction::DEC_r(_)
        | Instruction::ADD_r(_)
        | Instruction::ADC_r(_)
        | Instruction::SUB_r(_)
        | Instruction::SBC_r(_)
        | Instruction::AND_r(_)
        | Instruction::XOR_r(_)
        | Instruction::OR_r(_)
        | Instruction::CP_r(_)
        | Instruction::ADD_n
        | Instruction::ADC_n
        | Instruction::SUB_n
        | Instruction::SBC_n
        | Instruction::AND_n
        | Instruction::XOR_n
        | Instruction::OR_n
        | Instruction::CP_n
        | Instruction::ADD_SP_n
        | Instruction::DAA => {
            if is_dark_mode {
                egui::Color32::from_rgb(255, 200, 120)
            } else {
                egui::Color32::from_rgb(160, 80, 0)
            }
        }
        Instruction::RLCA
        | Instruction::RRCA
        | Instruction::RLA
        | Instruction::RRA
        | Instruction::RLC_r(_)
        | Instruction::RRC_r(_)
        | Instruction::RL_r(_)
        | Instruction::RR_r(_)
        | Instruction::SLA_r(_)
        | Instruction::SRA_r(_)
        | Instruction::SWAP_r(_)
        | Instruction::SRL_r(_)
        | Instruction::BIT_r(_, _)
        | Instruction::RES_r(_, _)
        | Instruction::SET_r(_, _)
        | Instruction::CPL
        | Instruction::SCF
        | Instruction::CCF => {
            if is_dark_mode {
                egui::Color32::from_rgb(255, 150, 200)
            } else {
                egui::Color32::from_rgb(180, 0, 120)
            }
        }
        Instruction::NOP
        | Instruction::STOP
        | Instruction::HALT
        | Instruction::DI
        | Instruction::EI => {
            if is_dark_mode {
                egui::Color32::from_rgb(150, 150, 150)
            } else {
                egui::Color32::from_rgb(100, 100, 100)
            }
        }
        Instruction::Invalid(_) => egui::Color32::RED,
    }
}

fn operand_color(op: &Operand, is_dark_mode: bool) -> egui::Color32 {
    match op {
        Operand::Reg(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(255, 180, 140)
            } else {
                egui::Color32::from_rgb(180, 100, 30)
            }
        }
        Operand::Cond(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(220, 220, 160)
            } else {
                egui::Color32::from_rgb(130, 130, 0)
            }
        }
        Operand::MemReg(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(160, 220, 255)
            } else {
                egui::Color32::from_rgb(0, 120, 180)
            }
        }
        Operand::Imm8(_) | Operand::Imm16(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(180, 255, 180)
            } else {
                egui::Color32::from_rgb(30, 130, 30)
            }
        }
        Operand::Address(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(240, 180, 255)
            } else {
                egui::Color32::from_rgb(140, 30, 160)
            }
        }
        Operand::Offset(_) | Operand::SpOffset(_) => {
            if is_dark_mode {
                egui::Color32::from_rgb(160, 255, 230)
            } else {
                egui::Color32::from_rgb(0, 140, 120)
            }
        }
    }
}
